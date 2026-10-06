"""Energy-accounted AAVF mechanical coast with a passive split air port.

After separated contact only. All modes and living acoustic states remain.
The fixed quadratic Hamiltonian evolves by its exact oscillator functions;
the full cubic/quartic remainder uses a two-point discrete gradient.
Symmetric mechanical-half / passive-air-full / mechanical-half composition.
Method reference: Wang/Wu, arXiv:1807.01044, eq. (5). Coarse accuracy and
numerical resonances need measurements; energy preservation alone is not
evidence of waveform fidelity. This is an offline pilot, not factory code.
"""
import numpy as np
from scipy.linalg import eigh,cho_factor,cho_solve
from scipy.signal import lfilter
from concert_grand_bounded_contact import BoundedContact
from concert_grand_pade_air import PadeAir
from concert_grand_pade_midpoint import PadeMidpoint


class ExponentialCoast(BoundedContact):
    def __init__(self,system,air,coupling=1.):
        super().__init__(system,air,coupling)
        core=system.string
        self.K=core.gradient_hessian(np.zeros(core.size))[1]
        # These are invariant blocks of the quadratic Hamiltonian, not a
        # reduced basis. Both transverse directions and every axial mode stay.
        groups=[np.arange(core.su.start,core.su.stop),
                np.r_[np.arange(core.sv.start,core.sv.stop),np.arange(core.sb.start,core.sb.stop)],
                np.arange(core.sw.start,core.sw.stop)]
        self.blocks=[]
        reconstruction=np.zeros_like(self.K)
        for indices in groups:
            block=self.K[np.ix_(indices,indices)]
            values,basis=eigh(block)
            if values[0]<=0:raise ValueError('positive quadratic mechanical block required')
            reconstruction[np.ix_(indices,indices)]=block
            self.blocks.append((indices,np.sqrt(values),basis))
        if not np.array_equal(reconstruction,self.K):raise ValueError('quadratic coupling crosses selected blocks')
        self.Cb=self.C[:,core.sb]
        self.Db=system.damping[core.sb,core.sb]
        residual=system.damping.copy();residual[core.sb,core.sb]=0
        if np.any(residual):raise ValueError('board-only material damping required')
        self.pade=isinstance(air.wave,PadeAir)
        self.residue_matrix=(None if self.pade else
            np.ascontiguousarray(air.wave.R[1:].transpose(1,0,2)).reshape(air.modes,-1))
        self.mechanical_cache=None;self.air_cache=None

    def project(self,vector):
        result=np.empty_like(vector)
        for indices,_,basis in self.blocks:result[indices]=basis.T@vector[indices]
        return result

    def unproject(self,vector):
        result=np.empty_like(vector)
        for indices,_,basis in self.blocks:result[indices]=basis@vector[indices]
        return result

    def nonlinear_gradient(self,q):
        core=self.system.string;base=core.base
        if not core.nonlinear:return np.zeros_like(q)
        _,_,ux,vx,wx,strain=core.fields(q)
        # Evaluate the remainder directly; subtracting K*q from the full
        # force would cancel the very large axial terms at small amplitudes.
        eta=.5*(vx*vx+wx*wx);ea=core.wire.axial_rigidity
        g=np.zeros_like(q)
        g[core.su]=ea*base.du.T@(base.weight*eta)
        g[core.sv]=ea*base.dt.T@(base.weight*strain*vx)
        g[core.sw]=ea*base.dt.T@(base.weight*strain*wx)
        g[core.sb]=core.phi*ea/core.wire.length*np.dot(base.weight,strain*vx)
        return core.pull_gradient(g)

    def nonlinear_avf(self,a,b):
        delta=b-a;c1=.5-.5/np.sqrt(3);c2=.5+.5/np.sqrt(3)
        return .5*(self.nonlinear_gradient(a+c1*delta)+self.nonlinear_gradient(a+c2*delta))

    def oscillator_functions(self,dt):
        if self.mechanical_cache is not None and self.mechanical_cache[0]==dt:return self.mechanical_cache[1:]
        n=self.system.string.size;C=np.empty(n);S=np.empty(n);Q=np.empty(n);W2=np.empty(n)
        for indices,omega,_ in self.blocks:
            phase=omega*dt;C[indices]=np.cos(phase)
            S[indices]=dt*np.sinc(phase/np.pi)
            Q[indices]=.5*dt*dt*np.sinc(phase/(2*np.pi))**2
            W2[indices]=omega**2
        self.mechanical_cache=(dt,C,S,Q,W2)
        return C,S,Q,W2

    def mechanical_step(self,q,v,dt):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive step')
        if self.system.compression(q)>=0:raise ValueError('coast requires a separated hammer')
        C,S,Q,W2=self.oscillator_functions(dt)
        a=self.project(q[:-1]);p=self.project(v[:-1]);linear=C*a+S*p
        candidate=linear.copy()
        scale=max(np.linalg.norm(a),np.linalg.norm(S*p),1e-12)
        old_norm=np.inf;fallback=0
        for iteration in range(30):
            physical=self.unproject(candidate)
            gradient=self.project(self.nonlinear_avf(q[:-1],physical))
            residual=candidate-linear+Q*gradient
            norm=np.linalg.norm(residual)
            if norm<2e-14*scale:break
            if iteration<10 and norm<old_norm:
                candidate-=residual
            else:
                # Endpoint derivative of the remainder's AVF. Only needed
                # if fixed-point iteration stops contracting.
                _,H=self.system.string.avf(q[:-1],physical)
                H-=self.K/2
                basis=np.zeros_like(self.K)
                for indices,_,U in self.blocks:basis[np.ix_(indices,indices)]=U
                J=np.eye(len(a))+Q[:,None]*(basis.T@H@basis)
                update=np.linalg.solve(J,residual);fallback+=1
                for power in range(12):
                    trial=candidate-update*2.**(-power)
                    tg=self.project(self.nonlinear_avf(q[:-1],self.unproject(trial)))
                    if np.linalg.norm(trial-linear+Q*tg)<norm:
                        candidate=trial;break
                else:raise RuntimeError('exponential mechanical line search')
            old_norm=norm
        else:raise RuntimeError('exponential mechanical discrete gradient did not converge')
        result_q=np.r_[physical,q[-1]+dt*v[-1]]
        result_v=np.r_[self.unproject(-W2*S*a+C*p-S*gradient),v[-1]]
        if self.system.compression(result_q)>=0:raise ValueError('hammer recontact needs the contact solver')
        return result_q,result_v,dict(mechanical_iterations=iteration+1,full_tangent_fallbacks=fallback)

    def _air_prepare(self,dt):
        if self.air_cache is not None and self.air_cache['dt']==dt:return self.air_cache
        A=self.air.impedance(2/dt).real;h=dt/2
        wave_plan=PadeMidpoint(self.air.wave,dt) if self.pade else None
        alpha=None if self.pade else dt*self.air.wave.a/2
        beta=None if self.pade else (alpha-1)/(alpha+1)
        self.air_cache=dict(dt=dt,A=A,wave_plan=wave_plan,alpha=alpha,beta=beta,
            powers=None if self.pade else beta**np.arange(1,self.air.wave.order+1),
            relaxation_alpha=dt/(2*self.air.relaxation.tau),
            factor=cho_factor(np.eye(self.air.modes)+h*self.Db+h*self.Cb.T@A@self.Cb))
        return self.air_cache

    def air_step(self,state,dt,defer_radiation=False):
        q,v,r,z=state;core=self.system.string;cache=self._air_prepare(dt)
        if self.pade:constant,offsets=cache['wave_plan'].constant(r)
        else:
            offsets=lfilter([2/(1+cache['alpha'])],[1,-cache['beta']],r,axis=0)
            constant=self.residue_matrix@offsets.ravel()
        constant-=self.air.relaxation.B@(z/(1+cache['relaxation_alpha']))
        center=cho_solve(cache['factor'],v[core.sb]-dt/2*self.Cb.T@constant)
        boundary=self.Cb@center;load=cache['A']@boundary+constant
        if self.pade:new_r,inputs=cache['wave_plan'].update(r,boundary,offsets)
        else:
            inputs=np.empty((self.air.wave.order+1,self.air.modes));inputs[0]=boundary
            inputs[1:]=offsets+cache['powers'][:,None]*boundary
            new_r=inputs[1:]+inputs[:-1]-r
        new_z,_,relax=self.air.relaxation.midpoint(z,boundary,dt)
        new_v=v.copy();new_v[core.sb]=2*center-v[core.sb]
        radiation=relax if defer_radiation else relax+self.air.wave.radiation_batch(inputs[None])[0]
        stats=dict(material_loss_j=dt*float(center@self.Db@center),radiation_loss_j=dt*float(radiation),
                   air_port_work_j=dt*float(boundary@load),felt_loss_j=0.,force_n=0.)
        stats['loss_j']=stats['material_loss_j']+stats['radiation_loss_j']
        if defer_radiation:stats['wave_inputs']=inputs
        return (q,new_v,new_r,new_z),stats

    def step(self,state,dt,defer_radiation=False):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive step')
        q,v,a=self.mechanical_step(state[0],state[1],dt/2)
        middle,stats=self.air_step((q,v,state[2],state[3]),dt,defer_radiation)
        q,v,b=self.mechanical_step(middle[0],middle[1],dt/2)
        stats['mechanical_iterations']=max(a['mechanical_iterations'],b['mechanical_iterations'])
        stats['full_tangent_fallbacks']=a['full_tangent_fallbacks']+b['full_tangent_fallbacks']
        return (q,v,middle[2],middle[3]),stats
