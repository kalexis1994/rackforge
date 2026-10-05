"""Passive distributed string damper on the existing live SI coast.

First release approximation: nonnegative viscous traction over a felt patch,
not contact impact, felt inertia, key-return noise or an identified damper.
Physical string velocity includes the moving endpoint. Its transpose applies
the reciprocal force to the mixed-mass system. Exact dissipative subflow is
composed symmetrically with the unchanged mechanical/acoustic integrator.
"""
import numpy as np
from numpy.polynomial.legendre import leggauss
from concert_grand_polynomial_coast import PolynomialCoast


class StringDamper:
    def __init__(self,core,coefficient=.6,center=.3,width=.08,nodes=16):
        if coefficient<0 or not np.isfinite(coefficient) or not 0<center-width/2<center+width/2<1:
            raise ValueError('nonnegative damper coefficient and interior felt patch')
        x,w=leggauss(nodes);x=center+width*x/2;w=w/2
        shape=np.sin(np.pi*x[:,None]*np.arange(1,core.nt+1))/np.sqrt(core.wire.modal_mass)
        rows=np.zeros((2*nodes,core.size+1))
        rows[:nodes,core.sv]=shape
        rows[:nodes,core.sb]=x[:,None]*core.phi
        rows[nodes:,core.sw]=shape
        # Positive quadrature of a distributed drag, not the outer product
        # of the average patch velocity (which would miss local motion).
        rows[:,:-1]=rows[:,:-1]@core.transform
        rows*=np.sqrt(coefficient*np.tile(w,2))[:,None]
        _,s,vt=np.linalg.svd(rows,full_matrices=False)
        keep=s>max(s.max(initial=0)*1e-12,1e-14)
        self.basis=vt[keep].T;self.eigenvalues=s[keep]**2
        self.rows=rows;self.cache=None
        self.parameters=dict(coefficient_n_s_m=coefficient,center_fraction=center,width_fraction=width,quadrature_nodes=nodes)

    def dissipate(self,velocity,dt,engagement):
        if not np.isfinite(dt) or dt<0 or not np.isfinite(engagement) or not 0<=engagement<=1:raise ValueError('damper step/engagement')
        h=dt*engagement
        if self.cache is None or self.cache[0]!=h:self.cache=(h,np.expm1(-h*self.eigenvalues))
        projected=self.basis.T@velocity;delta=self.cache[1]
        new=velocity+self.basis@(delta*projected)
        heat=-.5*float(np.dot(delta*(2+delta),projected**2))
        return new,heat


class DampedCoast(PolynomialCoast):
    def __init__(self,system,air,damper=None):
        super().__init__(system,air)
        self.damper=damper or StringDamper(system.string)

    def release_step(self,state,dt,engagement,defer_radiation=False):
        # Do not zero any string, soundboard, wave or relaxation state.
        v,a=self.damper.dissipate(state[1],dt/2,engagement)
        middle,stats=super().step((state[0],v,state[2],state[3]),dt,defer_radiation)
        v,b=self.damper.dissipate(middle[1],dt/2,engagement)
        stats['damper_heat_j']=a+b;stats['loss_j']+=a+b
        return (middle[0],v,middle[2],middle[3]),stats


def engagement(time,ramp=.03):
    """Assumed 30ms lowering, with continuous drag and zero endpoint slopes."""
    if ramp<=0:raise ValueError('positive lowering time')
    u=float(np.clip(time/ramp,0,1));return u*u*(3-2*u)
