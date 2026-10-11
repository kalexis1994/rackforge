"""Exact mechanical half carriers with two distinct Gauss air port forces.

Each force does work against its own half displacement. All nonlinear AVF
gradients, air memory and modes remain present. Half displacement means do
not coincide with Gauss nodes: no fourth-order claim for this composition.
"""
import numpy as np
from scipy.linalg import lu_factor,lu_solve
from concert_grand_port_avf_coast import PortAVFCoast
from concert_grand_staged_gauss_air import StagedGaussAirPlan


class StagedPortCoast(PortAVFCoast):
    def _port_prepare(self,dt):
        if self.port_cache is not None and self.port_cache['dt']==dt:return self.port_cache
        core=self.system.string
        C,S,Q,W2=self.oscillator_functions(dt)
        Ch,Sh,Qh,_=self.oscillator_functions(dt/2)
        group=[(indices,basis) for indices,_,basis in self.blocks if np.any(indices>=core.ns)]
        if len(group)!=1:raise ValueError('one coupled vertical/board block required')
        indices,basis=group[0];board=np.flatnonzero(indices>=core.ns)
        if not np.array_equal(indices[board],np.arange(core.ns,core.size)):
            raise ValueError('complete ordered board block required')
        H=basis@(Qh[indices,None]*basis[board].T)
        J=basis@((2*Ch[indices]*Qh[indices])[:,None]*basis[board].T)
        plan=StagedGaussAirPlan(self.air,dt,self.packed_residues)
        E=np.array([[self.Cb.T@plan.stage_A[i,j]@self.Cb
                     +(self.Db if i==j else 0.) for j in range(2)] for i in range(2)])
        resistance=np.block([[E[0,0],E[0,1]],[E[1,0],E[1,1]]])
        response=np.block([[H[board],np.zeros((core.nb,core.nb))],[J[board],H[board]]])
        factor=lu_factor(np.eye(2*core.nb)+response@resistance/(dt/2),check_finite=False)
        self.port_cache=dict(dt=dt,C=C,S=S,Q=Q,W2=W2,Ch=Ch,Sh=Sh,Qh=Qh,
                             indices=indices,H=H,J=J,plan=plan,E=E,
                             response=response,factor=factor)
        return self.port_cache

    def step(self,state,dt,defer_radiation=False):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive coupled coast interval')
        q,v,r,z=state;core=self.system.string;n=core.size;sb=core.sb
        if self.system.compression(q)>1e-12:raise ValueError('separated heads required')
        c=self._port_prepare(dt);half=dt/2
        constant,offsets=c['plan'].constant(r,z)
        external=constant@self.Cb
        a,p=self.project(q[:n]),self.project(v[:n])
        half_linear=c['Ch']*a+c['Sh']*p
        linear=c['C']*a+c['S']*p
        middle=self.unproject(half_linear);candidate=self.unproject(linear)
        scale=max(np.linalg.norm(a),np.linalg.norm(c['S']*p),1e-12)
        for iteration in range(64):
            g1=core.nonlinear_avf(q[:n],middle);g2=core.nonlinear_avf(middle,candidate)
            m1,m2=self.project(g1),self.project(g2)
            mid_pred=self.unproject(half_linear-c['Qh']*m1)
            end_pred=self.unproject(linear-c['Qh']*((1+2*c['Ch'])*m1+m2))
            increments=np.r_[mid_pred[sb]-q[sb],end_pred[sb]-mid_pred[sb]]
            delta=lu_solve(c['factor'],increments-c['response']@external.ravel(),check_finite=False).reshape(2,core.nb)
            force=np.einsum('ijab,jb->ia',c['E'],delta/half)+external
            mid_target=mid_pred.copy();end_target=end_pred.copy()
            mid_target[c['indices']]-=c['H']@force[0]
            end_target[c['indices']]-=(c['H']+c['J'])@force[0]+c['H']@force[1]
            residual=max(np.linalg.norm(mid_target-middle),np.linalg.norm(end_target-candidate))
            if residual<2e-14*scale:break
            middle,candidate=mid_target,end_target
        else:raise RuntimeError('two-force coupled AVF did not converge; refine dt')
        means=np.array([(middle[sb]-q[sb])/half,(candidate[sb]-middle[sb])/half])
        boundary=means@self.Cb.T
        loads=np.einsum('ijab,jb->ia',c['plan'].stage_A,boundary)+constant
        ports=np.zeros((2,n));ports[:,sb]=means@self.Db.T+loads@self.Cb
        f1,f2=self.project(ports[0]),self.project(ports[1])
        next_q=np.r_[candidate,q[n:]+dt*v[n:]]
        next_v=np.r_[self.unproject(-c['W2']*c['S']*a+c['C']*p
                    -c['Sh']*((2*c['Ch']-1)*(m1+f1)+m2+f2)),v[n:]]
        middle_q=np.r_[middle,q[n:]+half*v[n:]]
        if max(self.system.compression(next_q),self.system.compression(middle_q))>1e-12:
            raise ValueError('head recontact requires contact solver')
        next_r,next_z,relax,packets=c['plan'].update(r,z,boundary,offsets)
        radiation=relax if defer_radiation else relax+float(np.mean(self.air.wave.radiation_batch(packets)))
        stats=dict(force_n=0.,felt_loss_j=0.,material_loss_j=half*float(np.einsum('ia,ab,ib',means,self.Db,means)),
                   air_port_work_j=half*float(np.sum(boundary*loads)),radiation_loss_j=dt*float(radiation),
                   mechanical_iterations=iteration+1,full_tangent_fallbacks=0,
                   residual_relative=float(residual/scale),nonlinear_intervals=2,distinct_port_forces=2)
        stats['loss_j']=stats['material_loss_j']+stats['radiation_loss_j']
        if defer_radiation:stats['wave_inputs_stages']=packets
        return (next_q,next_v,next_r,next_z),stats
