"""Same AAVF discrete gradient integrated analytically as a polynomial.

The strain remainder is cubic/quartic. Its segment integral can be evaluated
from the two endpoint fields, rather than synthesizing both Gauss fields on
every nonlinear iteration. No approximation or physical coefficient change.
"""
import numpy as np
from concert_grand_exponential_coast import ExponentialCoast


class PolynomialCoast(ExponentialCoast):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        self.endpoint_fields=None

    def mechanical_step(self,q,v,dt):
        previous=self.endpoint_fields
        self.endpoint_fields=self.system.string.fields(q[:-1])
        try:return super().mechanical_step(q,v,dt)
        finally:self.endpoint_fields=previous

    def nonlinear_avf(self,a,b):
        core=self.system.string;base=core.base
        if not core.nonlinear:return np.zeros_like(a)
        first=core.fields(a) if self.endpoint_fields is None else self.endpoint_fields
        _,_,u0,v0,w0,_=first
        _,_,u1,v1,w1,_=core.fields(b)
        eta=(v0*v0+v0*v1+v1*v1+w0*w0+w0*w1+w1*w1)/6
        product=lambda x0,x1,y0,y1:(2*x0*y0+x0*y1+x1*y0+2*x1*y1)/6
        square_product=lambda x0,x1,y0,y1:(x0*x0*(3*y0+y1)+2*x0*x1*(y0+y1)+x1*x1*(y0+3*y1))/12
        strain_v=product(u0,u1,v0,v1)+(v0+v1)*(v0*v0+v1*v1)/8+square_product(w0,w1,v0,v1)/2
        strain_w=product(u0,u1,w0,w1)+(w0+w1)*(w0*w0+w1*w1)/8+square_product(v0,v1,w0,w1)/2
        ea=core.wire.axial_rigidity;g=np.zeros_like(a)
        g[core.su]=ea*base.du.T@(base.weight*eta)
        g[core.sv]=ea*base.dt.T@(base.weight*strain_v)
        g[core.sw]=ea*base.dt.T@(base.weight*strain_w)
        g[core.sb]=core.phi*ea/core.wire.length*np.dot(base.weight,strain_v)
        return core.pull_gradient(g)
