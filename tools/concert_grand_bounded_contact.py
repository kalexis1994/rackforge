"""SI hammer/string/bridge coupled to finite-limit passive radiation.

Mixed-mass mechanical coordinates are unchanged. C maps their velocity to
physical P2 board modes. AVF felt/string work and midpoint acoustic work use
the same centered mechanical velocity; no air mass is added instantaneously.
Offline vertical one-string pilot, not factory calibration or action model.
"""
import numpy as np
from scipy.linalg import cho_factor,cho_solve


class BoundedContact:
    def __init__(self,system,air,coupling=1.):
        core=system.string
        if air.modes!=core.nb or not np.isfinite(coupling):raise ValueError('physical board/air mapping')
        self.system=system;self.air=air;self.n=system.size;self.coupling=coupling
        self.board_map=np.zeros((core.nb,self.n));self.board_map[:,core.sb]=core.board_transform
        self.C=coupling*self.board_map;self.cache=None

    def initial_state(self,speed):
        if not np.isfinite(speed) or speed<0:raise ValueError('incident hammer speed')
        q,v=self.system.initial_state(speed)
        return q,v,*self.air.zero_state()

    def energy(self,state):
        dry=self.system.energy(state[0],state[1]);air=self.air.energy(state[2:])
        return dict(mechanical_j=float(dry),air_j=float(air),total_j=float(dry+air))

    def derivative(self,state):
        q,v,r,z=state;g,_=self.system.conservative_force(q,v)
        air_derivative,force=self.air.derivative((r,z),self.C@v)
        return v,-g-self.system.damping@v-self.C.T@force,*air_derivative

    def relaunch(self,state,speed):
        """Prescribed head after separation; preserve all living states.

        External work accounts for resetting the separated hammer only.
        Not a simulated backcheck, key action or automatic repetition.
        """
        q,v,r,z=state
        if not np.isfinite(speed) or speed<0:raise ValueError('incident speed')
        if self.system.compression(q)>1e-12:raise ValueError('hammer still in contact')
        new_q=q.copy();new_v=v.copy()
        new_q[-1]=-float(self.system.direction[:-1]@q[:-1])/self.system.direction[-1]
        new_v[-1]=np.sqrt(self.system.felt.hammer_mass)*speed
        result=(new_q,new_v,r.copy(),z.copy())
        # The preserved air energy cancels exactly; evaluate mechanical work
        # without two expensive FIR storage convolutions.
        return result,self.system.energy(new_q,new_v)-self.system.energy(q,v)

    def interface(self,state):
        q,v,r,z=state;core=self.system.string;acceleration=self.derivative(state)[1]
        old=self.system.interface(q,v,acceleration)
        physical=core.physical(q[:-1]);velocity=core.physical(v[:-1]);a=core.physical(acceleration[:-1])
        _,port=self.air.derivative((r,z),self.C@v);load=self.coupling*port
        residual=a[core.sb]+core.omega**2*physical[core.sb]+core.board_damping@velocity[core.sb]+load-core.phi*old['reaction_n']
        power=float(velocity[core.sb]@load)
        return dict(reaction_n=old['reaction_n'],power_into_board_w=old['power_into_support_w'],power_into_air_w=power,
            force_residual=float(np.linalg.norm(residual)),power_residual_w=old['power_residual_w']-power)

    def step(self,state,dt,defer_radiation=False):
        """Advance the coupled state without altering the physical port.

        With defer_radiation, loss_j/radiation_loss_j include relaxation only.
        The caller must add dt*wave.radiation_batch([wave_inputs])[0] to close
        the radiation ledger. This changes observer cost, never the dynamics.
        """
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive step')
        q,v,r,z=state;h=dt/2
        if self.cache is None or self.cache[0]!=dt:
            A=self.air.impedance(2/dt).real
            plus=np.eye(self.n)+h*self.system.damping+h*self.C.T@A@self.C
            self.cache=(dt,plus,cho_factor(np.eye(self.n)+h*self.system.damping))
        _,plus,velocity_factor=self.cache
        constant=self.air.centered_constant((r,z),dt);external=self.C.T@constant
        candidate=q+dt*v;scale=max(np.linalg.norm(candidate),np.linalg.norm(dt*v),1e-12)
        for iteration in range(20):
            g,H,stats=self.system.avf(q,candidate,dt)
            residual=plus@(candidate-q)-dt*v+.5*dt*dt*(g+external)
            norm=np.linalg.norm(residual)
            if norm<2e-14*scale:break
            update=np.linalg.solve(plus+.5*dt*dt*H,residual)
            for power in range(12):
                trial=candidate-update*2.**(-power);tg,_,_=self.system.avf(q,trial,dt)
                if np.linalg.norm(plus@(trial-q)-dt*v+.5*dt*dt*(tg+external))<norm:
                    candidate=trial;break
            else:raise RuntimeError('bounded contact line search')
        else:raise RuntimeError('bounded contact Newton convergence')
        center=(candidate-q)/dt;boundary=self.C@center
        if defer_radiation:
            updated,force,radiated,inputs=self.air.midpoint_deferred((r,z),boundary,dt)
            stats['wave_inputs']=inputs
        else:updated,force,radiated=self.air.midpoint((r,z),boundary,dt)
        next_v=cho_solve(velocity_factor,(np.eye(self.n)-h*self.system.damping)@v-dt*(g+self.C.T@force))
        result=(candidate,next_v,*updated)
        stats.update(material_loss_j=dt*float(center@self.system.damping@center),radiation_loss_j=dt*radiated,
            air_port_work_j=dt*float(boundary@force),newton_iterations=iteration+1)
        stats['loss_j']=stats['felt_loss_j']+stats['material_loss_j']+stats['radiation_loss_j']
        core=self.system.string;mid_B=self.board_map@(.5*(q+candidate));mid_Bv=self.board_map@center
        physical_g=core.inverse_transform.T@g[:-1];phi2=float(core.phi@core.phi)
        driving=float((core.omega**2*mid_B-physical_g[core.sb])@core.phi/phi2) if phi2 else 0.
        acceleration=core.physical(((next_v-v)/dt)[:-1]);bridge_acceleration=float(core.phi@acceleration[core.sb])
        reaction=driving-float(core.cross_vector@acceleration[core.sv])-core.lift_mass*bridge_acceleration
        stats['work_into_board_j']=dt*reaction*float(core.phi@mid_Bv)
        return result,stats
