"""SI Galerkin string + moving vertical endpoint + positive modal support.

v(x)=sum(q_n*sin(n*pi*x/L))+(x/L)*b. The endpoint lifting contributes both
mass and elastic energy. Cholesky normalization retains the mixed inertia.
Axial/horizontal endpoints remain fixed. Euler bending, not Timoshenko.
"""
from dataclasses import dataclass
import numpy as np
from numpy.polynomial.legendre import leggauss
from scipy.linalg import cholesky,solve_triangular
from concert_grand_coupled_contact import HammerString
from concert_grand_contact_study import CosineString


@dataclass(frozen=True)
class Support:
    frequency:tuple
    projection:tuple  # b=sum(phi_j*B_j), B_j mass-normalized, phi in kg^-1/2
    loss:tuple        # viscous c_j=loss_j*omega_j, modal mass normalized

    def __post_init__(self):
        if not len(self.frequency)==len(self.projection)==len(self.loss) or not len(self.frequency):raise ValueError('support dimensions')
        if not np.isfinite(np.r_[self.frequency,self.projection,self.loss]).all():raise ValueError('nonfinite support')
        if min(self.frequency)<=0 or min(self.loss)<0:raise ValueError('support frequency/loss')

    @classmethod
    def oscillator(cls,mass=.25,frequency=180.,loss=.05,coupling=1.):
        if mass<=0:raise ValueError('support mass')
        return cls((frequency,),(coupling/np.sqrt(mass),),(loss,))


def contact_footprint(nt,strike,width,nodes=512):
    """Positive Gaussian on the actual interval; force and displacement agree.

    sigma maps the historical exp(-1.2*(n*width)^2) Fourier filter. Truncating
    and renormalizing the real footprint also determines its endpoint moment.
    This is an explicit assumed distribution, not measured felt geometry.
    """
    if not 0<strike<1 or width<0:raise ValueError('contact geometry')
    if width==0:return np.sin(np.pi*np.arange(1,nt+1)*strike),strike
    x,w=leggauss(nodes);x=(x+1)/2;w=w/2;sigma=np.sqrt(2.4)/np.pi*width
    weight=w*np.exp(-.5*((x-strike)/sigma)**2);weight/=weight.sum()
    return weight@np.sin(np.pi*x[:,None]*np.arange(1,nt+1)),float(weight@x)


class MovingBridge:
    def __init__(self,wire,support,transverse=12,axial=None,bending=True,nonlinear=True):
        self.base=CosineString(wire,transverse,axial or 2*transverse,bending=bending)
        self.wire=wire;self.nt=transverse;self.nl=self.base.nl
        self.su=self.base.su;self.sv=self.base.sv;self.sw=slice(self.base.sw.start,self.base.size)
        self.ns=self.base.size;self.nb=len(support.frequency);self.size=self.ns+self.nb
        self.sb=slice(self.ns,self.size);self.support=support;self.nonlinear=nonlinear
        self.phi=np.asarray(support.projection);self.omega=2*np.pi*np.asarray(support.frequency)
        self.viscous=np.asarray(support.loss)*self.omega
        n=np.arange(1,self.nt+1)
        self.cross_vector=wire.linear_density*wire.length*(-1.)**(n+1)/(n*np.pi*np.sqrt(wire.modal_mass))
        self.cross=np.zeros((self.ns,self.nb));self.cross[self.sv]=self.cross_vector[:,None]*self.phi
        self.lift_mass=wire.linear_density*wire.length/3
        self.mass=np.eye(self.size)
        self.mass[self.sb,self.sb]+=self.lift_mass*np.outer(self.phi,self.phi)
        self.mass[:self.ns,self.sb]=self.cross;self.mass[self.sb,:self.ns]=self.cross.T
        schur=np.eye(self.nb)+(self.lift_mass-float(self.cross_vector@self.cross_vector))*np.outer(self.phi,self.phi)
        factor=cholesky(schur,lower=True)
        self.board_transform=solve_triangular(factor.T,np.eye(self.nb),lower=False)
        self.transform=np.eye(self.size)
        self.transform[:self.ns,self.sb]=-self.cross@self.board_transform
        self.transform[self.sb,self.sb]=self.board_transform
        self.inverse_transform=np.linalg.inv(self.transform)
        damping_z=np.zeros((self.size,self.size));damping_z[self.sb,self.sb]=np.diag(self.viscous)
        self.damping=self.transform.T@damping_z@self.transform
        self.damping=(self.damping+self.damping.T)/2

    def physical(self,Q):
        B=self.board_transform@Q[self.sb]
        return np.r_[Q[:self.ns]-self.cross@B,B]

    def normalized(self,z):return self.inverse_transform@z

    def pull_gradient(self,g):
        return np.r_[g[:self.ns],self.board_transform.T@(g[self.sb]-self.cross.T@g[:self.ns])]

    def fields(self,Q):
        z=self.physical(Q);base=self.base;b=float(self.phi@z[self.sb])
        ux=base.synthesize(base.ku*z[self.su])
        vx=base.synthesize(base.kv*z[self.sv])+b/self.wire.length
        wx=base.synthesize(base.kv*z[self.sw])
        strain=ux+.5*(vx*vx+wx*wx) if self.nonlinear else ux
        return z,b,ux,vx,wx,strain

    def potential(self,Q):
        z,b,ux,vx,wx,strain=self.fields(Q);base=self.base
        tension=.5*float(z[self.sv]@base.kt@z[self.sv]+z[self.sw]@base.kt@z[self.sw])
        return (tension+.5*self.wire.tension/self.wire.length*b*b
            +.5*self.wire.axial_rigidity*np.dot(base.weight,strain*strain)
            +.5*float((self.omega*z[self.sb])@(self.omega*z[self.sb])))

    def gradient_hessian(self,Q,hessian=True):
        z,b,ux,vx,wx,strain=self.fields(Q);base=self.base;ea=self.wire.axial_rigidity
        g=np.zeros(self.size)
        g[self.su]=ea*base.du.T@(base.weight*strain)
        g[self.sv]=base.kt@z[self.sv];g[self.sw]=base.kt@z[self.sw]
        g[self.sb]=self.omega**2*z[self.sb]+self.phi*self.wire.tension/self.wire.length*b
        if self.nonlinear:
            g[self.sv]+=ea*base.dt.T@(base.weight*strain*vx)
            g[self.sw]+=ea*base.dt.T@(base.weight*strain*wx)
            g[self.sb]+=self.phi*ea/self.wire.length*np.dot(base.weight,strain*vx)
        gn=self.pull_gradient(g)
        if not hessian:return gn
        H=np.zeros((self.size,self.size))
        H[self.sv,self.sv]=base.kt;H[self.sw,self.sw]=base.kt
        H[self.sb,self.sb]=np.diag(self.omega**2)+self.wire.tension/self.wire.length*np.outer(self.phi,self.phi)
        J=np.zeros((base.nodes,self.size));J[:,self.su]=base.du
        if self.nonlinear:
            J[:,self.sv]=vx[:,None]*base.dt;J[:,self.sw]=wx[:,None]*base.dt
            J[:,self.sb]=vx[:,None]*self.phi/self.wire.length
        H+=ea*J.T@(base.weight[:,None]*J)
        if self.nonlinear:
            gram=ea*base.dt.T@((base.weight*strain)[:,None]*base.dt)
            H[self.sv,self.sv]+=gram;H[self.sw,self.sw]+=gram
            mixed=ea*(base.dt.T@(base.weight*strain))[:,None]*self.phi/self.wire.length
            H[self.sv,self.sb]+=mixed;H[self.sb,self.sv]+=mixed.T
            H[self.sb,self.sb]+=ea*np.dot(base.weight,strain)*np.outer(self.phi,self.phi)/self.wire.length**2
        return gn,self.transform.T@H@self.transform

    def avf(self,a,b):
        d=b-a;c1=.5-.5/np.sqrt(3);c2=.5+.5/np.sqrt(3)
        g1,h1=self.gradient_hessian(a+c1*d);g2,h2=self.gradient_hessian(a+c2*d)
        return .5*(g1+g2),.5*(c1*h1+c2*h2)

    def energy(self,Q,P):return .5*float(P@P)+self.potential(Q)

    def components(self,Q,P):
        z,b,ux,vx,wx,strain=self.fields(Q);zdot=self.physical(P);base=self.base
        bd=float(self.phi@zdot[self.sb])
        string_k=(.5*float(zdot[:self.ns]@zdot[:self.ns])+bd*float(self.cross_vector@zdot[self.sv])+.5*self.lift_mass*bd*bd)
        board_e=.5*float(zdot[self.sb]@zdot[self.sb]+(self.omega*z[self.sb])@(self.omega*z[self.sb]))
        total=self.energy(Q,P);string_e=total-board_e
        return dict(string_energy_j=string_e,support_energy_j=board_e,string_kinetic_j=string_k,
                    bridge_displacement_m=b,bridge_velocity_m_s=bd,
                    support_loss_w=float(np.dot(self.viscous,zdot[self.sb]**2)),total_j=total)


class BridgeHammer(HammerString):
    def __init__(self,core,felt,footprint_nodes=512):
        self.string=core;self.felt=felt;self.size=core.size+1
        self.shape,self.lift_contact=contact_footprint(core.nt,felt.strike_fraction,felt.width_fraction,footprint_nodes)
        physical=np.zeros(core.size);physical[core.sv]=-self.shape/np.sqrt(core.wire.modal_mass)
        physical[core.sb]=-self.lift_contact*core.phi
        self.direction=np.r_[core.transform.T@physical,1/np.sqrt(felt.hammer_mass)]
        self.damping=np.zeros((self.size,self.size));self.damping[:-1,:-1]=core.damping
        self._step_cache=None

    def conservative_force(self,q,p):return super().continuous_force(q,p)

    def continuous_force(self,q,p):
        gradient,heat=self.conservative_force(q,p)
        return gradient+self.damping@p,heat+float(p@self.damping@p)

    def interface(self,q,p,acceleration):
        core=self.string;z=core.physical(q[:-1]);zd=core.physical(p[:-1]);za=core.physical(acceleration[:-1])
        b=float(core.phi@z[core.sb]);bd=float(core.phi@zd[core.sb]);ba=float(core.phi@za[core.sb])
        delta=max(self.compression(q),0.)
        force=self.felt.stiffness*delta**self.felt.exponent*np.exp(np.clip(self.felt.rate_damping_s_per_m*(self.direction@p),-40,40))
        _,_,ux,vx,wx,strain=core.fields(q[:-1])
        elastic=core.wire.tension/core.wire.length*b
        if core.nonlinear:elastic+=core.wire.axial_rigidity/core.wire.length*np.dot(core.base.weight,strain*vx)
        reaction=self.lift_contact*force-elastic-float(core.cross_vector@za[core.sv])-core.lift_mass*ba
        intrinsic=za[core.sb]+core.omega**2*z[core.sb]+core.viscous*zd[core.sb]
        residual=intrinsic-core.phi*reaction
        board_power=float(zd[core.sb]@(za[core.sb]+core.omega**2*z[core.sb]))
        loss=float(np.dot(core.viscous,zd[core.sb]**2))
        return dict(reaction_n=float(reaction),power_into_support_w=float(reaction*bd),support_energy_rate_w=board_power,
                    support_loss_w=loss,force_equation_residual=float(np.linalg.norm(residual)),
                    power_residual_w=float(reaction*bd-board_power-loss))

    def step(self,q,p,dt):
        if dt<=0:raise ValueError('positive dt required')
        if self._step_cache is None or self._step_cache[0]!=dt:
            plus=np.eye(self.size)+.5*dt*self.damping
            self._step_cache=(dt,plus,np.eye(self.size)-.5*dt*self.damping,np.linalg.inv(plus))
        _,plus,minus,inverse=self._step_cache
        candidate=q+dt*p;scale=max(np.linalg.norm(candidate),np.linalg.norm(dt*p),1e-12)
        for iteration in range(20):
            g,H,stats=self.avf(q,candidate,dt)
            residual=plus@(candidate-q)-dt*p+.5*dt*dt*g;norm=np.linalg.norm(residual)
            if norm<2e-14*scale:break
            update=np.linalg.solve(plus+.5*dt*dt*H,residual)
            for power in range(12):
                trial=candidate-update*2.**(-power);tg,_,_=self.avf(q,trial,dt)
                if np.linalg.norm(plus@(trial-q)-dt*p+.5*dt*dt*tg)<norm:
                    candidate=trial;break
            else:raise RuntimeError('bridge contact line search')
        else:raise RuntimeError('bridge contact Newton convergence')
        next_p=inverse@(minus@p-dt*g);mid=.5*(p+next_p)
        stats['support_loss_j']=dt*float(mid@self.damping@mid)
        stats['loss_j']=stats['felt_loss_j']+stats['support_loss_j'];stats['newton_iterations']=iteration+1
        return candidate,next_p,stats
