"""Passive finite-limit completion of a paired-wave radiation approximation.

Average adjacent FIR taps to remove the factor's Nyquist endpoint. The Cayley
load is still reciprocal and positive real, now with zero high-frequency
wave resistance. A strictly positive matrix relaxation supplies the missing
DC mass and physical high-frequency resistance, with explicit storage/loss.
No negative residual repair, sample fitting or coupled runtime claim.
"""
import numpy as np
from scipy.signal import fftconvolve
from concert_grand_rational_air import RationalAir


def positive_root(matrix):
    matrix=np.asarray(matrix,dtype=float)
    if matrix.ndim!=2 or matrix.shape[0]!=matrix.shape[1] or not np.isfinite(matrix).all():raise ValueError('finite square matrix')
    if not np.allclose(matrix,matrix.T,rtol=0,atol=1e-12*max(np.linalg.norm(matrix),1.)):raise ValueError('symmetric matrix required')
    val,vec=np.linalg.eigh(matrix)
    if val[0]<=1e-13*val[-1]:raise ValueError('strictly positive residual required; no clipping')
    return (vec*np.sqrt(val))@vec.T,(vec/np.sqrt(val))@vec.T


class RelaxingMass:
    """Z(s)=B diag(s*tau/(1+s*tau)) B.T, M=B diag(tau) B.T.

    q'=diag(1/tau)(B.T*v-q), force=B(B.T*v-q).
    E=sum(tau*q*q)/2; loss=||B.T*v-q||^2. Unlike s*M, its
    step kernel has no delta: B diag(exp(-t/tau)) B.T.
    """
    def __init__(self,mass,resistance):
        self.mass=np.asarray(mass,dtype=float);self.resistance=np.asarray(resistance,dtype=float)
        if self.mass.shape!=self.resistance.shape:raise ValueError('residual dimensions')
        root,inv=positive_root(self.resistance);positive_root(self.mass)
        self.tau,vec=np.linalg.eigh(inv@self.mass@inv)
        if self.tau[0]<=0:raise ValueError('positive relaxation times required')
        self.B=root@vec;self.modes=len(self.tau);self.states=self.modes

    def impedance(self,s):
        s=complex(s)
        if not np.isfinite(s) or np.any(1+s*self.tau==0):raise ValueError('finite nonsingular Laplace point')
        return (self.B*(s*self.tau/(1+s*self.tau)))@self.B.T

    def kernel(self,time):
        time=np.asarray(time,dtype=float).reshape(-1)
        if not np.isfinite(time).all() or (time<0).any():raise ValueError('nonnegative times')
        return np.einsum('mj,tj,nj->tmn',self.B,np.exp(-time[:,None]/self.tau),self.B)

    def midpoint(self,state,velocity,dt):
        state=np.asarray(state,dtype=float);velocity=np.asarray(velocity,dtype=float)
        if state.shape!=(self.modes,) or velocity.shape!=(self.modes,) or dt<=0 or not np.isfinite(dt):raise ValueError('state/input/time step')
        u=self.B.T@velocity;alpha=dt/(2*self.tau)
        centered=(state+alpha*u)/(1+alpha);difference=u-centered
        return 2*centered-state,self.B@difference,float(difference@difference)

    def energy(self,state):return float(self.tau@np.asarray(state)**2/2)

    def linear_drive_force(self,velocity,dt,coast_steps):
        velocity=np.asarray(velocity,dtype=float)
        if velocity.ndim!=2 or velocity.shape[1]!=self.modes or len(velocity)<2 or dt<=0 or coast_steps<0 or np.linalg.norm(velocity[0])>1e-14:raise ValueError('zero-start linear drive')
        count=len(velocity)+coast_steps
        start=np.arange(count-1)*dt
        integral=np.exp(-start[:,None]/self.tau)*(self.tau*(-np.expm1(-dt/self.tau)))
        u=velocity@self.B;acceleration=np.diff(u,axis=0)/dt
        result=np.zeros((count,self.modes))
        for j in range(self.modes):result[1:,j]=fftconvolve(acceleration[:,j],integral[:,j])[:count-1]
        knot=len(velocity)-1
        result[knot+1:]-=np.exp(-np.arange(1,coast_steps+1)[:,None]*dt/self.tau)*u[-1]
        return result@self.B.T


class BoundedAir:
    def __init__(self,factor,rate,target_mass,target_limit,smooth=True):
        H=np.asarray(factor,dtype=float)
        if smooth:
            filtered=np.zeros((len(H)+1,)+H.shape[1:]);filtered[:-1]+=H/2;filtered[1:]+=H/2;H=filtered
        self.wave=RationalAir(H,rate)
        self.wave_limit=self.wave.D+np.einsum('k,kmn->mn',(-1.)**np.arange(1,self.wave.order+1),self.wave.R[1:])
        self.relaxation=RelaxingMass(np.asarray(target_mass)-self.wave.static_mass(),np.asarray(target_limit)-self.wave_limit)
        self.modes=self.wave.modes;self.states=self.wave.states+self.relaxation.states

    def impedance(self,s):return self.wave.impedance(s)+self.relaxation.impedance(s)
    def static_mass(self):return self.wave.static_mass()+(self.relaxation.B*self.relaxation.tau)@self.relaxation.B.T
    def high_limit(self):return self.wave_limit+self.relaxation.B@self.relaxation.B.T
    def zero_state(self):return self.wave.zero_state(),np.zeros(self.modes)
    def energy(self,state):
        # Exact zero-state shortcut only, never a small-energy threshold.
        wave=self.wave.energy(state[0]) if np.any(state[0]) else 0.
        return wave+self.relaxation.energy(state[1])

    def derivative(self,state,velocity):
        """Continuous state derivative and force, with no inertial delta."""
        u=np.asarray(velocity,dtype=float).copy();force=self.wave.D@u;rows=[]
        for row,residue in zip(state[0],self.wave.R[1:]):
            rows.append(self.wave.a*(u-row));u=2*row-u;force+=residue@u
        difference=self.relaxation.B.T@velocity-state[1]
        force+=self.relaxation.B@difference
        return (np.array(rows),difference/self.relaxation.tau),force

    def midpoint(self,state,velocity,dt):
        wave,f1,p1=self.wave.midpoint(state[0],velocity,dt)
        relax,f2,p2=self.relaxation.midpoint(state[1],velocity,dt)
        return (wave,relax),f1+f2,p1+p2

    def midpoint_deferred(self,state,velocity,dt):
        wave,f1,inputs=self.wave.midpoint_deferred(state[0],velocity,dt)
        relax,f2,p2=self.relaxation.midpoint(state[1],velocity,dt)
        return (wave,relax),f1+f2,p2,inputs

    def centered_constant(self,state,dt):
        """Centered force at zero input, without calculating radiation power."""
        alpha=dt*self.wave.a/2;beta=(alpha-1)/(alpha+1)
        u=np.zeros(self.modes);force=np.zeros(self.modes)
        for row,residue in zip(state[0],self.wave.R[1:]):
            u=beta*u+2*row/(1+alpha);force+=residue@u
        alpha=dt/(2*self.relaxation.tau)
        return force-self.relaxation.B@(state[1]/(1+alpha))

    def linear_drive_force(self,velocity,dt,coast_steps,quadrature=8):
        return self.wave.linear_drive_force(velocity,dt,coast_steps,quadrature)+self.relaxation.linear_drive_force(velocity,dt,coast_steps)


class LoadedBoundedBoard:
    """Unit modal mechanical masses, persistent air states, implicit midpoint.

    Eliminate the air at the centered velocity with Z(2/dt). Its constant
    force comes from live states. Work closes with dry and acoustic storage
    and separate nonnegative material/radiation loss at any positive step.
    This small Schur solve is an offline benchmark, not a runtime engine.
    """
    def __init__(self,stiffness,damping,air):
        self.K=np.asarray(stiffness,dtype=float);self.D=np.asarray(damping,dtype=float);self.air=air
        if self.K.shape!=(air.modes,air.modes) or self.D.shape!=self.K.shape:raise ValueError('mechanical matrices')
        positive_root(self.K)
        if not np.isfinite(self.D).all() or not np.allclose(self.D,self.D.T,rtol=0,atol=1e-12) or np.linalg.eigvalsh(self.D)[0]<0:raise ValueError('passive symmetric damping')
        self.cache=None

    def zero_state(self):return np.zeros(self.air.modes),np.zeros(self.air.modes),self.air.zero_state()
    def energy(self,state):
        q,v,a=state
        return float((v@v+q@self.K@q)/2+self.air.energy(a))

    def step(self,state,dt,force=None):
        from scipy.linalg import cho_factor,cho_solve
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive step')
        q,v,a=state;force=np.zeros(self.air.modes) if force is None else np.asarray(force,dtype=float)
        if force.shape!=(self.air.modes,):raise ValueError('force dimensions')
        if self.cache is None or self.cache[0]!=dt:
            matrix=2/dt*np.eye(self.air.modes)+self.D+dt/2*self.K+self.air.impedance(2/dt).real
            self.cache=(dt,cho_factor(matrix))
        constant=self.air.centered_constant(a,dt)
        center=cho_solve(self.cache[1],force-self.K@q+2/dt*v-constant)
        new_air,load,radiated=self.air.midpoint(a,center,dt)
        updated=(q+dt*center,2*center-v,new_air)
        return updated,dict(input_work_j=dt*float(center@force),air_port_work_j=dt*float(center@load),
            material_loss_j=dt*float(center@self.D@center),radiation_loss_j=dt*radiated)
