"""Galerkin Rayleigh load of constant rectangular surface cells, SI units.

exp(+i omega t): Z(s)=rho*s/(2*pi) integral integral phi(x)phi(y)
exp(-s*|x-y|/c)/|x-y| dSx dSy. Both resistance and reactance come from
the same outgoing kernel. A rectangular overlap integral includes each
cell's self interaction; a Duffy transform removes its integrable singularity.
This is frequency-domain data, not a causal reduced time-domain realization.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.fft import fft2,ifft2,next_fast_len


def average_basis(length,count,edges,boundary):
    mid=(edges[:-1]+edges[1:])/2;h=np.diff(edges)
    k=np.pi*np.arange(1,count+1)/length
    if boundary=='simply-supported':
        return np.sin(mid[:,None]*k)*np.sinc(h[:,None]*k/(2*np.pi))
    if boundary!='clamped':raise ValueError(boundary)
    q=np.pi/length
    return .5*(np.cos(mid[:,None]*(k-q))*np.sinc(h[:,None]*(k-q)/(2*np.pi))
               -np.cos(mid[:,None]*(k+q))*np.sinc(h[:,None]*(k+q)/(2*np.pi)))


class RectangularLoad:
    def __init__(self,length,width,shapes,density=1.2,speed=343.):
        """shapes[xcell,ycell,mode], constant modal shape in kg^-1/2."""
        self.shapes=np.asarray(shapes,dtype=float)
        if self.shapes.ndim!=3 or min(self.shapes.shape)==0 or not np.isfinite(self.shapes).all():raise ValueError('surface shapes')
        if not np.isfinite([length,width,density,speed]).all() or min(length,width,density,speed)<=0:raise ValueError('geometry/air')
        self.length=length;self.width=width;self.density=density;self.speed=speed
        self.nx,self.ny,self.modes=self.shapes.shape
        self.hx=length/self.nx;self.hy=width/self.ny
        # Toeplitz-to-circulant embedding with sufficient zero padding.
        self.fft_size=(next_fast_len(2*self.nx-1),next_fast_len(2*self.ny-1))
        self.shape_fft=fft2(self.shapes,s=self.fft_size,axes=(0,1))

    @classmethod
    def from_plate(cls,model,nx,ny,**kwargs):
        length=model['panel']['length'];width=model['panel']['width']
        bx=average_basis(length,model['nx'],np.linspace(0,length,nx+1),model['boundary'])
        by=average_basis(width,model['ny'],np.linspace(0,width,ny+1),model['boundary'])
        vectors=model['vectors'].reshape(model['nx'],model['ny'],-1)
        shapes=np.einsum('xa,abm,yb->xym',bx,vectors,by,optimize=True)
        return cls(length,width,shapes,**kwargs)

    def overlap_kernel(self,s,order=8):
        """Double area integral per pair displacement; units m^3.

        Convolution of two cell indicators is (hx-|u|)(hy-|v|).
        Nonlocal entries use composite Gauss quadrature across u=v=0.
        Local and touching-cell singular quadrants use two triangles each:
        the Jacobian cancels 1/r, including zero overlap at a touching edge.
        """
        if order<2 or int(order)!=order or not np.isfinite(s):raise ValueError('kernel quadrature/s')
        g,w=leggauss(order);t=(g+1)/2;wt=w/2
        dx=np.arange(self.nx)[:,None]*self.hx;dy=np.arange(self.ny)[None,:]*self.hy
        out=np.zeros((self.nx,self.ny),dtype=complex)
        for a,wa in zip(t,wt):
            for b,wb in zip(t,wt):
                for sx in (-1,1):
                    for sy in (-1,1):
                        r=np.hypot(dx+sx*self.hx*a,dy+sy*self.hy*b)
                        out+=wa*wb*self.hx**2*self.hy**2*(1-a)*(1-b)*np.exp(-s*r/self.speed)/r
        for i in range(min(2,self.nx)):
            for j in range(min(2,self.ny)):
                # Remove just those old quadrants whose closure contains r=0.
                mx=2 if i==0 else 1;my=2 if j==0 else 1
                old=0j;regular=0j
                for a,wa in zip(t,wt):
                    for b,wb in zip(t,wt):
                        r=np.hypot(self.hx*(a if i==0 else 1-a),self.hy*(b if j==0 else 1-b))
                        old+=wa*wb*self.hx**2*self.hy**2*(1-a)*(1-b)*np.exp(-s*r/self.speed)/r
                        for swap in (False,True):
                            x=self.hx*a*(b if swap else 1)
                            y=self.hy*a*(1 if swap else b)
                            radial=np.hypot(self.hx*(b if swap else 1),self.hy*(1 if swap else b))
                            overlap_x=x if i else self.hx-x
                            overlap_y=y if j else self.hy-y
                            regular+=wa*wb*self.hx*self.hy*overlap_x*overlap_y*np.exp(-s*a*radial/self.speed)/radial
                out[i,j]+=mx*my*(regular-old)
        return out

    def project_kernel(self,kernel):
        """Symmetric modal projection without assembling a dense cell matrix."""
        padded=np.zeros(self.fft_size,dtype=complex)
        ix=np.arange(self.fft_size[0]);iy=np.arange(self.fft_size[1])
        ix=np.minimum(ix,self.fft_size[0]-ix);iy=np.minimum(iy,self.fft_size[1]-iy)
        valid_x=ix<self.nx;valid_y=iy<self.ny
        padded[np.ix_(valid_x,valid_y)]=kernel[np.ix_(ix[valid_x],iy[valid_y])]
        convolution=ifft2(fft2(padded)[:,:,None]*self.shape_fft,axes=(0,1))[:self.nx,:self.ny]
        return self.shapes.reshape(-1,self.modes).T@convolution.reshape(-1,self.modes)

    def impedance(self,s,order=8):
        """Force/velocity modal impedance, s^-1. No eigenvalue clipping."""
        s=complex(s)
        return self.density*s/(2*np.pi)*self.project_kernel(self.overlap_kernel(s,order))

    def added_mass_static(self,order=8):
        """lim(s->0) Z(s)/s, dimensionless in mass-normalized coordinates."""
        return (self.density/(2*np.pi)*self.project_kernel(self.overlap_kernel(0.,order))).real

    def angular_resistance(self,frequency,polar=24,azimuth=48):
        """Independent hemisphere Gram for the same constant cells.

        Integrates each rectangle analytically in the far field (sinc factors),
        avoiding treating finite cells as point radiators.
        """
        g,w=leggauss(polar);u=(g+1)/2;w=w/2
        az=2*np.pi*(np.arange(azimuth)+.5)/azimuth
        sintheta,angle=np.meshgrid(np.sqrt(1-u*u),az,indexing='ij')
        directions=np.c_[(sintheta*np.cos(angle)).ravel(),(sintheta*np.sin(angle)).ravel()]
        weights=np.repeat(w,azimuth)*2*np.pi/azimuth
        x,y=np.meshgrid((np.arange(self.nx)+.5)*self.hx,(np.arange(self.ny)+.5)*self.hy,indexing='ij')
        path=directions@np.c_[x.ravel(),y.ravel()].T
        omega=2*np.pi*frequency;k=omega/self.speed
        aperture=np.sinc(k*self.hx*directions[:,0]/(2*np.pi))*np.sinc(k*self.hy*directions[:,1]/(2*np.pi))
        far=(np.exp(1j*k*path)@self.shapes.reshape(-1,self.modes))*((self.hx*self.hy)*aperture[:,None])
        return self.density*omega**2/(4*np.pi**2*self.speed)*(far.conj().T@(weights[:,None]*far)).real

    def angular_resistance_streamed(self,frequency,polar=24,azimuth=48,block=64):
        """Same exact-cell hemisphere Gram, with bounded temporary memory."""
        if block<1:raise ValueError('positive direction block')
        g,w=leggauss(polar);u=(g+1)/2;w=w/2
        az=2*np.pi*(np.arange(azimuth)+.5)/azimuth
        sintheta,angle=np.meshgrid(np.sqrt(1-u*u),az,indexing='ij')
        directions=np.c_[(sintheta*np.cos(angle)).ravel(),(sintheta*np.sin(angle)).ravel()]
        weights=np.repeat(w,azimuth)*2*np.pi/azimuth
        x,y=np.meshgrid((np.arange(self.nx)+.5)*self.hx,(np.arange(self.ny)+.5)*self.hy,indexing='ij')
        positions=np.c_[x.ravel(),y.ravel()];shapes=self.shapes.reshape(-1,self.modes)
        omega=2*np.pi*frequency;k=omega/self.speed;gram=np.zeros((self.modes,self.modes),dtype=complex)
        for start in range(0,len(directions),block):
            d=directions[start:start+block];weight=weights[start:start+block]
            aperture=np.sinc(k*self.hx*d[:,0]/(2*np.pi))*np.sinc(k*self.hy*d[:,1]/(2*np.pi))
            far=(np.exp(1j*k*(d@positions.T))@shapes)*((self.hx*self.hy)*aperture[:,None])
            gram+=far.conj().T@(weight[:,None]*far)
        return (self.density*omega**2/(4*np.pi**2*self.speed)*gram).real
