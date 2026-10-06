"""Second-order lossless delay lift and fourth-order paired wave derivative.

The wave's polynomial/correlation and positive FIR storage remain intact.
Only its continuous allpass section is replaced. No eigenvalue/gain repair.
"""
import numpy as np
from concert_grand_bounded_air import BoundedAir,RelaxingMass
from concert_grand_streaming_air import SurfaceFactorBlocks


def fourth_factor(H):
    """[1/2,0,-1/2] * [-1/6,4/3,-1/6] gives the fourth-order derivative.

    Opposite channels are constructed by exact reversal, including roundoff.
    The new filter has unit DC gain; the derivative retains both endpoint zeros.
    """
    H=np.asarray(H,dtype=float)
    if H.ndim!=3 or H.shape[1]%2 or not np.array_equal(H[:,1::2],-H[::-1,::2]):
        raise ValueError('exact paired factor required')
    out=np.zeros((len(H)+2,)+H.shape[1:]);part=H[:,::2]
    out[:-2,::2]-=part/6;out[1:-1,::2]+=4*part/3;out[2:,::2]-=part/6
    out[:,1::2]=-out[::-1,::2]
    return out


class FourthFactorBlocks:
    def __init__(self,provider):
        if not provider.smooth:raise ValueError('fourth derivative requires the adjacent-averaged base factor')
        self.provider=provider;self.taps=provider.taps+2
    def __getattr__(self,name):return getattr(self.provider,name)
    def blocks(self):
        for H in self.provider.blocks():yield fourth_factor(H)
    def budget(self,frequency_block=16,row_block=16,real_gram=False):
        budget=SurfaceFactorBlocks.budget(self,frequency_block,row_block,real_gram)
        # Filtering also holds one base factor block until it is yielded.
        budget['estimated_array_peak_bytes']+=self.provider.taps*min(self.outputs,2*self.block)*self.modes*8
        return budget


def pade_delay(s,rate):
    """[2/2] approximation of exp(-s/rate), stable and Schur in Re(s)>0."""
    s=complex(s)
    if not np.isfinite(s) or not np.isfinite(rate) or rate<=0:raise ValueError('finite Laplace point and positive rate')
    p=s/rate
    if abs(p)>1:
        v=1/p;denominator=1+6*v+12*v*v;numerator=1-6*v+12*v*v
    else:denominator=p*p+6*p+12;numerator=p*p-6*p+12
    if denominator==0:raise ValueError('delay pole')
    return numerator/denominator


class PadeAir:
    """Cascade x'=r[[0,1],[-12,-6]]x+r[0,1]u; y=u-12*x2.

    Section P=diag(144,12)/r obeys d(x.T P x/2)=(u^2-y^2)/2.
    Lifting the FIR future-tail Gram with this P gives positive wave storage.
    Midpoint preserves its quadratic work identity at every positive dt.
    """
    def __init__(self,wave):
        if np.any(wave.tail_mass):raise ValueError('no inertial tail in Padé lift')
        self.base=wave;self.rate=wave.rate;self.order=wave.order;self.modes=wave.modes
        self.R=wave.R;self.D=wave.D;self.states=2*wave.states
        self.outputs=wave.outputs
    def delay(self,s):return pade_delay(s,self.rate)
    def impedance(self,s):
        t=self.delay(s)
        return self.D+np.einsum('k,kmn->mn',t**np.arange(1,self.order+1),self.R[1:])
    def static_mass(self):return self.base.static_mass()
    def high_limit(self):return self.D+self.R[1:].sum(axis=0)
    def zero_state(self):return np.zeros((self.order,2,self.modes))
    def validate(self,state,velocity=None):
        state=np.asarray(state,dtype=float)
        if state.shape!=(self.order,2,self.modes) or not np.isfinite(state).all():raise ValueError('finite Padé state')
        if velocity is None:return state
        velocity=np.asarray(velocity,dtype=float)
        if velocity.shape!=(self.modes,) or not np.isfinite(velocity).all():raise ValueError('finite velocity')
        return state,velocity
    def energy(self,state):
        state=self.validate(state)
        return 144*self.base.energy(state[:,0])+12*self.base.energy(state[:,1])
    def radiation_batch(self,inputs):return self.base.radiation_batch(inputs)
    def derivative(self,state,velocity):
        state,u=self.validate(state,velocity);u=u.copy();force=self.D@u
        updated=np.empty_like(state)
        for j,row in enumerate(state):
            updated[j,0]=self.rate*row[1]
            updated[j,1]=self.rate*(-12*row[0]-6*row[1]+u)
            u=u-12*row[1];force+=self.R[j+1]@u
        return updated,force
    def midpoint_deferred(self,state,velocity,dt):
        state,u=self.validate(state,velocity)
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive finite time step')
        h=dt*self.rate/2;denominator=1+6*h+12*h*h
        updated=np.empty_like(state);inputs=np.empty((self.order+1,self.modes));inputs[0]=u
        force=self.D@u
        for j,row in enumerate(state):
            rhs=row[1]+h*inputs[j]
            x1=((1+6*h)*row[0]+h*rhs)/denominator
            x2=(-12*h*row[0]+rhs)/denominator
            updated[j,0]=2*x1-row[0];updated[j,1]=2*x2-row[1]
            inputs[j+1]=inputs[j]-12*x2;force+=self.R[j+1]@inputs[j+1]
        return updated,force,inputs
    def midpoint(self,state,velocity,dt):
        updated,force,inputs=self.midpoint_deferred(state,velocity,dt)
        return updated,force,float(self.radiation_batch(inputs[None])[0])
    def centered_constant(self,state,dt):return self.midpoint_deferred(state,np.zeros(self.modes),dt)[1]


class PadeBoundedAir(BoundedAir):
    def __init__(self,wave,target_mass,target_limit):
        self.wave=wave;self.wave_limit=wave.high_limit()
        self.relaxation=RelaxingMass(np.asarray(target_mass)-wave.static_mass(),np.asarray(target_limit)-self.wave_limit)
        self.modes=wave.modes;self.states=wave.states+self.relaxation.states
    def derivative(self,state,velocity):
        derivative,force=self.wave.derivative(state[0],velocity)
        difference=self.relaxation.B.T@velocity-state[1]
        return (derivative,difference/self.relaxation.tau),force+self.relaxation.B@difference
    def centered_constant(self,state,dt):
        return self.wave.centered_constant(state[0],dt)-self.relaxation.B@(state[1]/(1+dt/(2*self.relaxation.tau)))
    def linear_drive_force(self,*args,**kwargs):raise NotImplementedError('Padé transient reference must be independently validated')
