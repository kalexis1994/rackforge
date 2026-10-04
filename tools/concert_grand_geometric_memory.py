"""Direct continuous-time Rayleigh memory of constant rectangular cells.

Z(s)=s*Laplace[K](s), K(t)=rho*c/(2*pi) integral over a radius-c*t
circle of the cell-overlap function, projected into the physical modal basis.
The angular integral is piecewise elementary, including local/touching cells.
No frequency grid, fitted poles, delay histogram or inertial tail is used.
This prescribed-port evaluator is not a passive coupled time-stepper.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.fft import ifft2
from scipy.sparse import csr_matrix
from scipy.signal import fftconvolve


def ring_overlap(hx,hy,dx,dy,radius):
    """Exact circle integral of (hx-|x-dx|)+ (hy-|y-dy|)+.

    Break each angular arc at overlap edges and absolute-value cusps.
    Within an arc integrate A+B*cos+C*sin+D*cos*sin analytically.
    """
    dx,dy=np.broadcast_arrays(np.asarray(dx,dtype=float),np.asarray(dy,dtype=float))
    if min(hx,hy)<=0 or radius<0 or not np.isfinite(radius):raise ValueError('positive cells/nonnegative radius')
    if radius==0:return 2*np.pi*np.maximum(hx-abs(dx),0)*np.maximum(hy-abs(dy),0)
    flatx=dx.ravel();flaty=dy.ravel();roots=[np.zeros(len(flatx)),np.full(len(flatx),2*np.pi)]
    for offset in (-hx,0.,hx):
        value=(flatx+offset)/radius;valid=abs(value)<=1
        angle=np.arccos(np.clip(value,-1.,1.))
        roots.extend((np.where(valid,angle,0.),np.where(valid,2*np.pi-angle,0.)))
    for offset in (-hy,0.,hy):
        value=(flaty+offset)/radius;valid=abs(value)<=1
        angle=np.arcsin(np.clip(value,-1.,1.))
        roots.extend((np.where(valid,angle%(2*np.pi),0.),np.where(valid,(np.pi-angle)%(2*np.pi),0.)))
    edges=np.sort(np.array(roots).T,axis=1);left=edges[:,:-1];right=edges[:,1:];mid=(left+right)/2
    delta_x=radius*np.cos(mid)-flatx[:,None];delta_y=radius*np.sin(mid)-flaty[:,None]
    valid=(abs(delta_x)<hx)&(abs(delta_y)<hy)
    sx=np.where(delta_x>=0,1.,-1.);sy=np.where(delta_y>=0,1.,-1.)
    bx=-sx*radius;by=-sy*radius;xm=hx-abs(delta_x);ym=hy-abs(delta_y)
    half=(right-left)/2;small=half<.03;h2=half*half
    # Center the primitive on the arc: large dx/dy terms otherwise cancel
    # to tiny overlap weights near the far edge of a fine surface grid.
    first=np.where(small,half*h2*(-1/3+h2*(1/60-h2/2520)),2*(np.sin(half)-half))
    mixed=np.where(small,half*h2*(-2/3+h2*(7/30+h2*(-31/1260+h2*127/90720))),np.sin(2*half)-4*np.sin(half)+2*half)
    integral=(xm*ym*2*half+(bx*ym*np.cos(mid)+by*xm*np.sin(mid))*first
              +bx*by*np.sin(mid)*np.cos(mid)*mixed)
    result=np.sum(np.where(valid,integral,0.),axis=1)
    tolerance=1e-12*hx*hy
    if result.min(initial=0)<-tolerance:raise ArithmeticError('negative overlap beyond angular roundoff')
    return np.maximum(result,0.).reshape(dx.shape)


class GeometricMemory:
    def __init__(self,surface):
        self.surface=surface;self.modes=surface.modes
        self.max_delay=np.hypot(surface.length,surface.width)/surface.speed
        self.upper=np.triu_indices(self.modes)
        # Modal correlations group all signs of a cell displacement. This
        # contracts any even Toeplitz kernel without repeated surface FFTs.
        nx,ny=surface.nx,surface.ny;fx,fy=surface.fft_size
        negx=(-np.arange(nx))%fx;negy=(-np.arange(ny))%fy
        self.correlation=np.empty((nx*ny,len(self.upper[0])))
        column=0
        for m in range(self.modes):
            corr=ifft2(surface.shape_fft[:,:,m].conj()[:,:,None]*surface.shape_fft[:,:,m:],axes=(0,1)).real
            grouped=corr[:nx,:ny].copy()
            grouped[1:]+=corr[negx[1:],:ny]
            grouped[:,1:]+=corr[:nx,negy[1:]]
            grouped[1:,1:]+=corr[negx[1:,None],negy[None,1:]]
            count=self.modes-m
            self.correlation[:,column:column+count]=grouped.reshape(-1,count)
            column+=count
        xx,yy=np.meshgrid(np.arange(nx)*surface.hx,np.arange(ny)*surface.hy,indexing='ij')
        self.dx=xx.ravel();self.dy=yy.ravel()
        self.near=np.hypot(np.maximum(self.dx-surface.hx,0),np.maximum(self.dy-surface.hy,0))
        self.far=np.hypot(self.dx+surface.hx,self.dy+surface.hy)

    def overlap_rows(self,time):
        time=np.asarray(time,dtype=float).reshape(-1)
        if not np.isfinite(time).all() or (time<0).any():raise ValueError('nonnegative finite times')
        values=[];indices=[];pointers=[0];s=self.surface
        for t in time:
            radius=t*s.speed
            active=np.flatnonzero((self.near<=radius)&(self.far>=radius))
            weight=ring_overlap(s.hx,s.hy,self.dx[active],self.dy[active],radius)
            keep=weight>0;values.extend(weight[keep]);indices.extend(active[keep]);pointers.append(len(values))
        return csr_matrix((values,indices,pointers),shape=(len(time),len(self.dx)))

    def unpack(self,packed):
        out=np.zeros((len(packed),self.modes,self.modes),dtype=np.result_type(packed,float))
        out[:,self.upper[0],self.upper[1]]=packed
        out[:,self.upper[1],self.upper[0]]=packed
        return out

    def kernel(self,time):
        """K(t), including K(0)=rho*c*integral phi*phi.T and finite support."""
        s=self.surface
        return self.unpack((self.overlap_rows(time)@self.correlation)*(s.density*s.speed/(2*np.pi)))

    def quadrature_kernel(self,dt,quadrature=8):
        """Continuous samples for quadrature, not a rasterized delay kernel."""
        if dt<=0 or not np.isfinite(dt) or quadrature<2:raise ValueError('time grid/quadrature')
        count=int(np.ceil(self.max_delay/dt));nodes,weights=leggauss(quadrature)
        packed=np.empty((count,quadrature,len(self.upper[0])));s=self.surface
        all_times=(np.arange(count)[:,None]+(nodes[None,:]+1)/2)*dt
        for start in range(0,count,128):
            stop=min(start+128,count)
            times=all_times[start:stop]
            rows=self.overlap_rows(times.ravel())
            values=(rows@self.correlation).reshape(stop-start,quadrature,-1)
            packed[start:stop]=values
        return all_times,weights*dt/2,packed*(s.density*s.speed/(2*np.pi))

    def integrated_kernel(self,dt,quadrature=8):
        """Integrals on uniform intervals; only the physical support is stored."""
        times,weights,packed=self.quadrature_kernel(dt,quadrature)
        return self.unpack(np.einsum('q,tqm->tm',weights,packed))

    def linear_drive_force(self,velocity,dt,coast_steps,quadrature=8,integration=None):
        """Piecewise linear velocity, then an exact jump to a stationary port.

        Force at the stop knot uses the left limit. No delta tail exists in
        this geometric model. Finite propagation support is physical, and
        FFT convolution is zero-padded; no output threshold or time mask.
        """
        velocity=np.asarray(velocity,dtype=float)
        if velocity.ndim!=2 or velocity.shape[1]!=self.modes or dt<=0 or coast_steps<0 or np.linalg.norm(velocity[0])>1e-14:raise ValueError('zero-start linear drive')
        integration=self.integrated_kernel(dt,quadrature) if integration is None else np.asarray(integration)
        if integration.ndim!=3 or integration.shape[1:]!=(self.modes,self.modes):raise ValueError('integrated kernel')
        count=len(velocity)+coast_steps;acceleration=np.diff(velocity,axis=0)/dt;result=np.zeros((count,self.modes))
        for mode in range(self.modes):
            convolved=fftconvolve(acceleration[:,mode,None],integration[:,:,mode],axes=0)
            result[1:1+min(count-1,len(convolved))]+=convolved[:count-1]
        knot=len(velocity)-1
        times=np.arange(1,min(coast_steps,int(np.ceil(self.max_delay/dt)))+1)*dt
        result[knot+1:knot+1+len(times)]-=np.einsum('tmn,n->tm',self.kernel(times),velocity[-1])
        return result
