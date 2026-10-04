"""Passive reciprocal air-load benchmark; not a piano soundboard geometry.

Each port is a parallel resistance/inertance with F=R(v-u), M*u'=F.
The exact pulsating sphere is one physical realization. Modal projection
maps normalized mechanical velocity to physical boundary velocity; its
transpose returns force. Radiation is escaped wave energy, not material heat.
"""
import numpy as np


class PassiveAirLoad:
    def __init__(self,resistance,mass,projection):
        self.resistance=np.atleast_1d(np.asarray(resistance,dtype=float))
        self.mass=np.atleast_1d(np.asarray(mass,dtype=float));self.projection=np.asarray(projection,dtype=float)
        ports=len(self.mass)
        if self.mass.shape!=(ports,) or self.resistance.shape!=(ports,) or self.projection.ndim!=2 or self.projection.shape[0]!=ports or ports==0:raise ValueError('air-port dimensions')
        if self.projection.shape[1]==0 or not np.isfinite(np.r_[self.mass,self.resistance,self.projection.ravel()]).all():raise ValueError('finite air coefficients required')
        if min(self.mass.min(),self.resistance.min())<=0:raise ValueError('positive air mass/resistance required')
        self.ports=ports;self.mechanical_size=self.projection.shape[1];self.root_mass=np.sqrt(self.mass)
        self.relative=np.c_[self.projection,-np.diag(1/self.root_mass)]
        self.damping=self.relative.T@(self.resistance[:,None]*self.relative)
        self.damping=.5*(self.damping+self.damping.T)

    def response(self,P,W):
        v=self.projection@P;u=W/self.root_mass;relative=v-u;force=self.resistance*relative
        return dict(mechanical_force=self.projection.T@force,air_derivative=force/self.root_mass,
            boundary_force_n=force,boundary_velocity_m_s=v,air_velocity_m_s=u,
            port_power_w=float(force@v),escaped_power_w=float(np.dot(self.resistance,relative**2)),
            stored_energy_rate_w=float(W@(force/self.root_mass)))

    def impedance(self,frequency):
        return self.laplace_impedance(2j*np.pi*np.atleast_1d(frequency))

    def laplace_impedance(self,s):
        rows=[]
        for value in np.atleast_1d(s):
            z=self.resistance*value*self.mass/(self.resistance+value*self.mass)
            rows.append(self.projection.T@(z[:,None]*self.projection))
        return np.asarray(rows)


class SphereLoad(PassiveAirLoad):
    def __init__(self,radius,projection,density=1.2,speed=343.):
        self.radius=np.atleast_1d(np.asarray(radius,dtype=float))
        if not np.isfinite(np.r_[self.radius,density,speed]).all() or min(self.radius.min(),density,speed)<=0:raise ValueError('sphere/air parameters')
        self.area=4*np.pi*self.radius**2;self.density=density;self.speed=speed
        super().__init__(density*speed*self.area,density*self.area*self.radius,projection)

    def pressure_transfer(self,frequency,distance):
        """Pa per physical radial velocity; distance from sphere centre."""
        r=np.atleast_1d(np.asarray(distance,dtype=float))
        if r.shape!=self.radius.shape or not np.isfinite(r).all() or (r<self.radius).any():raise ValueError('observer outside sphere required')
        s=2j*np.pi*np.atleast_1d(frequency)[:,None];tau=self.radius/self.speed
        return self.density*self.speed*s*tau/(1+s*tau)*self.radius/r*np.exp(-s*(r-self.radius)/self.speed)


def contact_sphere(system,radius=.18,coupling=1.):
    """Map the experimental moving endpoint to a sphere's radial coordinate.

    The sphere is an analytical test load, not an equivalent Steinway panel.
    Mapping uses the same mixed-mass normalization as the mechanical pilot.
    """
    core=system.string;projection=np.zeros((1,system.size))
    projection[0,core.sb]=coupling*(core.phi@core.board_transform)
    return SphereLoad([radius],projection)


def linear_generator(stiffness,material_damping,load):
    """Energy-normalized mechanics + inertance; independent linear control."""
    K=np.asarray(stiffness,dtype=float);D=np.asarray(material_damping,dtype=float)
    n=load.mechanical_size;m=load.ports
    if K.shape!=(n,n) or D.shape!=(n,n):raise ValueError('linear mechanics dimensions')
    if not np.allclose(K,K.T) or not np.allclose(D,D.T) or np.linalg.eigvalsh(K).min()<=0 or np.linalg.eigvalsh(D).min()<-1e-12:raise ValueError('positive symmetric mechanics required')
    A=np.zeros((2*n+m,2*n+m));A[:n,n:2*n]=np.eye(n);A[n:2*n,:n]=-K
    A[n:,n:]=-load.damping;A[n:2*n,n:2*n]-=D
    H=np.zeros_like(A);H[:n,:n]=K;H[n:,n:]=np.eye(n+m)
    loss=np.zeros_like(A);loss[n:,n:]=load.damping;loss[n:2*n,n:2*n]+=D
    return A,H,loss


class AirCoupledContact:
    """Shared SI Hamiltonian with passive air inertance and outgoing channel.

    Extra coordinates integrate inertance velocity but have no elastic energy.
    Positive damping acts on relative port velocity, not separately on the
    board and air. Existing contact/felt potential is unchanged.
    """
    def __init__(self,system,load):
        if load.mechanical_size!=system.size:raise ValueError('contact/air dimensions')
        self.system=system;self.load=load;self.n=system.size;self.size=self.n+load.ports
        self.damping=load.damping.copy();self.damping[:self.n,:self.n]+=system.damping
        self._cache=None

    def initial_state(self,speed):
        q,p=self.system.initial_state(speed)
        return np.r_[q,np.zeros(self.load.ports)],np.r_[p,np.zeros(self.load.ports)]

    def energy(self,q,p):return self.system.energy(q[:self.n],p[:self.n])+.5*float(p[self.n:]@p[self.n:])

    def continuous_force(self,q,p):
        g,heat=self.system.continuous_force(q[:self.n],p[:self.n])
        port=self.load.response(p[:self.n],p[self.n:])
        return np.r_[g+port['mechanical_force'],-port['air_derivative']],heat,port

    def avf(self,a,b,dt):
        g,H,stats=self.system.avf(a[:self.n],b[:self.n],dt)
        extended=np.zeros((self.size,self.size));extended[:self.n,:self.n]=H
        return np.r_[g,np.zeros(self.load.ports)],extended,stats

    def step(self,q,p,dt):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive finite dt required')
        if self._cache is None or self._cache[0]!=dt:
            plus=np.eye(self.size)+.5*dt*self.damping
            self._cache=(dt,plus,np.eye(self.size)-.5*dt*self.damping,np.linalg.inv(plus))
        _,plus,minus,inverse=self._cache
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
            else:raise RuntimeError('air contact line search')
        else:raise RuntimeError('air contact Newton convergence')
        next_p=inverse@(minus@p-dt*g);mid=.5*(p+next_p);port=self.load.response(mid[:self.n],mid[self.n:])
        stats['material_loss_j']=dt*float(mid[:self.n]@self.system.damping@mid[:self.n])
        stats['escaped_wave_energy_j']=dt*port['escaped_power_w'];stats['port_work_j']=dt*port['port_power_w']
        stats['air_energy_delta_j']=.5*float(next_p[self.n:]@next_p[self.n:]-p[self.n:]@p[self.n:])
        stats['loss_j']=stats['felt_loss_j']+stats['material_loss_j']+stats['escaped_wave_energy_j']
        stats['newton_iterations']=iteration+1
        return candidate,next_p,stats
