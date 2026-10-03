"""SI Rayleigh pressure of a flat baffled surface, exp(+i omega t).

The input is intrinsic mass-normalized board acceleration, not native drive
units. This is a one-way acoustic observer: no acoustic loading, room, lid,
microphone pattern or EQ. An infinite baffle is an explicit approximation.
"""
from dataclasses import dataclass
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.signal import fftconvolve
import concert_grand_orthotropic_plate as plate


@dataclass
class Surface:
    points:np.ndarray       # points in metres, (quadrature,3)
    area:np.ndarray         # dS in m^2
    shapes:np.ndarray       # kg^-1/2, intrinsic modal displacement shapes

    def __post_init__(self):
        self.points=np.asarray(self.points,dtype=float);self.area=np.asarray(self.area,dtype=float)
        self.shapes=np.asarray(self.shapes,dtype=float)
        n=len(self.area)
        if self.points.shape!=(n,3) or self.shapes.ndim!=2 or self.shapes.shape[0]!=n or self.shapes.shape[1]==0:raise ValueError('surface dimensions')
        if not np.isfinite(self.points).all() or not np.isfinite(self.area).all() or not np.isfinite(self.shapes).all():raise ValueError('nonfinite surface')
        if n==0 or (self.area<=0).any():raise ValueError('positive area required')
        if not np.allclose(self.points[:,2],0.,atol=1e-14):raise ValueError('surface must lie in z=0 plane')

    @classmethod
    def from_plate(cls,model,nx=64,ny=48):
        gx,wx=leggauss(nx);gy,wy=leggauss(ny)
        length=model['panel']['length'];width=model['panel']['width']
        x,y=np.meshgrid((gx+1)*length/2,(gy+1)*width/2,indexing='ij')
        area=np.outer(wx*length/2,wy*width/2).ravel()
        points=np.c_[x.ravel(),y.ravel(),np.zeros(x.size)]
        return cls(points,area,plate.shape(model,x.ravel(),y.ravel()))


class RayleighObserver:
    def __init__(self,surface,microphones,density=1.2,speed=343.):
        self.surface=surface;self.microphones=np.asarray(microphones,dtype=float)
        if self.microphones.ndim!=2 or self.microphones.shape[1]!=3 or len(self.microphones)==0 or not np.isfinite(self.microphones).all():raise ValueError('microphone dimensions')
        if (self.microphones[:,2]<=0).any() or not np.isfinite([density,speed]).all() or min(density,speed)<=0:raise ValueError('air/half-space geometry')
        self.density=density;self.speed=speed
        self.distance=np.linalg.norm(self.microphones[:,None,:]-surface.points[None,:,:],axis=2)
        self.delay=self.distance/speed
        self.weight=density/(2*np.pi)*surface.area[None,:,None]*surface.shapes[None,:,:]/self.distance[:,:,None]

    def acceleration_transfer(self,frequency):
        """Pa per (sqrt(kg)*m/s^2); shape (frequency,channel,mode)."""
        f=np.atleast_1d(np.asarray(frequency,dtype=float))
        if not np.isfinite(f).all():raise ValueError('finite frequencies required')
        return np.array([np.einsum('cn,cnm->cm',np.exp(-2j*np.pi*hz*self.delay),self.weight) for hz in f])

    def displacement_transfer(self,frequency):
        f=np.atleast_1d(np.asarray(frequency,dtype=float))
        return -(2*np.pi*f[:,None,None])**2*self.acceleration_transfer(f)

    def radiation_resistance(self,frequency,polar=16,azimuth=32):
        """Modal dissipative acoustic load via positive hemisphere integration.

        Mean harmonic radiated power is .5*V.conj()@R@V. R acts on intrinsic
        modal velocity in sqrt(kg)*m/s and has units s^-1. This diagnoses
        loading; it is not fed back into the mechanical pilot.
        """
        gu,wu=leggauss(polar);u=(gu+1)/2;weight_u=wu/2
        angle=2*np.pi*(np.arange(azimuth)+.5)/azimuth
        x,y=np.meshgrid(np.sqrt(1-u*u),angle,indexing='ij')
        directions=np.c_[(x*np.cos(y)).ravel(),(x*np.sin(y)).ravel()]
        weight=np.repeat(weight_u,azimuth)*2*np.pi/azimuth
        path=directions@self.surface.points[:,:2].T
        weighted_shape=self.surface.area[:,None]*self.surface.shapes
        rows=[]
        for hz in np.atleast_1d(frequency):
            omega=2*np.pi*hz
            far=np.exp(1j*omega/self.speed*path)@weighted_shape
            gram=far.conj().T@(weight[:,None]*far)
            resistance=self.density*omega**2/(4*np.pi**2*self.speed)*gram.real
            rows.append(.5*(resistance+resistance.T))
        return np.asarray(rows)

    def kernel(self,rate):
        """Causal two-tap fractional-delay approximation; valid after refinement.

        First tap is at ceil(delay*rate), so it cannot precede any geometric
        arrival. Linear extrapolation at that tap and the following tap retains
        DC and first delay moment. Signed taps are a numerical stencil, not
        extra physical sources. Accuracy is checked against the continuous
        frequency integral over the declared frequency band.
        """
        if not np.isfinite(rate) or rate<=0:raise ValueError('positive sample rate')
        d=self.delay*rate;first=np.ceil(d).astype(int);fraction=d-first
        out=np.zeros((first.max()+2,len(self.microphones),self.surface.shapes.shape[1]))
        for channel in range(len(self.microphones)):
            np.add.at(out[:,channel,:],first[channel],self.weight[channel]*(1-fraction[channel,:,None]))
            np.add.at(out[:,channel,:],first[channel]+1,self.weight[channel]*fraction[channel,:,None])
        return out

    def sampled_transfer(self,frequency,rate):
        impulse=self.kernel(rate);f=np.atleast_1d(np.asarray(frequency,dtype=float))
        phase=np.exp(-2j*np.pi*f[:,None]*np.arange(len(impulse))[None,:]/rate)
        return np.einsum('ft,tcm->fcm',phase,impulse)

    def pressure(self,acceleration,rate):
        acceleration=np.asarray(acceleration,dtype=float)
        if acceleration.ndim!=2 or acceleration.shape[1]!=self.surface.shapes.shape[1] or not np.isfinite(acceleration).all():raise ValueError('modal acceleration dimensions')
        impulse=self.kernel(rate);out=np.zeros((len(acceleration)+len(impulse)-1,len(self.microphones)))
        for channel in range(out.shape[1]):
            for mode in range(acceleration.shape[1]):
                out[:,channel]+=fftconvolve(acceleration[:,mode],impulse[:,channel,mode])
        # FFT round-off is not a physical precursor.
        for channel in range(out.shape[1]):out[:int(np.ceil(self.delay[channel].min()*rate)),channel]=0.
        return out
