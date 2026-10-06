"""Offline acceleration of the existing AVF/midpoint contact equations.

No modal truncation, new damping, linearized string or reset of acoustic
memory. The fixed tangent is only a nonlinear-solve preconditioner; the
residual still uses the full nonlinear potential at both Gauss points.
Reference scalar air/contact implementations remain unchanged.
"""
import numpy as np
from scipy.linalg import cho_factor, cho_solve
from scipy.signal import lfilter
from concert_grand_bounded_contact import BoundedContact
from concert_grand_coupled_contact import felt_divided_force
from concert_grand_pade_air import PadeAir
from concert_grand_pade_midpoint import PadeMidpoint


class FastBoundedContact(BoundedContact):
    def __init__(self, system, air, coupling=1.):
        super().__init__(system, air, coupling)
        self.fast_cache = None
        # Contract all residues in one BLAS operation, in the same port order.
        self.pade = isinstance(air.wave, PadeAir)
        self.residue_matrix = (None if self.pade else np.ascontiguousarray(
            air.wave.R[1:].transpose(1, 0, 2)).reshape(air.modes, -1))

    def avf_gradient(self, a, b, dt):
        delta = b - a
        c1, c2 = .5 - .5 / np.sqrt(3), .5 + .5 / np.sqrt(3)
        core = self.system.string
        g = np.r_[.5 * (core.gradient_hessian((a+c1*delta)[:-1], False)
                       + core.gradient_hessian((a+c2*delta)[:-1], False)), 0.]
        d0, d1 = self.system.compression(a), self.system.compression(b)
        f = self.system.felt
        elastic, _ = felt_divided_force(d0, d1, f.stiffness, f.exponent)
        force = elastic * np.exp(np.clip(f.rate_damping_s_per_m*(d1-d0)/dt, -40, 40))
        g += self.system.direction*force
        return g, dict(force_n=float(force), felt_loss_j=float((force-elastic)*(d1-d0)),
                       compression_start_m=d0, compression_end_m=d1)

    def _prepare(self, dt):
        if self.fast_cache is not None and self.fast_cache['dt'] == dt:
            return self.fast_cache
        h = dt/2
        A = self.air.impedance(2/dt).real
        plus = np.eye(self.n)+h*self.system.damping+h*self.C.T@A@self.C
        H = np.zeros((self.n, self.n))
        H[:-1, :-1] = self.system.string.gradient_hessian(np.zeros(self.n-1))[1]
        wave_plan = PadeMidpoint(self.air.wave, dt) if self.pade else None
        alpha = None if self.pade else dt*self.air.wave.a/2
        beta = None if self.pade else (alpha-1)/(alpha+1)
        self.fast_cache = dict(dt=dt, A=A, plus=plus,
            # The AVF gradient's endpoint tangent of a quadratic potential
            # is H/2. The residual already multiplies that tangent by dt²/2.
            tangent=cho_factor(plus+.25*dt*dt*H),
            velocity_factor=cho_factor(np.eye(self.n)+h*self.system.damping),
            minus=np.eye(self.n)-h*self.system.damping,
            wave_plan=wave_plan, alpha=alpha, beta=beta,
            powers=None if self.pade else beta**np.arange(1,self.air.wave.order+1),
            relaxation_alpha=dt/(2*self.air.relaxation.tau))
        return self.fast_cache

    def step(self, state, dt, defer_radiation=False):
        if not np.isfinite(dt) or dt <= 0:
            raise ValueError('positive step')
        q, v, r, z = state
        cache = self._prepare(dt)
        # Eliminate the living wave states before the nonlinear solve.
        # Each allpass plan has its own recurrence and the same Schur port.
        if self.pade:
            constant, offsets = cache['wave_plan'].constant(r)
        else:
            offsets = lfilter([2/(1+cache['alpha'])], [1,-cache['beta']], r, axis=0)
            constant = self.residue_matrix@offsets.ravel()
        constant -= self.air.relaxation.B@(z/(1+cache['relaxation_alpha']))
        external = self.C.T@constant
        candidate = q+dt*v
        scale = max(np.linalg.norm(candidate),np.linalg.norm(dt*v),1e-12)
        fallback = 0
        accepted_gradient = None
        for iteration in range(24):
            if accepted_gradient is None:
                g, stats = self.avf_gradient(q,candidate,dt)
            else:
                g, stats = accepted_gradient
                accepted_gradient = None
            residual = cache['plus']@(candidate-q)-dt*v+.5*dt*dt*(g+external)
            norm = np.linalg.norm(residual)
            if norm < 2e-14*scale:
                break
            if iteration < 8:
                update = cho_solve(cache['tangent'],residual)
            else:
                _, H, _ = self.system.avf(q,candidate,dt)
                update = np.linalg.solve(cache['plus']+.5*dt*dt*H,residual)
                fallback += 1
            for power in range(12):
                trial = candidate-update*2.**(-power)
                tg, trial_stats = self.avf_gradient(q,trial,dt)
                test = cache['plus']@(trial-q)-dt*v+.5*dt*dt*(tg+external)
                if np.linalg.norm(test) < norm:
                    candidate = trial
                    accepted_gradient = (tg,trial_stats)
                    break
            else:
                raise RuntimeError('fast contact line search')
        else:
            raise RuntimeError('fast contact residual did not converge')
        center = (candidate-q)/dt
        boundary = self.C@center
        if self.pade:
            new_r, inputs = cache['wave_plan'].update(r, boundary, offsets)
        else:
            inputs = np.empty((self.air.wave.order+1,self.air.modes))
            inputs[0] = boundary
            inputs[1:] = offsets+cache['powers'][:,None]*boundary
            new_r = inputs[1:]+inputs[:-1]-r
        new_z, _, relax_power = self.air.relaxation.midpoint(z,boundary,dt)
        load = cache['A']@boundary+constant
        next_v = cho_solve(cache['velocity_factor'], cache['minus']@v-dt*(g+self.C.T@load))
        if defer_radiation:
            radiated = relax_power
            stats['wave_inputs'] = inputs
        else:
            radiated = relax_power+self.air.wave.radiation_batch(inputs[None])[0]
        stats.update(material_loss_j=dt*float(center@self.system.damping@center),
                     radiation_loss_j=dt*float(radiated), air_port_work_j=dt*float(boundary@load),
                     newton_iterations=iteration+1, full_tangent_fallbacks=fallback)
        stats['loss_j'] = stats['felt_loss_j']+stats['material_loss_j']+stats['radiation_loss_j']
        return (candidate,next_v,new_r,new_z), stats
