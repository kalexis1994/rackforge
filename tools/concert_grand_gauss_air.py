"""Two-stage Gauss air with one real residue contraction per interval.

The conjugate stage pair is resolved through one complex Padé cascade.
Force averaging happens before contracting the real residues, so there
is no complex copy of the large kernel or second residue GEMV. Radiation
keeps both real stages. Continuous coefficients and living states unchanged.
"""
import numpy as np
from scipy.signal import lfilter
from concert_grand_packed_air import PackedResidues


class GaussAirPlan:
    a=.25+1j*np.sqrt(3)/12
    weight=.5-1j*np.sqrt(3)/2
    nodes=np.array([.5-np.sqrt(3)/6,.5+np.sqrt(3)/6])
    stage_weights=(nodes-a.conjugate())/(a-a.conjugate())

    def __init__(self,air,dt,residues=None):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive finite air interval')
        self.air=air;self.wave=air.wave;self.dt=dt
        self.residues=residues if residues is not None else PackedResidues(self.wave)
        if self.residues.wave is not self.wave:raise ValueError('same wave residues required')
        self.h=dt*self.a*self.wave.inner.rate
        self.denominator=1+6*self.h+12*self.h*self.h
        self.transfer=(1-6*self.h+12*self.h*self.h)/self.denominator
        self.powers=self.transfer**np.arange(1,self.wave.order+1)
        self.alpha=dt*self.a/air.relaxation.tau
        s=1/(dt*self.a)
        wave_matrix=self.wave.port@(2*np.real(self.weight*self.wave.inner.impedance(s)))@self.wave.port.T
        B=air.relaxation.B
        self.A=wave_matrix+(B*(2*np.real(self.weight/(1+self.alpha))))@B.T

    def constant(self,r,z):
        r=self.wave.inner.validate(r)
        if z.shape!=(self.air.relaxation.states,) or not np.isfinite(z).all():
            raise ValueError('finite complete relaxation state')
        source=(144*self.h*r[:,0]-12*r[:,1])/self.denominator
        offsets=lfilter([1.],[1.,-self.transfer],source,axis=0)
        # Linearity permits force-stage averaging before the large GEMV.
        averaged=2*np.real(self.weight*offsets)
        force=self.wave.port@(self.residues.matrix@averaged.ravel())
        force-=self.air.relaxation.B@(2*np.real(self.weight*z/(1+self.alpha)))
        return force,offsets

    def update(self,r,z,velocity,offsets):
        r=self.wave.inner.validate(r)
        if velocity.shape!=(self.air.modes,) or not np.isfinite(velocity).all():
            raise ValueError('finite boundary velocity')
        if offsets.shape!=(self.wave.order,self.wave.inner.modes):raise ValueError('complete offsets')
        u=self.wave.port.T@velocity
        inputs=np.empty((self.wave.order+1,self.wave.inner.modes),complex)
        inputs[0]=u;inputs[1:]=offsets+self.powers[:,None]*u
        center=np.empty_like(r,dtype=complex);rhs=r[:,1]+self.h*inputs[:-1]
        center[:,0]=((1+6*self.h)*r[:,0]+self.h*rhs)/self.denominator
        center[:,1]=(-12*self.h*r[:,0]+rhs)/self.denominator
        next_r=r+2*np.real(self.weight/self.a*(center-r))
        B=self.air.relaxation.B;drive=B.T@velocity
        relax_center=(z+self.alpha*drive)/(1+self.alpha)
        next_z=z+2*np.real(self.weight/self.a*(relax_center-z))
        relax_stages=2*np.real(self.stage_weights[:,None]*relax_center)
        loss=float(np.sum((drive-relax_stages)**2)/2)
        packets=2*np.real(self.stage_weights[:,None,None]*inputs)
        return next_r,next_z,loss,packets
