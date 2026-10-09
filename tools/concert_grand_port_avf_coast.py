"""Coupled exponential-AVF coast with a displacement-average acoustic port.

Exact quadratic mechanical carriers, full geometric discrete gradient and
the unchanged passive midpoint acoustic operator share one port work.
No mechanical/air Strang split. All modes and living states stay present.
An experimental time integrator, not a new physical piano or factory backend.
"""
import numpy as np
from scipy.linalg import lu_factor,lu_solve
from concert_grand_packed_air import PackedCoast


class PortAVFCoast(PackedCoast):
    def __init__(self,system,air,residues=None):
        super().__init__(system,air,residues)
        self.port_cache=None

    def _port_prepare(self,dt):
        if self.port_cache is not None and self.port_cache['dt']==dt: return self.port_cache
        C,S,Q,W2=self.oscillator_functions(dt)
        core=self.system.string; air=self._air_prepare(dt)
        E=self.Db+self.Cb.T@air['A']@self.Cb
        group=[(indices,basis) for indices,_,basis in self.blocks if np.any(indices>=core.ns)]
        if len(group)!=1: raise ValueError('one coupled vertical/board block required')
        indices,basis=group[0]; board=np.flatnonzero(indices>=core.ns)
        if not np.array_equal(indices[board],np.arange(core.ns,core.size)):
            raise ValueError('complete ordered board block required')
        response=basis@(Q[indices,None]*basis[board].T)
        Qbb=response[board]
        # I+Qbb*E/dt has positive eigenvalues for positive port resistance.
        # No inverse of Qbb, so zeros of the oscillator response are safe.
        factor=lu_factor(np.eye(core.nb)+Qbb@E/dt,check_finite=False)
        self.port_cache=dict(dt=dt,C=C,S=S,Q=Q,W2=W2,E=E,indices=indices,
                             response=response,Qbb=Qbb,factor=factor,air=air)
        return self.port_cache

    def step(self,state,dt,defer_radiation=False):
        if not np.isfinite(dt) or dt<=0: raise ValueError('positive coupled coast interval')
        q,v,r,z=state; core=self.system.string; n=core.size; sb=core.sb
        if self.system.compression(q)>1e-12: raise ValueError('separated heads required')
        cache=self._port_prepare(dt); air=cache['air']
        constant,offsets=air['wave_plan'].constant(r)
        constant-=self.air.relaxation.B@(z/(1+air['relaxation_alpha']))
        external=self.Cb.T@constant
        a,p=self.project(q[:n]),self.project(v[:n])
        linear=cache['C']*a+cache['S']*p
        candidate=self.unproject(linear)
        scale=max(np.linalg.norm(a),np.linalg.norm(cache['S']*p),1e-12)
        for iteration in range(64):
            gradient=core.nonlinear_avf(q[:n],candidate)
            mode_gradient=self.project(gradient)
            predicted=self.unproject(linear-cache['Q']*mode_gradient)
            delta_board=lu_solve(cache['factor'],predicted[sb]-q[sb]-cache['Qbb']@external,check_finite=False)
            force=cache['E']@(delta_board/dt)+external
            target=predicted.copy(); target[cache['indices']]-=cache['response']@force
            residual=target-candidate
            if np.linalg.norm(residual)<2e-14*scale:
                break
            candidate=target
        else: raise RuntimeError('coupled port AVF did not converge; refine dt')
        # Use the actual displacement average for both sides of the port.
        mean=(candidate[sb]-q[sb])/dt; boundary=self.Cb@mean
        load=air['A']@boundary+constant
        total_gradient=gradient.copy(); total_gradient[sb]+=self.Db@mean+self.Cb.T@load
        next_q=np.r_[candidate,q[n:]+dt*v[n:]]
        next_v=np.r_[self.unproject(-cache['W2']*cache['S']*a+cache['C']*p
                                   -cache['S']*self.project(total_gradient)),v[n:]]
        if self.system.compression(next_q)>1e-12: raise ValueError('head recontact requires contact solver')
        next_r,inputs=air['wave_plan'].update(r,boundary,offsets)
        next_z,_,relax=self.air.relaxation.midpoint(z,boundary,dt)
        radiation=relax if defer_radiation else relax+self.air.wave.radiation_batch(inputs[None])[0]
        stats=dict(force_n=0.,felt_loss_j=0.,material_loss_j=dt*float(mean@self.Db@mean),
            air_port_work_j=dt*float(boundary@load),radiation_loss_j=dt*float(radiation),
            mechanical_iterations=iteration+1,full_tangent_fallbacks=0,
            residual_relative=float(np.linalg.norm(residual)/scale))
        stats['loss_j']=stats['material_loss_j']+stats['radiation_loss_j']
        if defer_radiation: stats['wave_inputs']=inputs
        return (next_q,next_v,next_r,next_z),stats
