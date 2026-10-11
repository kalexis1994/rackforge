"""Two distinct real inputs to the living two-stage Gauss acoustic system.

Keep the measured constant-input implementation immutable. One complex
resolvent represents both real stages, but both port forces and radiation
packets survive. Two real residue contractions; no complex R copy/cache.
"""
import numpy as np
from concert_grand_gauss_air import GaussAirPlan


class StagedGaussAirPlan(GaussAirPlan):
    input_weights=np.array([.5+np.sqrt(3)/4-1j/4,
                            .5-np.sqrt(3)/4+1j/4])

    def __init__(self,air,dt,residues=None):
        super().__init__(air,dt,residues)
        B=air.relaxation.B
        Z=self.wave.port@self.wave.inner.impedance(1/(dt*self.a))@self.wave.port.T
        Z+=(B*(1/(1+self.alpha)))@B.T
        self.stage_A=np.array([[2*np.real(t*w*Z) for w in self.input_weights]
                               for t in self.stage_weights])

    def constant(self,r,z):
        # The parent validates the entire state and constructs causal offsets.
        # Avoid its force contraction: both real and imaginary components are
        # needed here, rather than only the averaged force.
        r=self.wave.inner.validate(r)
        if z.shape!=(self.air.relaxation.states,) or not np.isfinite(z).all():
            raise ValueError('finite complete relaxation state')
        from scipy.signal import lfilter
        source=(144*self.h*r[:,0]-12*r[:,1])/self.denominator
        offsets=lfilter([1.],[1.,-self.transfer],source,axis=0)
        branch=self.wave.port@(self.residues.matrix@offsets.real.ravel()
                              +1j*(self.residues.matrix@offsets.imag.ravel()))
        branch-=self.air.relaxation.B@(z/(1+self.alpha))
        return 2*np.real(self.stage_weights[:,None]*branch),offsets

    def update(self,r,z,velocities,offsets):
        r=self.wave.inner.validate(r)
        if velocities.shape!=(2,self.air.modes) or not np.isrealobj(velocities) or not np.isfinite(velocities).all():
            raise ValueError('two finite real boundary velocities required')
        if z.shape!=(self.air.relaxation.states,) or not np.isfinite(z).all():
            raise ValueError('finite complete relaxation state')
        if offsets.shape!=(self.wave.order,self.wave.inner.modes) or not np.isfinite(offsets).all():
            raise ValueError('complete finite offsets')
        branch_velocity=self.input_weights@velocities
        u=self.wave.port.T@branch_velocity
        inputs=np.empty((self.wave.order+1,self.wave.inner.modes),complex)
        inputs[0]=u;inputs[1:]=offsets+self.powers[:,None]*u
        center=np.empty_like(r,dtype=complex);rhs=r[:,1]+self.h*inputs[:-1]
        center[:,0]=((1+6*self.h)*r[:,0]+self.h*rhs)/self.denominator
        center[:,1]=(-12*self.h*r[:,0]+rhs)/self.denominator
        next_r=r+2*np.real(self.weight/self.a*(center-r))
        B=self.air.relaxation.B
        branch_drive=B.T@branch_velocity
        relax_center=(z+self.alpha*branch_drive)/(1+self.alpha)
        next_z=z+2*np.real(self.weight/self.a*(relax_center-z))
        relax_stages=2*np.real(self.stage_weights[:,None]*relax_center)
        drives=velocities@B
        loss=float(np.sum((drives-relax_stages)**2)/2)
        packets=2*np.real(self.stage_weights[:,None,None]*inputs)
        return next_r,next_z,loss,packets
