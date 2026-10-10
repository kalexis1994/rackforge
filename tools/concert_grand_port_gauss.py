"""Nonlinear half carriers sharing a two-stage Gauss acoustic port.

Compose exact quadratic carriers and two full geometric AVF gradients.
The constant acoustic force does work against the complete displacement;
the unchanged continuous air is advanced by two Gauss stages per interval.
Experimental integration method, not a change of physical piano coefficients.
"""
import numpy as np
from scipy.linalg import lu_solve
from concert_grand_port_avf_coast import PortAVFCoast
from concert_grand_gauss_air import GaussAirPlan


class PortGaussCoast(PortAVFCoast):
    def __init__(self,system,air,residues=None):
        super().__init__(system,air,residues)
        self.gauss_cache=None

    def _air_prepare(self,dt):
        if self.gauss_cache is None or self.gauss_cache['dt']!=dt:
            plan=GaussAirPlan(self.air,dt,self.packed_residues)
            self.gauss_cache=dict(dt=dt,plan=plan,A=plan.A)
        return self.gauss_cache

    def _port_prepare(self,dt):
        cache=super()._port_prepare(dt)
        if 'half_response' not in cache:
            Ch,Sh,Qh,_=self.oscillator_functions(dt/2)
            indices=cache['indices']
            basis=next(b for i,_,b in self.blocks if np.array_equal(i,indices))
            board=np.flatnonzero(indices>=self.system.string.ns)
            cache.update(Ch=Ch,Sh=Sh,Qh=Qh,
                         half_response=basis@(Qh[indices,None]*basis[board].T))
        return cache

    def step(self,state,dt,defer_radiation=False):
        if not np.isfinite(dt) or dt<=0: raise ValueError('positive coupled coast interval')
        q,v,r,z=state; core=self.system.string; n=core.size; sb=core.sb
        if self.system.compression(q)>1e-12: raise ValueError('separated heads required')
        c=self._port_prepare(dt); air=c['air']
        constant,offsets=air['plan'].constant(r,z)
        external=self.Cb.T@constant
        a,p=self.project(q[:n]),self.project(v[:n])
        half_linear=c['Ch']*a+c['Sh']*p
        linear=c['C']*a+c['S']*p
        middle=self.unproject(half_linear); candidate=self.unproject(linear)
        scale=max(np.linalg.norm(a),np.linalg.norm(c['S']*p),1e-12)
        for iteration in range(64):
            g1=core.nonlinear_avf(q[:n],middle)
            g2=core.nonlinear_avf(middle,candidate)
            m1,m2=self.project(g1),self.project(g2)
            predicted=self.unproject(linear-c['Qh']*((1+2*c['Ch'])*m1+m2))
            delta=lu_solve(c['factor'],predicted[sb]-q[sb]-c['Qbb']@external,check_finite=False)
            force=c['E']@(delta/dt)+external
            target=predicted.copy(); target[c['indices']]-=c['response']@force
            half_target=self.unproject(half_linear-c['Qh']*m1)
            half_target[c['indices']]-=c['half_response']@force
            residual=max(np.linalg.norm(target-candidate),np.linalg.norm(half_target-middle))
            if residual<2e-14*scale: break
            candidate,middle=target,half_target
        else: raise RuntimeError('coupled half AVF did not converge; refine dt')
        mean=(candidate[sb]-q[sb])/dt; boundary=self.Cb@mean
        load=air['A']@boundary+constant
        port=np.zeros(n); port[sb]=self.Db@mean+self.Cb.T@load
        next_q=np.r_[candidate,q[n:]+dt*v[n:]]
        next_v=np.r_[self.unproject(-c['W2']*c['S']*a+c['C']*p
            -c['Sh']*((2*c['Ch']-1)*m1+m2)-c['S']*self.project(port)),v[n:]]
        middle_q=np.r_[middle,q[n:]+dt/2*v[n:]]
        if max(self.system.compression(next_q),self.system.compression(middle_q))>1e-12:
            raise ValueError('head recontact requires contact solver')
        next_r,next_z,relax,inputs=air['plan'].update(r,z,boundary,offsets)
        radiation=relax if defer_radiation else relax+float(np.mean(self.air.wave.radiation_batch(inputs)))
        stats=dict(force_n=0.,felt_loss_j=0.,material_loss_j=dt*float(mean@self.Db@mean),
            air_port_work_j=dt*float(boundary@load),radiation_loss_j=dt*float(radiation),
            mechanical_iterations=iteration+1,full_tangent_fallbacks=0,
            residual_relative=float(residual/scale),nonlinear_intervals=2)
        stats['loss_j']=stats['material_loss_j']+stats['radiation_loss_j']
        if defer_radiation: stats['wave_inputs_stages']=inputs
        return (next_q,next_v,next_r,next_z),stats
