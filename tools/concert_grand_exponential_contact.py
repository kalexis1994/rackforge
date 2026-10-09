"""Experimental exact quadratic carriers during live multi-wire contact.

AAVF oscillator update (Wang/Wu, arXiv:1807.01044, eq. 5) with exact
discrete string/felt work. The assumed felt rate loss retains its old law.
Symmetric mechanics/air splitting needs its OWN pressure/state convergence
tests; energy balance alone does not approve this for listening or factory.
"""
import numpy as np
from concert_grand_shared_kernels import BatchedCoast
from concert_grand_coupled_contact import felt_divided_force


class ExponentialContact(BatchedCoast):
    def __init__(self, system, air):
        super().__init__(system, air)
        self.directions = np.array([self.project_full(d) for _,d,_ in system.contacts])
        self.stiffness = np.array([k for _,_,k in system.contacts])
        self.contact_functions_cache = None

    def project_full(self, x):
        n = self.system.string.size
        return np.r_[self.project(x[:n]), x[n:]]

    def unproject_full(self, x):
        n = self.system.string.size
        return np.r_[self.unproject(x[:n]), x[n:]]

    def contact_functions(self, dt):
        if self.contact_functions_cache is None or self.contact_functions_cache[0] != dt:
            C,S,Q,W2 = self.oscillator_functions(dt)
            nh = self.system.nh
            self.contact_functions_cache = (dt, np.r_[C,np.ones(nh)],
                np.r_[S,np.full(nh,dt)], np.r_[Q,np.full(nh,dt*dt/2)],
                np.r_[W2,np.zeros(nh)])
        return self.contact_functions_cache[1:]

    def remainder(self, q, a, candidate, dt):
        n = self.system.string.size
        physical = self.unproject(candidate[:n])
        g = np.r_[self.project(self.system.string.nonlinear_avf(q[:n],physical)),
                  np.zeros(self.system.nh)]
        compressions0 = self.directions @ a
        compressions1 = self.directions @ candidate
        forces = [];jacobians = [];heat = 0.
        felt = self.system.felt
        for d0,d1,k in zip(compressions0,compressions1,self.stiffness):
            # The -inf intermediate at a zero ratio has a finite expm1 limit.
            with np.errstate(divide='ignore'):
                elastic,jac = felt_divided_force(float(d0),float(d1),k,felt.exponent)
            argument = felt.rate_damping_s_per_m*(d1-d0)/dt
            factor = np.exp(np.clip(argument,-40,40))
            force = elastic*factor
            forces.append(force)
            jacobians.append(factor*(jac+(elastic*felt.rate_damping_s_per_m/dt
                                           if -40<argument<40 else 0.)))
            heat += (force-elastic)*(d1-d0)
        g += self.directions.T @ np.asarray(forces)
        return g,np.asarray(jacobians),dict(felt_loss_j=float(heat),force_n=float(sum(forces)))

    def mechanical_step(self, q, v, dt):
        if not np.isfinite(dt) or dt <= 0:
            raise ValueError('positive mechanical contact step')
        C,S,Q,W2 = self.contact_functions(dt)
        a,p = self.project_full(q),self.project_full(v)
        linear = C*a+S*p
        candidate = linear.copy()
        scale = max(np.linalg.norm(a),np.linalg.norm(S*p),1e-12)
        accepted = None
        for iteration in range(40):
            g,jac,stats = self.remainder(q,a,candidate,dt) if accepted is None else accepted
            accepted = None
            residual = candidate-linear+Q*g
            norm = np.linalg.norm(residual)
            if norm < 2e-14*scale:
                break
            # Only a preconditioner: full string/felt residual is still solved.
            # Woodbury retains all nine contacts, including unequal compressions.
            W = np.sqrt(np.maximum(jac,0.))[:,None]*self.directions
            matrix = np.eye(len(W))+(W*Q)@W.T
            update = residual-Q*(W.T@np.linalg.solve(matrix,W@residual))
            for power in range(14):
                trial = candidate-update*2.**(-power)
                tg,tj,ts = self.remainder(q,a,trial,dt)
                if np.linalg.norm(trial-linear+Q*tg) < norm:
                    candidate = trial;accepted = (tg,tj,ts);break
            else:
                raise RuntimeError('exponential contact line search; refine dt')
        else:
            raise RuntimeError('exponential contact residual; refine dt')
        new_q = self.unproject_full(candidate)
        new_v = self.unproject_full(-W2*S*a+C*p-S*g)
        stats.update(mechanical_iterations=iteration+1,full_tangent_fallbacks=0)
        return new_q,new_v,stats

    def step(self, state, dt, defer_radiation=False):
        if not np.isfinite(dt) or dt <= 0:
            raise ValueError('positive contact/air step')
        q,v,a = self.mechanical_step(state[0],state[1],dt/2)
        middle,stats = self.air_step((q,v,state[2],state[3]),dt,defer_radiation)
        q,v,b = self.mechanical_step(middle[0],middle[1],dt/2)
        stats['felt_loss_j'] = a['felt_loss_j']+b['felt_loss_j']
        stats['loss_j'] += stats['felt_loss_j']
        stats['force_n'] = max(a['force_n'],b['force_n'])
        stats['mechanical_iterations'] = max(a['mechanical_iterations'],b['mechanical_iterations'])
        stats['full_tangent_fallbacks'] = 0
        return (q,v,middle[2],middle[3]),stats
