"""Scalable quadratic splitting of the SAME shared mechanical Hamiltonian.

Every fixed-end wire oscillator and the 606-board block are exact carriers.
The reciprocal quadratic wire/board coupling belongs to the AVF remainder;
its implicit part is eliminated by a board-sized Schur complement. No mode,
coupling or nonlinear strain term is dropped. Dispersion versus the global
exact quadratic carrier MUST be measured before using this for audition.
"""
import numpy as np
from scipy.linalg import eigh,cho_factor,cho_solve
from concert_grand_bounded_contact import BoundedContact
from concert_grand_shared_strings import SharedCoast


class PortCoast(SharedCoast):
    def __init__(self,system,air):
        BoundedContact.__init__(self,system,air)
        core=system.string
        physical_board=np.diag(core.omega**2)
        for e in core.entries:
            wire=e['port'].wire
            physical_board+=(wire.tension/wire.length+np.dot(e['cross']**2,np.diag(e['base'].kt)))*np.outer(e['phi'],e['phi'])
        board_K=core.board_transform.T@physical_board@core.board_transform
        values,self.board_basis=eigh(board_K)
        if values.min()<=0:raise ValueError('positive board carrier')
        self.omega2=np.empty(core.size);self.omega2[core.sb]=values
        self.couplings=[]
        for e in core.entries:
            base,wire=e['base'],e['port'].wire
            self.omega2[e['su']]=wire.axial_rigidity*wire.length/2*base.ku**2
            self.omega2[e['sv']]=np.diag(base.kt)
            self.omega2[e['sw']]=np.diag(base.kt)
            port=e['phi']@core.board_transform@self.board_basis
            weights=-np.diag(base.kt)*e['cross']
            self.couplings.append((e['sv'],weights,port))
        self.Cb=self.C[:,core.sb];self.Db=system.damping[core.sb,core.sb]
        self.pade=True;self.residue_matrix=None
        self.mechanical_cache=self.air_cache=self.port_cache=None

    def project(self,x):
        y=x.copy();sb=self.system.string.sb;y[sb]=self.board_basis.T@x[sb];return y

    def unproject(self,x):
        y=x.copy();sb=self.system.string.sb;y[sb]=self.board_basis@x[sb];return y

    def oscillator_functions(self,dt):
        if self.mechanical_cache is None or self.mechanical_cache[0]!=dt:
            omega=np.sqrt(self.omega2);phase=omega*dt
            C=np.cos(phase);S=dt*np.sinc(phase/np.pi)
            Q=.5*dt*dt*np.sinc(phase/(2*np.pi))**2
            self.mechanical_cache=(dt,C,S,Q,self.omega2)
        return self.mechanical_cache[1:]

    def coupling_gradient(self,x):
        result=np.zeros_like(x);sb=self.system.string.sb
        for indices,weights,port in self.couplings:
            result[indices]+=weights*(port@x[sb])
            result[sb]+=port*(weights@x[indices])
        return result

    def prepare_schur(self,dt,Q):
        if self.port_cache is None or self.port_cache[0]!=dt:
            core=self.system.string
            J=np.zeros((core.nb,core.nb))
            for indices,weights,port in self.couplings:
                J+=np.dot(Q[indices]/2,weights*weights)*np.outer(port,port)
            root=np.sqrt(Q[core.sb]/2)
            matrix=np.eye(core.nb)-root[:,None]*J*root[None,:]
            self.port_cache=(dt,J,root,cho_factor(matrix))
        return self.port_cache[1:]

    def solve_remainder(self,rhs,dt,Q):
        core=self.system.string;sb=core.sb
        J,root,factor=self.prepare_schur(dt,Q)
        board=rhs[sb].copy()
        for indices,weights,port in self.couplings:
            board-=Q[sb]/2*port*(weights@rhs[indices])
        # Woodbury form avoids division by Q at exact carrier resonances.
        board+=root*cho_solve(factor,root*(J@board),check_finite=False)
        result=rhs.copy();result[sb]=board
        for indices,weights,port in self.couplings:
            result[indices]-=Q[indices]/2*weights*(port@board)
        return result

    def mechanical_step(self,q,v,dt):
        if dt<=0 or self.system.compression(q)>1e-12:
            raise ValueError('separated heads and positive dt required')
        n=self.system.string.size
        C,S,Q,W2=self.oscillator_functions(dt)
        a,p=self.project(q[:n]),self.project(v[:n])
        linear=C*a+S*p
        candidate=self.solve_remainder(linear-Q*self.coupling_gradient(a)/2,dt,Q)
        scale=max(np.linalg.norm(a),np.linalg.norm(S*p),1e-12)
        for iteration in range(40):
            physical=self.unproject(candidate)
            nonlinear=self.project(self.system.string.nonlinear_avf(q[:n],physical))
            gradient=self.coupling_gradient((a+candidate)/2)+nonlinear
            residual=candidate-linear+Q*gradient
            if np.linalg.norm(residual)<2e-14*scale:break
            candidate-=self.solve_remainder(residual,dt,Q)
        else:raise RuntimeError('port nonlinear AVF did not converge; refine dt')
        next_q=np.r_[physical,q[n:]+dt*v[n:]]
        next_v=np.r_[self.unproject(-W2*S*a+C*p-S*gradient),v[n:]]
        if self.system.compression(next_q)>1e-12:
            raise ValueError('shared head recontact needs contact solver')
        return next_q,next_v,dict(mechanical_iterations=iteration+1,full_tangent_fallbacks=0)
