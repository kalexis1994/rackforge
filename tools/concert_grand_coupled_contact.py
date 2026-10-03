"""Energy-accounted SI hammer + extensible string, before output calibration.

The hammer and every string coordinate share one compression potential.
Exact scalar divided differences supply felt work, while the quartic string
potential uses its existing AVF gradient. Optional rate damping is passive,
not the factory's hereditary felt law and not identified real felt.
"""
from dataclasses import dataclass
import numpy as np
from concert_grand_nonlinear_string import String


class LinearString(String):
    """Matched elastic reference with T/L nonlinear exchange disabled."""
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        self.linear=super().gradient_hessian(np.zeros(self.size))[1]

    def potential(self,q):return .5*float(q@self.linear@q)

    def gradient_hessian(self,q,hessian=True):
        gradient=self.linear@q
        return (gradient,self.linear) if hessian else gradient


@dataclass(frozen=True)
class Felt:
    hammer_mass:float
    stiffness:float
    exponent:float
    strike_fraction:float
    width_fraction:float
    rate_damping_s_per_m:float=0.

    def __post_init__(self):
        if (self.hammer_mass<=0 or self.stiffness<=0 or self.exponent<1
            or not 0<self.strike_fraction<1 or self.width_fraction<0
            or self.rate_damping_s_per_m<0):raise ValueError('invalid felt parameters')


def felt_divided_force(a,b,k,p):
    """Exact elastic potential secant and its endpoint derivative.

    Positive compression only; expm1 avoids cancelling nearly equal powers.
    The Jacobian uses the midpoint limit only for a tiny relative interval.
    """
    hi=max(a,b);lo=min(a,b)
    if hi<=0:return 0.,0.
    if hi==lo:return k*hi**p,.5*k*p*hi**(p-1)
    if lo>0:
        relative=(hi-lo)/hi
        difference=-np.expm1((p+1)*np.log1p(-relative))
        force=k*hi**p*difference/((p+1)*relative)
    else:
        force=k/(p+1)*hi**(p+1)/(hi-lo)
    delta=b-a
    if abs(delta)<1e-5*hi and lo>0:
        jac=.5*k*p*((a+b)/2)**(p-1)
    else:
        jac=(k*max(b,0.)**p-force)/delta
    return float(force),float(jac)


class HammerString:
    def __init__(self,string,felt):
        self.string=string;self.felt=felt;self.size=string.size+1
        self.direction=np.zeros(self.size)
        h=np.arange(1,string.nt+1)
        # Same distributed contact shape for force and felt displacement.
        # No comb floor from a fictitious flexible endpoint.
        shape=np.sin(np.pi*h*felt.strike_fraction)*np.exp(-1.2*(h*felt.width_fraction)**2)
        self.direction[string.sv]=-shape/np.sqrt(string.wire.modal_mass)
        self.direction[-1]=1/np.sqrt(felt.hammer_mass)

    def compression(self,q):return float(self.direction@q)

    def felt_potential(self,q):
        d=max(0.,self.compression(q));f=self.felt
        return f.stiffness/(f.exponent+1)*d**(f.exponent+1)

    def potential(self,q):return self.string.potential(q[:-1])+self.felt_potential(q)

    def energy(self,q,p):return .5*float(p@p)+self.potential(q)

    def initial_state(self,speed):
        q=np.zeros(self.size);p=np.zeros(self.size)
        p[-1]=np.sqrt(self.felt.hammer_mass)*speed
        return q,p

    def continuous_force(self,q,p):
        f=self.felt;delta=max(0.,self.compression(q));speed=float(self.direction@p)
        elastic=f.stiffness*delta**f.exponent
        factor=np.exp(np.clip(f.rate_damping_s_per_m*speed,-40,40))
        force=elastic*factor
        gradient=np.r_[self.string.gradient_hessian(q[:-1],False),0.]+self.direction*force
        heat=(force-elastic)*speed
        return gradient,float(heat)

    def avf(self,q0,q1,dt):
        gs,js=self.string.avf(q0[:-1],q1[:-1])
        gradient=np.r_[gs,0.];jac=np.zeros((self.size,self.size));jac[:-1,:-1]=js
        d0,d1=self.compression(q0),self.compression(q1)
        f=self.felt
        elastic,derivative=felt_divided_force(d0,d1,f.stiffness,f.exponent)
        relative_speed=(d1-d0)/dt
        argument=f.rate_damping_s_per_m*relative_speed
        factor=np.exp(np.clip(argument,-40,40))
        force=elastic*factor
        derivative=factor*(derivative+(elastic*f.rate_damping_s_per_m/dt if -40<argument<40 else 0.))
        gradient+=self.direction*force
        jac+=np.outer(self.direction,self.direction)*derivative
        lost=(force-elastic)*(d1-d0)
        return gradient,jac,dict(felt_loss_j=float(lost),force_n=float(force),
                                compression_start_m=d0,compression_end_m=d1)

    def step(self,q,p,dt,damping=0.):
        sigma=np.broadcast_to(damping,q.shape)
        if dt<=0 or (sigma<0).any():raise ValueError('invalid time step/damping')
        candidate=q+dt*p
        diagonal=np.eye(self.size)*(1+dt*sigma)
        scale=max(np.linalg.norm(candidate),np.linalg.norm(dt*p),1e-12)
        for iteration in range(20):
            g,jac,stats=self.avf(q,candidate,dt)
            residual=(1+dt*sigma)*(candidate-q)-dt*p+.5*dt*dt*g
            norm=np.linalg.norm(residual)
            if norm<2e-14*scale:break
            update=np.linalg.solve(diagonal+.5*dt*dt*jac,residual)
            for power in range(12):
                trial=candidate-update*2.**(-power)
                trial_g,_,_=self.avf(q,trial,dt)
                trial_residual=(1+dt*sigma)*(trial-q)-dt*p+.5*dt*dt*trial_g
                if np.linalg.norm(trial_residual)<norm:
                    candidate=trial;break
            else:raise RuntimeError('contact Newton line search failed')
        else:raise RuntimeError('contact Newton did not converge')
        next_p=((1-dt*sigma)*p-dt*g)/(1+dt*sigma)
        loss=stats['felt_loss_j']+dt*np.dot(2*sigma,((p+next_p)/2)**2)
        stats.update(loss_j=float(loss),newton_iterations=iteration+1)
        return candidate,next_p,stats
