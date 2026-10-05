"""Continuous Rayleigh feedback without modal/cell dense air matrices.

Same constant-cell integral as RectangularLoad, including Duffy self terms.
Separable Ritz maps and their exact transpose retain mechanical reciprocity.
This is a frequency-domain verification, NOT a causal time realization.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.fft import fft2,ifft2
from scipy.linalg import lu_factor,lu_solve
from scipy.sparse.linalg import LinearOperator,gmres
from concert_grand_surface_impedance import RectangularLoad,average_basis
from concert_grand_cell_observer import CellObserver


class RitzSurface:
    def __init__(self,model,cells,symmetric_y=True):
        self.length=model['panel']['length'];self.width=model['panel']['width']
        self.nx,self.ny=map(int,cells);self.hx=self.length/self.nx;self.hy=self.width/self.ny
        self.density=1.2;self.speed=343.
        nx,ny=model['nx'],model['ny'];vectors=np.asarray(model['vectors'])
        if symmetric_y:
            keep=np.arange(0,ny,2);omitted=np.arange(1,ny,2)
            rows=(np.arange(nx)[:,None]*ny+omitted).ravel()
            if np.any(vectors[rows]!=0):raise ValueError('exact y-symmetric coefficient input required')
        else:keep=np.arange(ny)
        rows=(np.arange(nx)[:,None]*ny+keep).ravel()
        self.vectors=np.ascontiguousarray(vectors[rows]);self.modes=vectors.shape[1]
        self.bx=average_basis(self.length,nx,np.linspace(0,self.length,self.nx+1),model['boundary'])
        self.by=average_basis(self.width,ny,np.linspace(0,self.width,self.ny+1),model['boundary'])[:,keep]
        self.ritz_shape=(nx,len(keep))
        self.geometry=RectangularLoad(self.length,self.width,np.ones((self.nx,self.ny,1)))

    def forward(self,coordinates):
        q=np.asarray(coordinates);vector=q.ndim==1
        if vector:q=q[:,None]
        if q.ndim!=2 or q.shape[0]!=self.modes:raise ValueError('modal input dimensions')
        coefficients=(self.vectors@q).reshape(*self.ritz_shape,q.shape[1])
        field=np.einsum('xa,abp,yb->xyp',self.bx,coefficients,self.by,optimize=True)
        return field[:,:,0] if vector else field

    def adjoint(self,field):
        field=np.asarray(field);vector=field.ndim==2
        if vector:field=field[:,:,None]
        if field.ndim!=3 or field.shape[:2]!=(self.nx,self.ny):raise ValueError('cell force dimensions')
        coefficients=np.einsum('xa,xyp,yb->abp',self.bx,field,self.by,optimize=True)
        result=self.vectors.T@coefficients.reshape(len(self.vectors),field.shape[2])
        return result[:,0] if vector else result

    def pressure(self,frequency,coordinates,microphones,order=4):
        observer=CellObserver(self.geometry,microphones,order)
        s=2j*np.pi*frequency
        green=np.sum(observer.area[:,None,None]*np.exp(-s*observer.distance/self.speed)/observer.distance,axis=0)
        field=self.forward(coordinates).reshape(self.nx*self.ny,-1)
        return s*s*self.density/(2*np.pi)*(green@field)

    def angular_power(self,frequency,velocity,polar=96,azimuth=256,block=128):
        """Independent exact-cell far field, complex source, mean watts."""
        g,weights=leggauss(polar);u=(g+1)/2;weights=weights/2
        angles=2*np.pi*(np.arange(azimuth)+.5)/azimuth
        radial=np.sqrt(1-u*u)
        dx=(radial[:,None]*np.cos(angles)[None]).ravel()
        dy=(radial[:,None]*np.sin(angles)[None]).ravel()
        weights=np.repeat(weights,azimuth)*2*np.pi/azimuth
        k=2*np.pi*frequency/self.speed
        x=(np.arange(self.nx)+.5)*self.hx;y=(np.arange(self.ny)+.5)*self.hy
        v=np.asarray(velocity)
        if v.ndim==1:v=v[:,None]
        coefficients=(self.vectors@v).reshape(*self.ritz_shape,v.shape[1])
        if block<1:raise ValueError('positive angular block')
        power=np.zeros(v.shape[1])
        for first in range(0,len(dx),block):
            xx=dx[first:first+block];yy=dy[first:first+block]
            ax=np.exp(1j*k*xx[:,None]*x[None])@self.bx
            ay=np.exp(1j*k*yy[:,None]*y[None])@self.by
            far=np.einsum('da,abp,db->dp',ax,coefficients,ay,optimize=True)
            aperture=self.hx*self.hy*np.sinc(k*self.hx*xx/(2*np.pi))*np.sinc(k*self.hy*yy/(2*np.pi))
            far*=aperture[:,None];power+=weights[first:first+block]@abs(far)**2
        return self.density*(2*np.pi*frequency)**2/(8*np.pi**2*self.speed)*power


class CellAir:
    def __init__(self,surface,s,order=8):
        self.surface=surface;self.s=complex(s);geometry=surface.geometry
        kernel=geometry.overlap_kernel(self.s,order);shape=geometry.fft_size
        ix=np.arange(shape[0]);iy=np.arange(shape[1]);ix=np.minimum(ix,shape[0]-ix);iy=np.minimum(iy,shape[1]-iy)
        vx=ix<surface.nx;vy=iy<surface.ny;padded=np.zeros(shape,complex)
        padded[np.ix_(vx,vy)]=kernel[np.ix_(ix[vx],iy[vy])]
        self.kernel_fft=fft2(padded);self.fft_size=shape

    def force(self,velocity):
        field=self.surface.forward(velocity);vector=field.ndim==2
        if vector:field=field[:,:,None]
        convolution=ifft2(self.kernel_fft[:,:,None]*fft2(field,s=self.fft_size,axes=(0,1)),axes=(0,1))[:self.surface.nx,:self.surface.ny]
        result=self.surface.adjoint(convolution)*self.surface.density*self.s/(2*np.pi)
        return result[:,0] if vector else result


class MechanicalDynamic:
    def __init__(self,frequency,hz,loss=.046,projected=None):
        self.s=2j*np.pi*hz;omega=2*np.pi*np.asarray(frequency)
        self.modes=len(omega);self.low=self.modes if projected is None else int(projected['low_count'])
        self.low_damping=loss*omega[:self.low] if projected is None else projected['low_damping']
        self.diagonal=omega[:self.low]**2+self.s*self.low_damping+self.s*self.s
        self.tail=None if projected is None else (projected['tail_stiffness']+self.s*projected['tail_damping']+self.s*self.s*projected['tail_mass'])
        if self.tail is not None:
            self.factor=lu_factor(self.tail);self.tail_damping=projected['tail_damping']

    def solve(self,force):
        b=np.asarray(force);vector=b.ndim==1
        if vector:b=b[:,None]
        result=np.empty_like(b,dtype=complex);result[:self.low]=b[:self.low]/self.diagonal[:,None]
        if self.tail is not None:result[self.low:]=lu_solve(self.factor,b[self.low:])
        return result[:,0] if vector else result

    def apply(self,coordinates):
        q=np.asarray(coordinates);vector=q.ndim==1
        if vector:q=q[:,None]
        out=np.empty_like(q,dtype=complex);out[:self.low]=self.diagonal[:,None]*q[:self.low]
        if self.tail is not None:out[self.low:]=self.tail@q[self.low:]
        return out[:,0] if vector else out

    def heat(self,velocity):
        v=np.asarray(velocity)
        if v.ndim==1:v=v[:,None]
        result=np.sum(self.low_damping[:,None]*abs(v[:self.low])**2,axis=0)
        if self.tail is not None:result+=np.sum((v[self.low:].conj()*(self.tail_damping@v[self.low:])).real,axis=0)
        return result/2


def block_gmres(action,B,tolerance=1e-10,max_steps=20):
    """Unrestarted block Arnoldi with two-pass MGS and true-residual approval.

    All right hand sides are advanced by the same physical operator. QR
    near-breakdowns may add irrelevant columns but never repair/filter forces.
    A least-squares estimate triggers a separate actual residual evaluation.
    """
    B=np.asarray(B,dtype=complex);p=B.shape[1];scale=np.maximum(np.linalg.norm(B,axis=0),1e-40)
    initial=B.copy();residual=B-action(initial)
    if (np.linalg.norm(residual,axis=0)/scale).max()<tolerance:return initial,0
    V,beta=np.linalg.qr(residual,mode='reduced');blocks=[V]
    H=np.zeros(((max_steps+1)*p,max_steps*p),complex)
    G=np.zeros(((max_steps+1)*p,p),complex);G[:p]=beta
    for j in range(max_steps):
        W=action(blocks[j])
        for _ in range(2):
            for i in range(j+1):
                product=blocks[i].conj().T@W
                H[i*p:(i+1)*p,j*p:(j+1)*p]+=product;W-=blocks[i]@product
        next_block,h=np.linalg.qr(W,mode='reduced')
        H[(j+1)*p:(j+2)*p,j*p:(j+1)*p]=h
        hess=H[:(j+2)*p,:(j+1)*p];rhs=G[:(j+2)*p]
        coefficients=np.linalg.lstsq(hess,rhs,rcond=None)[0]
        predicted=np.linalg.norm(rhs-hess@coefficients,axis=0)/scale
        if predicted.max()<tolerance:
            actual=initial+np.concatenate(blocks,axis=1)@coefficients
            true=np.linalg.norm(B-action(actual),axis=0)/scale
            if true.max()<tolerance*2:return actual,j+1
        blocks.append(next_block)
    raise RuntimeError('block GMRES exhausted iterations; no approximate approval')


def solve_loaded(mechanical,air,forces,tolerance=1e-10,restart=40,maxiter=20,solver='block'):
    """Right-preconditioned GMRES; full original force residual is checked."""
    B=np.asarray(forces);B=B[:,None] if B.ndim==1 else B
    if B.ndim!=2 or B.shape[0]!=mechanical.modes or air.surface.modes!=mechanical.modes:
        raise ValueError('shared mechanics/air/ports dimensions')
    def action(u):return u+mechanical.s*air.force(mechanical.solve(u))
    if solver=='block':
        u,steps=block_gmres(action,B,tolerance,maxiter)
        coordinates=mechanical.solve(u);iterations=[steps]*B.shape[1]
    elif solver=='gmres':
        op=LinearOperator((len(B),len(B)),matvec=action,dtype=complex)
        coordinates=np.empty_like(B,dtype=complex);iterations=[]
        for j in range(B.shape[1]):
            history=[];u,info=gmres(op,B[:,j],x0=B[:,j].astype(complex),rtol=tolerance,atol=0,
                restart=restart,maxiter=maxiter,callback=history.append,callback_type='pr_norm')
            if info!=0:raise RuntimeError(f'loaded GMRES did not converge: {info}')
            coordinates[:,j]=mechanical.solve(u);iterations.append(len(history))
    else:raise ValueError('block or gmres solver required')
    velocity=mechanical.s*coordinates;air_force=air.force(velocity)
    residual=mechanical.apply(coordinates)+air_force-B
    relative=np.linalg.norm(residual,axis=0)/np.maximum(np.linalg.norm(B,axis=0),1e-40)
    if relative.max()>tolerance*2:raise AssertionError('original loaded force residual')
    input_power=np.sum((velocity.conj()*B).real,axis=0)/2
    air_power=np.sum((velocity.conj()*air_force).real,axis=0)/2;heat=mechanical.heat(velocity)
    power_error=np.abs(input_power-air_power-heat)/np.maximum(abs(input_power),1e-40)
    return coordinates,dict(iterations=iterations,force_residual=relative.tolist(),
        input_power=input_power.tolist(),viscous_heat=heat.tolist(),air_power=air_power.tolist(),power_balance_relative=power_error.tolist())
