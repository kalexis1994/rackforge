"""SI nonlinear hammer/string/board coupled to the frozen acoustic bath.

Old mixed-mass normalized coordinates are retained. The air tail adds
C.T*M_tail*C to their kinetic mass; C maps to intrinsic physical board modes.
Felt/string AVF work and acoustic midpoint work share one mechanical velocity.
This is an offline vertical one-string pilot, not factory calibration.
"""
import numpy as np
from scipy.linalg import cho_factor,cho_solve


class BathContact:
    def __init__(self,system,bath,coupling=1.):
        core=system.string
        if bath.modes!=core.nb or not np.isfinite(coupling):raise ValueError('board/bath mapping')
        self.system=system;self.bath=bath;self.n=system.size;self.coupling=coupling
        self.board_map=np.zeros((core.nb,self.n));self.board_map[:,core.sb]=core.board_transform
        self.C=coupling*self.board_map
        self.M=np.eye(self.n)+self.C.T@bath.tail_mass@self.C
        self.mass_factor=cho_factor(self.M);self.cache=None

    def initial_state(self,speed):
        if not np.isfinite(speed) or speed<0:raise ValueError('incident hammer speed')
        q,v=self.system.initial_state(speed)
        return q,v,np.zeros(self.bath.ports),np.zeros(self.bath.ports)

    def energy(self,state):
        q,v,a,p=state;boundary=self.C@v
        dry=self.system.energy(q,v)
        air=.5*(boundary@self.bath.tail_mass@boundary+p@p+np.dot(self.bath.omega*a,self.bath.omega*a))
        return dict(mechanical_j=float(dry),air_j=float(air),total_j=float(dry+air))

    def derivative(self,state):
        q,v,a,p=state;g,felt_heat=self.system.conservative_force(q,v)
        acceleration=cho_solve(self.mass_factor,-g-self.system.damping@v-self.C.T@(self.bath.projection.T@p))
        return (v,acceleration,p,self.bath.projection@(self.C@v)-self.bath.omega**2*a-2*self.bath.gamma*p)

    def relaunch(self,state,speed):
        """Prescribed incident hammer, preserving string/board/air motion.

        Only after separation; not a backcheck/key-action trajectory. Removal
        of old hammer energy and injection of new energy are external work.
        """
        q,v,a,p=state
        if not np.isfinite(speed) or speed<0:raise ValueError('incident hammer speed')
        if self.system.compression(q)>1e-12:raise ValueError('hammer still in contact')
        new_q=q.copy();new_v=v.copy()
        new_q[-1]=-float(self.system.direction[:-1]@q[:-1])/self.system.direction[-1]
        new_v[-1]=np.sqrt(self.system.felt.hammer_mass)*speed
        result=(new_q,new_v,a.copy(),p.copy())
        return result,self.energy(result)['total_j']-self.energy(state)['total_j']

    def interface(self,state):
        q,v,a,p=state;core=self.system.string;acceleration=self.derivative(state)[1]
        old=self.system.interface(q,v,acceleration)
        z=core.physical(q[:-1]);zd=core.physical(v[:-1]);za=core.physical(acceleration[:-1])
        force=self.coupling**2*(self.bath.tail_mass@za[core.sb])+self.coupling*(self.bath.projection.T@p)
        residual=za[core.sb]+core.omega**2*z[core.sb]+core.board_damping@zd[core.sb]+force-core.phi*old['reaction_n']
        power=float(zd[core.sb]@force)
        return dict(reaction_n=old['reaction_n'],power_into_board_w=old['power_into_support_w'],
            power_into_air_w=power,force_residual=float(np.linalg.norm(residual)),
            power_residual_w=old['power_residual_w']-power)

    def step(self,state,dt):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive timestep')
        q,v,a,p=state;h=dt/2;G=self.bath.projection;w=self.bath.omega;gamma=self.bath.gamma
        if self.cache is None or self.cache[0]!=dt:
            denominator=1+2*h*gamma+h*h*w*w
            reduced=G.T@((1/denominator)[:,None]*G)
            plus=self.M+h*self.system.damping+h*h*(self.C.T@reduced@self.C)
            self.cache=(dt,denominator,plus,cho_factor(self.M+h*self.system.damping))
        _,denominator,plus,velocity_factor=self.cache
        free=(p-h*w*w*a)/denominator
        external=self.C.T@(G.T@free)
        candidate=q+dt*v;scale=max(np.linalg.norm(candidate),np.linalg.norm(dt*v),1e-12)
        for iteration in range(20):
            g,H,stats=self.system.avf(q,candidate,dt)
            residual=plus@(candidate-q)-dt*(self.M@v)+.5*dt*dt*(g+external)
            norm=np.linalg.norm(residual)
            if norm<2e-14*scale:break
            update=np.linalg.solve(plus+.5*dt*dt*H,residual)
            for power in range(12):
                trial=candidate-update*2.**(-power);tg,_,_=self.system.avf(q,trial,dt)
                if np.linalg.norm(plus@(trial-q)-dt*(self.M@v)+.5*dt*dt*(tg+external))<norm:
                    candidate=trial;break
            else:raise RuntimeError('bath contact line search')
        else:raise RuntimeError('bath contact Newton convergence')
        mid_v=(candidate-q)/dt
        mid_p=free+h/denominator*(G@(self.C@mid_v))
        # Momentum update avoids subtracting nearly equal displacements / dt.
        next_v=cho_solve(velocity_factor,(self.M-h*self.system.damping)@v-dt*(g+self.C.T@(G.T@mid_p)))
        result=(candidate,next_v,a+dt*mid_p,2*mid_p-p)
        boundary=self.C@mid_v;boundary_acceleration=self.C@((next_v-v)/dt)
        stats.update(material_loss_j=dt*float(mid_v@self.system.damping@mid_v),
            numerical_bath_absorption_j=dt*float(np.dot(2*gamma,mid_p*mid_p)),
            air_port_work_j=dt*float(boundary@(G.T@mid_p+self.bath.tail_mass@boundary_acceleration)),
            newton_iterations=iteration+1)
        stats['loss_j']=stats['felt_loss_j']+stats['material_loss_j']+stats['numerical_bath_absorption_j']
        core=self.system.string
        mid_B=self.board_map@(.5*(q+candidate));mid_Bv=self.board_map@mid_v
        physical_g=core.inverse_transform.T@g[:-1]
        phi2=float(core.phi@core.phi)
        driving=float((core.omega**2*mid_B-physical_g[core.sb])@core.phi/phi2) if phi2 else 0.
        physical_acceleration=core.physical(((next_v-v)/dt)[:-1])
        bridge_acceleration=float(core.phi@physical_acceleration[core.sb])
        reaction=driving-float(core.cross_vector@physical_acceleration[core.sv])-core.lift_mass*bridge_acceleration
        stats['work_into_board_j']=dt*reaction*float(core.phi@mid_Bv)
        return result,stats
