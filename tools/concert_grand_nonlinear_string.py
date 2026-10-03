"""SI modal extensible string with two transverse directions and axial motion.

Small-slope strain u_x + (v_x**2+w_x**2)/2, fixed ends, positive mass.
The same potential drives both sides of T/L exchange. This is a standalone
Kirchhoff pilot, not the phasor engine, Timoshenko string or a fitted piano.
Average-vector-field integration uses exact two-point Gauss integration of
the cubic potential gradient, with an implicit Newton solve.
"""
from dataclasses import dataclass

import numpy as np
from numpy.polynomial.legendre import leggauss


@dataclass(frozen=True)
class Wire:
    length: float
    linear_density: float
    tension: float = 700.
    young: float = 200e9
    density: float = 7850.

    @property
    def area(self):return self.linear_density/self.density

    @property
    def axial_rigidity(self):return self.young*self.area

    @property
    def modal_mass(self):return self.linear_density*self.length/2


class String:
    def __init__(self,wire,transverse=12,axial=2,quadrature=96,bending=False):
        self.wire=wire;self.nt=transverse;self.nl=axial
        self.size=axial+2*transverse
        self.su=slice(0,axial);self.sv=slice(axial,axial+transverse);self.sw=slice(axial+transverse,None)
        nodes,weight=leggauss(quadrature)
        self.x=(nodes+1)*wire.length/2;self.weight=weight*wire.length/2
        ku=np.pi*np.arange(1,axial+1)/wire.length
        kt=np.pi*np.arange(1,transverse+1)/wire.length
        root=np.sqrt(wire.modal_mass)
        self.du=np.cos(self.x[:,None]*ku)*ku/root
        self.dt=np.cos(self.x[:,None]*kt)*kt/root
        self.kt=wire.tension*(self.dt.T@(self.weight[:,None]*self.dt))
        if bending:
            curvature=-np.sin(self.x[:,None]*kt)*kt**2/root
            inertia=wire.area**2/(4*np.pi)
            self.kt+=wire.young*inertia*(curvature.T@(self.weight[:,None]*curvature))

    def potential(self,q):
        ux=self.du@q[self.su];vx=self.dt@q[self.sv];wx=self.dt@q[self.sw]
        strain=ux+.5*(vx*vx+wx*wx)
        return float(.5*(q[self.sv]@self.kt@q[self.sv]+q[self.sw]@self.kt@q[self.sw])
                     +.5*self.wire.axial_rigidity*np.dot(self.weight,strain*strain))

    def gradient_hessian(self,q,hessian=True):
        ux=self.du@q[self.su];vx=self.dt@q[self.sv];wx=self.dt@q[self.sw]
        strain=ux+.5*(vx*vx+wx*wx)
        ea=self.wire.axial_rigidity
        g=np.empty(self.size)
        g[self.su]=ea*self.du.T@(self.weight*strain)
        g[self.sv]=self.kt@q[self.sv]+ea*self.dt.T@(self.weight*strain*vx)
        g[self.sw]=self.kt@q[self.sw]+ea*self.dt.T@(self.weight*strain*wx)
        if not hessian:return g
        gram=lambda a,z,b:ea*a.T@((self.weight*z)[:,None]*b)
        h=np.empty((self.size,self.size))
        h[self.su,self.su]=gram(self.du,np.ones(len(vx)),self.du)
        h[self.su,self.sv]=gram(self.du,vx,self.dt)
        h[self.su,self.sw]=gram(self.du,wx,self.dt)
        h[self.sv,self.su]=h[self.su,self.sv].T
        h[self.sw,self.su]=h[self.su,self.sw].T
        h[self.sv,self.sv]=self.kt+gram(self.dt,strain+vx*vx,self.dt)
        h[self.sw,self.sw]=self.kt+gram(self.dt,strain+wx*wx,self.dt)
        h[self.sv,self.sw]=gram(self.dt,vx*wx,self.dt)
        h[self.sw,self.sv]=h[self.sv,self.sw].T
        return g,h

    def energy(self,q,p):return .5*float(p@p)+self.potential(q)

    def components(self,q,p):
        axial=.5*float(p[self.su]@p[self.su])
        ux=self.du@q[self.su]
        axial+=.5*self.wire.axial_rigidity*np.dot(self.weight,ux*ux)
        transverse=.5*float(p[self.sv]@p[self.sv]+p[self.sw]@p[self.sw])
        transverse+=.5*float(q[self.sv]@self.kt@q[self.sv]+q[self.sw]@self.kt@q[self.sw])
        return dict(axial_linear_j=float(axial),transverse_linear_j=float(transverse),
                    nonlinear_interaction_j=float(self.energy(q,p)-axial-transverse),total_j=self.energy(q,p))

    def avf(self,a,b):
        delta=b-a
        c1=.5-.5/np.sqrt(3);c2=.5+.5/np.sqrt(3)
        g1,h1=self.gradient_hessian(a+c1*delta)
        g2,h2=self.gradient_hessian(a+c2*delta)
        return .5*(g1+g2),.5*(c1*h1+c2*h2)

    def step(self,q,p,dt,damping=0.):
        sigma=np.broadcast_to(damping,q.shape)
        if (sigma<0).any():raise ValueError('negative damping')
        candidate=q+dt*p
        diagonal=np.eye(self.size)*(1+dt*sigma)
        for iteration in range(16):
            g,jac=self.avf(q,candidate)
            residual=(1+dt*sigma)*(candidate-q)-dt*p+.5*dt*dt*g
            scale=max(np.linalg.norm(candidate),np.linalg.norm(dt*p),1e-12)
            if np.linalg.norm(residual)<2e-14*scale:break
            candidate-=np.linalg.solve(diagonal+.5*dt*dt*jac,residual)
        else:raise RuntimeError('nonlinear step did not converge')
        # Use the momentum equation instead of subtracting close q samples;
        # the latter loses precision under temporal refinement.
        next_p=((1-dt*sigma)*p-dt*g)/(1+dt*sigma)
        # Exact work of the centred positive velocity damping during this step.
        dissipated=dt*np.dot(2*sigma,((p+next_p)/2)**2)
        return candidate,next_p,float(dissipated),iteration+1
