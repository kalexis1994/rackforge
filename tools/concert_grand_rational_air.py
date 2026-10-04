"""Passive rational radiation load from paired surface wave factors.

A finite causal far-field FIR H defines Z_d = R_0/2 + sum R_k z^-k,
R_k = sum H_l.T H_{l+k}. Re_H Z_d = H.H H/2. Opposite angular
directions have reversed impulse responses, making every R_k symmetric.
The Cayley map z^-1=(a-s)/(a+s), a=2*factor_rate, preserves positive
realness. Repeated stable poles are a delay approximation, not fitted modes
or identified acoustic damping. Allpass states retain living acoustic memory.
This is an offline research construction, not a factory implementation.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.fft import rfft,irfft,next_fast_len
from scipy.signal import fftconvolve,lfilter
from scipy.sparse import coo_matrix
from concert_grand_acoustic_bath import positive_factor


def surface_factor(surface,rate,polar=12,azimuth=24):
    """Integrate each rectangular cell's projected delay distribution exactly.

    Cell-constant mass-normalized shapes, one face in a rigid baffle.
    Histogram taps retain area; the backward derivative retains zero DC.
    Binning and the bilinear frequency warp remain approximation errors.
    Opposite directions are constructed by reversal, not independent rounding.
    """
    if rate<=0 or polar<2 or azimuth<4 or azimuth%2:raise ValueError('wave grid')
    g,w=leggauss(polar);u=(g+1)/2;w=w/2
    az=2*np.pi*(np.arange(azimuth//2)+.5)/azimuth
    radial=np.sqrt(1-u*u)
    directions=np.column_stack((np.repeat(radial,len(az))*np.tile(np.cos(az),polar),
                                np.repeat(radial,len(az))*np.tile(np.sin(az),polar)))
    weights=np.repeat(w,len(az))*2*np.pi/azimuth
    extent=(abs(directions[:,0])*surface.length+abs(directions[:,1])*surface.width)/surface.speed
    order=int(np.ceil(extent.max()*rate))+2
    center=order/(2*rate);edges=(np.arange(order+2)-.5)/rate
    x=(np.arange(surface.nx)+.5)*surface.hx-surface.length/2
    y=(np.arange(surface.ny)+.5)*surface.hy-surface.width/2
    xx,yy=np.meshgrid(x,y,indexing='ij');positions=np.column_stack((xx.ravel(),yy.ravel()))
    shapes=surface.shapes.reshape(-1,surface.modes)
    H=np.zeros((order+2,2*len(directions),surface.modes))
    for i,(direction,weight) in enumerate(zip(directions,weights)):
        a=abs(direction[0])*surface.hx/surface.speed
        b=abs(direction[1])*surface.hy/surface.speed
        lower=center+positions@direction/surface.speed-(a+b)/2
        z=edges[:,None]-lower[None,:]
        if min(a,b)<1e-14:
            cdf=np.clip(z/max(a,b),0.,1.)
        else:
            cdf=(np.maximum(z,0)**2-np.maximum(z-a,0)**2-np.maximum(z-b,0)**2+np.maximum(z-a-b,0)**2)/(2*a*b)
            cdf=np.where(z<=0,0.,np.where(z>=a+b,1.,cdf))
        area=(np.diff(cdf,axis=0)@shapes)*(surface.hx*surface.hy)
        scale=rate*np.sqrt(surface.density*weight/(2*np.pi**2*surface.speed))
        H[:,2*i]=np.diff(np.vstack((np.zeros((1,surface.modes)),area,np.zeros((1,surface.modes)))),axis=0)*scale
        H[:,2*i+1]=-H[::-1,2*i]
    return H


def surface_factor_sparse(surface,rate,polar=12,azimuth=24):
    """Same cell-delay integral with only intersected histogram bins stored.

    A projected cell covers few bins. Its uniform-sum CDF is evaluated in
    triangular/linear pieces to avoid subtracting large squared distances.
    The opposite direction, area normalization and backward derivative are
    identical to surface_factor. This changes cost, not the discretization.
    """
    if rate<=0 or polar<2 or azimuth<4 or azimuth%2:raise ValueError('wave grid')
    g,w=leggauss(polar);u=(g+1)/2;w=w/2
    az=2*np.pi*(np.arange(azimuth//2)+.5)/azimuth;radial=np.sqrt(1-u*u)
    directions=np.column_stack((np.repeat(radial,len(az))*np.tile(np.cos(az),polar),np.repeat(radial,len(az))*np.tile(np.sin(az),polar)))
    weights=np.repeat(w,len(az))*2*np.pi/azimuth
    extent=(abs(directions[:,0])*surface.length+abs(directions[:,1])*surface.width)/surface.speed
    order=int(np.ceil(extent.max()*rate))+2;center=order/(2*rate)
    xx,yy=np.meshgrid((np.arange(surface.nx)+.5)*surface.hx-surface.length/2,(np.arange(surface.ny)+.5)*surface.hy-surface.width/2,indexing='ij')
    positions=np.column_stack((xx.ravel(),yy.ravel()));shapes=surface.shapes.reshape(-1,surface.modes)
    H=np.zeros((order+2,2*len(directions),surface.modes));cells=np.arange(len(positions))
    for i,(direction,weight) in enumerate(zip(directions,weights)):
        a,b=sorted((abs(direction[0])*surface.hx/surface.speed,abs(direction[1])*surface.hy/surface.speed))
        lower=center+positions@direction/surface.speed-(a+b)/2
        first=np.floor(lower*rate+.5).astype(int)
        bins=first[None,:]+np.arange(int(np.ceil((a+b)*rate))+2)[:,None]
        def cdf(z):
            if a<1e-14:return np.clip(z/b,0.,1.)
            z=np.clip(z,0.,a+b)
            return np.where(z<a,z*z/(2*a*b),np.where(z<=b,(z-a/2)/b,1-(a+b-z)**2/(2*a*b)))
        left=(bins-.5)/rate-lower[None,:]
        probability=cdf(left+1/rate)-cdf(left)
        active=(bins>=0)&(bins<order+1)&(probability!=0)
        rows=bins[active];columns=np.broadcast_to(cells,bins.shape)[active]
        histogram=coo_matrix((probability[active],(rows,columns)),shape=(order+1,len(cells))).tocsr()
        area=(histogram@shapes)*(surface.hx*surface.hy)
        scale=rate*np.sqrt(surface.density*weight/(2*np.pi**2*surface.speed))
        H[:,2*i]=np.diff(np.vstack((np.zeros((1,surface.modes)),area,np.zeros((1,surface.modes)))),axis=0)*scale
        H[:,2*i+1]=-H[::-1,2*i]
    return H


class RationalAir:
    def __init__(self,factor,rate,tail_mass=None):
        self.H=np.asarray(factor,dtype=float)
        if self.H.ndim!=3 or min(self.H.shape)<1 or len(self.H)<2 or not np.isfinite(self.H).all() or not np.isfinite(rate) or rate<=0:raise ValueError('finite wave factor and rate')
        self.rate=float(rate);self.a=2*self.rate;self.order=len(self.H)-1
        self.outputs,self.modes=self.H.shape[1:];self.states=self.order*self.modes
        self.tail_mass=np.zeros((self.modes,self.modes)) if tail_mass is None else np.asarray(tail_mass,dtype=float)
        if self.tail_mass.shape!=(self.modes,self.modes):raise ValueError('tail mass dimensions')
        positive_factor(self.tail_mass)
        size=next_fast_len(2*len(self.H)-1)
        spectrum=rfft(self.H,n=size,axis=0)
        gram=np.swapaxes(spectrum.conj(),1,2)@spectrum
        self.R=irfft(gram,n=size,axis=0)[:len(self.H)]
        scale=max(np.linalg.norm(self.R),1e-30)
        self.reciprocity_error=float(np.linalg.norm(self.R-self.R.swapaxes(1,2))/scale)
        if self.reciprocity_error>1e-11:raise ValueError('paired reciprocal wave factors required')
        # Only arithmetic roundoff is symmetrized, never negative eigenvalues.
        self.R=(self.R+self.R.swapaxes(1,2))/2
        self.D=self.R[0]/2
        self.dc_error=float(np.linalg.norm(self.D+self.R[1:].sum(axis=0))/scale)
        if self.dc_error>1e-11:raise ValueError('wave factor must vanish at DC')

    def static_mass(self):
        return self.tail_mass-np.einsum('k,kmn->mn',np.arange(1,self.order+1)/self.rate,self.R[1:])

    def with_static_tail(self,target):
        """Complete missing DC mass only when its residual is already PSD.

        No eigenvalue repair and no change to the wave factor/resistance.
        This is an inertial high-frequency approximation, assessed separately.
        """
        if np.any(self.tail_mass):raise ValueError('tail already present')
        return RationalAir(self.H,self.rate,np.asarray(target)-self.static_mass())

    def mass_congruent(self,target):
        """One global port congruence matches DC mass, with no frequency fit.

        Both masses must be strictly positive; no clipping or damping repair.
        This changes radiation transfer and must pass held-out verification.
        """
        target=np.asarray(target,dtype=float)
        if np.any(self.tail_mass):raise ValueError('mass congruence expects wave-only load')
        if target.shape!=(self.modes,self.modes) or not np.isfinite(target).all() or not np.allclose(target,target.T,rtol=0,atol=1e-12):raise ValueError('symmetric target mass')
        def root(matrix,inverse=False):
            val,vec=np.linalg.eigh(matrix)
            if val.min()<=1e-13*val.max():raise ValueError('strictly positive mass required')
            return (vec*(1/np.sqrt(val) if inverse else np.sqrt(val)))@vec.T
        mass=self.static_mass();sqrt=root(mass);inv=root(mass,True)
        transform=inv@root(sqrt@target@sqrt)@inv
        return RationalAir(self.H@transform,self.rate),transform

    def impedance(self,s):
        s=complex(s)
        if not np.isfinite(s) or s==-self.a:raise ValueError('finite nonsingular Laplace point')
        t=(self.a-s)/(self.a+s)
        return s*self.tail_mass+self.D+np.einsum('k,kmn->mn',t**np.arange(1,self.order+1),self.R[1:])

    def zero_state(self):return np.zeros((self.order,self.modes))

    def midpoint(self,state,velocity,dt):
        """Return cascade, centered wave load force and radiated loss power.

        Continuous states r_j'=a(u_{j-1}-r_j), u_j=2r_j-u_{j-1}.
        The loss is ||sum H_j u_j||^2/2, not numerical absorption. Any tail
        belongs in the caller's mechanical mass; it is not in this force.
        """
        state=np.asarray(state,dtype=float);velocity=np.asarray(velocity,dtype=float)
        if state.shape!=(self.order,self.modes) or velocity.shape!=(self.modes,) or not np.isfinite(dt) or dt<=0:raise ValueError('state/input/time step')
        centered=np.empty_like(state);u=velocity.copy();force=self.D@u;wave=self.H[0]@u
        factor=dt*self.a/2
        for j in range(self.order):
            centered[j]=(state[j]+factor*u)/(1+factor)
            u=2*centered[j]-u
            force+=self.R[j+1]@u;wave+=self.H[j+1]@u
        return 2*centered-state,force,float(wave@wave/2)

    def midpoint_deferred(self,state,velocity,dt):
        """Identical dynamics; retain centered wave inputs for a batched observer.

        Radiation is an output, not a feedback term. The caller must evaluate
        all retained inputs with radiation_batch before closing a loss ledger.
        """
        state=np.asarray(state,dtype=float);velocity=np.asarray(velocity,dtype=float)
        if state.shape!=(self.order,self.modes) or velocity.shape!=(self.modes,) or not np.isfinite(dt) or dt<=0:raise ValueError('state/input/time step')
        centered=np.empty_like(state);inputs=np.empty((self.order+1,self.modes))
        inputs[0]=velocity;force=self.D@velocity;factor=dt*self.a/2
        for j in range(self.order):
            centered[j]=(state[j]+factor*inputs[j])/(1+factor)
            inputs[j+1]=2*centered[j]-inputs[j]
            force+=self.R[j+1]@inputs[j+1]
        return 2*centered-state,force,inputs

    def radiation_batch(self,inputs):
        """All angular channels, same FIR factor; only summation order changes."""
        inputs=np.asarray(inputs,dtype=float)
        if inputs.ndim!=3 or inputs.shape[1:]!=(self.order+1,self.modes):raise ValueError('batch wave inputs')
        if not hasattr(self,'observer_matrix'):
            self.observer_matrix=np.ascontiguousarray(self.H.transpose(1,0,2)).reshape(self.outputs,-1)
        field=self.observer_matrix@inputs.reshape(len(inputs),-1).T
        return np.sum(field*field,axis=0)/2

    def energy(self,state,velocity=None):
        """Positive storage: energy of all future FIR wave tails.

        The same quadratic certificate applies to the continuous allpass
        realization and to midpoint at every dt, not just 1/factor_rate.
        FFT convolution is only an evaluation of this finite quadratic form.
        """
        state=np.asarray(state,dtype=float)
        if state.shape!=(self.order,self.modes):raise ValueError('cascade state')
        if np.any(self.tail_mass) and velocity is None:raise ValueError('velocity required for inertial tail energy')
        tails=np.zeros((self.order,self.outputs))
        for mode in range(self.modes):
            convolution=fftconvolve(self.H[:,:,mode],state[::-1,mode,None],axes=0)
            tails+=convolution[self.order:self.order*2]
        tail=0. if velocity is None else np.asarray(velocity)@self.tail_mass@np.asarray(velocity)/2
        return float(np.sum(tails*tails)/(2*self.rate)+tail)

    def sampled_force(self,velocity,dt,acceleration=None):
        """Zero-state trapezoidal transfer at sample times, for convergence.

        Inputs are sample values, first sample must be zero. This includes
        the rational direct term. A tail needs independently supplied physical
        acceleration; impulses at input jumps must be accounted separately.
        scipy's scalar allpass filters independently implement each section.
        """
        velocity=np.asarray(velocity,dtype=float)
        if velocity.ndim!=2 or velocity.shape[1]!=self.modes or dt<=0 or np.linalg.norm(velocity[0])>1e-14:raise ValueError('zero-start drive')
        if np.any(self.tail_mass) and acceleration is None:raise ValueError('acceleration required for inertial tail force')
        if acceleration is not None and np.shape(acceleration)!=velocity.shape:raise ValueError('acceleration dimensions')
        tail=0. if acceleration is None else np.asarray(acceleration)@self.tail_mass.T
        t=(dt*self.a/2-1)/(dt*self.a/2+1)
        if t==0:
            kernel=self.R.copy();kernel[0]/=2
            result=np.zeros_like(velocity)
            for mode in range(self.modes):
                result+=fftconvolve(velocity[:,mode,None],kernel[:,:,mode],axes=0)[:len(velocity)]
            return result+tail
        u=velocity.copy();result=u@self.D.T
        for residue in self.R[1:]:
            u=lfilter([t,1.],[1.,t],u,axis=0)
            result+=u@residue.T
        return result+tail

    def step_kernel(self,time):
        """Wave Z(s)/s in continuous time, via scaled Laguerre polynomials.

        Exact rational step response; no impulse rasterization or FFT period.
        Scaling prevents exp(-a*t) underflow before polynomial amplification.
        """
        t=np.asarray(time,dtype=float).reshape(-1)
        if not np.isfinite(t).all() or (t<0).any():raise ValueError('nonnegative kernel times')
        x=2*self.a*t;previous=np.ones(len(t));current=1-x;logscale=np.zeros(len(t))
        weighted=np.empty((len(t),self.order));weighted[:,0]=np.exp(-x/2)
        for j in range(1,self.order):
            large=np.maximum(abs(previous),abs(current))>1e100
            previous[large]/=1e100;current[large]/=1e100;logscale[large]+=np.log(1e100)
            logmagnitude=np.full(len(t),-np.inf)
            np.log(abs(current),out=logmagnitude,where=current!=0)
            weighted[:,j]=(-1.)**j*np.sign(current)*np.exp(logscale-x/2+logmagnitude)
            previous,current=current,((2*j+1-x)*current-j*previous)/(j+1)
        suffix=np.cumsum(self.R[:0:-1],axis=0)[::-1]
        return -2*np.einsum('tj,jmn->tmn',weighted,suffix,optimize=True)

    def linear_drive_force(self,velocity,dt,coast_steps,quadrature=8):
        """Continuous rational force for linear segments and an exact stop.

        Driven sample knots plus a stationary port after the last knot.
        Gauss integration of the analytic step kernel handles every segment;
        -K(t-stop)*v_stop handles the jump, without smearing it over a step.
        Regular inertial force at knots uses the LEFT segment derivative,
        matching the bath's exact linear drive. Force at stop is the left
        limit. Both inertial delta impulses
        are external to this sampled array. Refine quadrature independently.
        """
        velocity=np.asarray(velocity,dtype=float)
        if velocity.ndim!=2 or velocity.shape[1]!=self.modes or dt<=0 or coast_steps<0 or quadrature<2 or np.linalg.norm(velocity[0])>1e-14:raise ValueError('linear drive')
        count=len(velocity)+coast_steps;nodes,weights=leggauss(quadrature)
        integration=np.empty((count-1,self.modes,self.modes))
        for start in range(0,count-1,256):
            stop=min(start+256,count-1)
            times=(np.arange(start,stop)[:,None]+(nodes[None,:]+1)/2)*dt
            kernels=self.step_kernel(times.ravel()).reshape(stop-start,quadrature,self.modes,self.modes)
            integration[start:stop]=np.einsum('q,tqmn->tmn',weights*dt/2,kernels)
        acceleration=np.diff(velocity,axis=0)/dt
        result=np.zeros((count,self.modes))
        for mode in range(self.modes):
            result[1:]+=fftconvolve(acceleration[:,mode,None],integration[:,:,mode],axes=0)[:count-1]
        knot=len(velocity)-1
        result[knot+1:]-=np.einsum('tmn,n->tm',self.step_kernel(np.arange(1,coast_steps+1)*dt),velocity[-1])
        result[1:knot+1]+=acceleration@self.tail_mass.T
        return result
