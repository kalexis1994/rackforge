"""Reciprocal SI string choir on ONE moving soundboard and acoustic port.

The existing extensible two-polarization string potential is summed, while
board mass, stiffness, material damping and air are included exactly once.
Endpoint lifting retains mixed inertia. Each key has one hammer and a
separate one-sided felt contact per wire. Parameters remain assumed, not
identified piano regulation. No samples, output envelope or artificial tail.
"""
from dataclasses import dataclass
import numpy as np
from scipy.fft import dct
from scipy.linalg import cholesky, solve_triangular, eigh, cho_factor, cho_solve
from numpy.polynomial.legendre import leggauss
from concert_grand_contact_study import CosineString
from concert_grand_coupled_contact import felt_divided_force
from concert_grand_moving_bridge import contact_footprint
from concert_grand_bounded_contact import BoundedContact
from concert_grand_fast_contact import FastBoundedContact
from concert_grand_exponential_coast import ExponentialCoast
from concert_grand_audition_air import MappedPlan, MappedWave
from concert_grand_pade_midpoint import PadeMidpoint


@dataclass
class WirePort:
    key: int
    wire: object
    projection: object


class SharedStrings:
    def __init__(self, ports, frequency, damping, transverse=96, axial=192,
                 nonlinear=True):
        self.ports = list(ports)
        self.nb = len(frequency)
        self.omega = 2*np.pi*np.asarray(frequency)
        self.board_damping = np.asarray(damping, float)
        if not self.ports or self.board_damping.shape != (self.nb, self.nb):
            raise ValueError('shared board dimensions')
        if not np.allclose(self.board_damping, self.board_damping.T, atol=1e-12):
            raise ValueError('reciprocal board damping')
        if np.linalg.eigvalsh(self.board_damping).min() < -1e-12:
            raise ValueError('passive material damping')
        self.nonlinear = nonlinear
        self.entries = []
        self.ns = len(ports)*(axial+2*transverse)
        self.size = self.ns+self.nb
        self.sb = slice(self.ns, self.size)
        cross = np.zeros((self.ns, self.nb))
        schur = np.eye(self.nb)
        self.mass = np.eye(self.size)
        physical_K = np.zeros_like(self.mass)
        physical_K[self.sb, self.sb] = np.diag(self.omega**2)
        self.groups = []
        vertical = []
        for i, port in enumerate(ports):
            base = CosineString(port.wire, transverse, axial)
            start = i*base.size
            su = slice(start, start+axial)
            sv = slice(su.stop, su.stop+transverse)
            sw = slice(sv.stop, sv.stop+transverse)
            phi = np.asarray(port.projection, float)
            if phi.shape != (self.nb,) or not np.isfinite(phi).all():
                raise ValueError('wire-to-board port')
            n = np.arange(1, transverse+1)
            cv = port.wire.linear_density*port.wire.length*(-1.)**(n+1)/(n*np.pi*np.sqrt(port.wire.modal_mass))
            lift = port.wire.linear_density*port.wire.length/3
            cross[sv] = cv[:, None]*phi
            outer = np.outer(phi, phi)
            schur += (lift-cv@cv)*outer
            self.mass[self.sb, self.sb] += lift*outer
            physical_K[su, su] = np.diag(port.wire.axial_rigidity*port.wire.length/2*base.ku**2)
            physical_K[sv, sv] = base.kt
            physical_K[sw, sw] = base.kt
            physical_K[self.sb, self.sb] += port.wire.tension/port.wire.length*outer
            self.entries.append(dict(port=port, base=base, su=su, sv=sv, sw=sw,
                                     phi=phi, cross=cv, lift_mass=lift))
            self.groups.extend([np.arange(su.start, su.stop), np.arange(sw.start, sw.stop)])
            vertical.extend(range(sv.start, sv.stop))
        self.groups.append(np.r_[vertical, np.arange(self.ns, self.size)])
        self.cross = cross
        self.mass[:self.ns, self.sb] = cross
        self.mass[self.sb, :self.ns] = cross.T
        factor = cholesky(schur, lower=True)
        self.board_transform = solve_triangular(factor.T, np.eye(self.nb), lower=False)
        self.transform = np.eye(self.size)
        self.transform[:self.ns, self.sb] = -cross@self.board_transform
        self.transform[self.sb, self.sb] = self.board_transform
        self.K = self.transform.T@physical_K@self.transform
        self.K = (self.K+self.K.T)/2
        self.damping = np.zeros_like(self.K)
        self.damping[self.sb, self.sb] = self.board_transform.T@self.board_damping@self.board_transform

    def physical(self, q):
        b = self.board_transform@q[self.sb]
        z = q.copy()
        z[self.sb] = b
        for e in self.entries:
            z[e['sv']] -= e['cross']*(e['phi']@b)
        return z

    def pull_gradient(self, g):
        result = g.copy()
        board = g[self.sb].copy()
        for e in self.entries:
            board -= e['phi']*(e['cross']@g[e['sv']])
        result[self.sb] = self.board_transform.T@board
        return result

    def linear_gradient(self, q):
        z=self.physical(q);g=np.zeros_like(q)
        g[self.sb]=self.omega**2*z[self.sb]
        for e in self.entries:
            wire,base=e['port'].wire,e['base']
            g[e['su']]=wire.axial_rigidity*wire.length/2*base.ku**2*z[e['su']]
            g[e['sv']]=np.diag(base.kt)*z[e['sv']]
            g[e['sw']]=np.diag(base.kt)*z[e['sw']]
            g[self.sb]+=e['phi']*wire.tension/wire.length*(e['phi']@z[self.sb])
        return self.pull_gradient(g)

    def fields(self, z, e):
        base = e['base']
        b = float(e['phi']@z[self.sb])
        ux = base.synthesize(base.ku*z[e['su']])
        vx = base.synthesize(base.kv*z[e['sv']])+b/e['port'].wire.length
        wx = base.synthesize(base.kv*z[e['sw']])
        return b, ux, vx, wx

    def potential(self, q):
        z = self.physical(q)
        energy = .5*float(np.dot(self.omega*z[self.sb], self.omega*z[self.sb]))
        for e in self.entries:
            base, wire = e['base'], e['port'].wire
            b, ux, vx, wx = self.fields(z, e)
            strain = ux+.5*(vx*vx+wx*wx) if self.nonlinear else ux
            energy += .5*float(z[e['sv']]@base.kt@z[e['sv']]+z[e['sw']]@base.kt@z[e['sw']])
            energy += .5*wire.tension/wire.length*b*b+.5*wire.axial_rigidity*np.dot(base.weight, strain*strain)
        return float(energy)

    def gradient_hessian(self, q, hessian=True):
        z = self.physical(q)
        g = np.zeros_like(q)
        g[self.sb] = self.omega**2*z[self.sb]
        H = np.zeros_like(self.K) if hessian else None
        if hessian:
            H[self.sb, self.sb] = np.diag(self.omega**2)
        for e in self.entries:
            base, wire = e['base'], e['port'].wire
            su, sv, sw, phi = e['su'], e['sv'], e['sw'], e['phi']
            b, ux, vx, wx = self.fields(z, e)
            strain = ux+.5*(vx*vx+wx*wx) if self.nonlinear else ux
            factor = wire.axial_rigidity*wire.length/(2*base.nodes)
            g[su] = factor*base.ku*dct(strain, type=2)[1:base.nl+1]
            g[sv] = np.diag(base.kt)*z[sv]
            g[sw] = np.diag(base.kt)*z[sw]
            g[self.sb] += phi*wire.tension/wire.length*b
            if self.nonlinear:
                g[sv] += factor*base.kv*dct(strain*vx, type=2)[1:base.nt+1]
                g[sw] += factor*base.kv*dct(strain*wx, type=2)[1:base.nt+1]
                g[self.sb] += phi*wire.axial_rigidity/wire.length*np.dot(base.weight, strain*vx)
            if hessian:
                H[sv, sv] += base.kt; H[sw, sw] += base.kt
                H[self.sb, self.sb] += wire.tension/wire.length*np.outer(phi, phi)
                J = np.zeros((base.nodes, self.size)); J[:, su] = base.du
                if self.nonlinear:
                    J[:, sv] = vx[:, None]*base.dt; J[:, sw] = wx[:, None]*base.dt
                    J[:, self.sb] = vx[:, None]*phi/wire.length
                H += wire.axial_rigidity*J.T@(base.weight[:, None]*J)
                if self.nonlinear:
                    gram = wire.axial_rigidity*base.dt.T@((base.weight*strain)[:, None]*base.dt)
                    H[sv, sv] += gram; H[sw, sw] += gram
                    mixed = wire.axial_rigidity*(base.dt.T@(base.weight*strain))[:, None]*phi/wire.length
                    H[sv, self.sb] += mixed; H[self.sb, sv] += mixed.T
                    H[self.sb, self.sb] += wire.axial_rigidity*np.dot(base.weight, strain)*np.outer(phi, phi)/wire.length**2
        gn = self.pull_gradient(g)
        return (gn, self.transform.T@H@self.transform) if hessian else gn

    def nonlinear_avf(self, a, b):
        if not self.nonlinear:
            return np.zeros_like(a)
        z0, z1 = self.physical(a), self.physical(b)
        g = np.zeros_like(a)
        product = lambda x0,x1,y0,y1: (2*x0*y0+x0*y1+x1*y0+2*x1*y1)/6
        square_product = lambda x0,x1,y0,y1: (x0*x0*(3*y0+y1)+2*x0*x1*(y0+y1)+x1*x1*(y0+3*y1))/12
        for e in self.entries:
            base, wire = e['base'], e['port'].wire
            _, u0,v0,w0 = self.fields(z0, e); _, u1,v1,w1 = self.fields(z1, e)
            eta = (v0*v0+v0*v1+v1*v1+w0*w0+w0*w1+w1*w1)/6
            pv = product(u0,u1,v0,v1)+(v0+v1)*(v0*v0+v1*v1)/8+square_product(w0,w1,v0,v1)/2
            pw = product(u0,u1,w0,w1)+(w0+w1)*(w0*w0+w1*w1)/8+square_product(v0,v1,w0,w1)/2
            factor = wire.axial_rigidity*wire.length/(2*base.nodes)
            g[e['su']] = factor*base.ku*dct(eta, type=2)[1:base.nl+1]
            g[e['sv']] = factor*base.kv*dct(pv, type=2)[1:base.nt+1]
            g[e['sw']] = factor*base.kv*dct(pw, type=2)[1:base.nt+1]
            g[self.sb] += e['phi']*wire.axial_rigidity/wire.length*np.dot(base.weight, pv)
        return self.pull_gradient(g)

    def avf(self, a, b):
        c = .5/np.sqrt(3); delta = b-a
        g0,H0 = self.gradient_hessian(a+(.5-c)*delta)
        g1,H1 = self.gradient_hessian(a+(.5+c)*delta)
        return .5*(g0+g1), .5*((.5-c)*H0+(.5+c)*H1)


class SharedHammers:
    def __init__(self, core, felt):
        self.string, self.felt = core, felt
        self.keys = sorted(set(p.key for p in core.ports))
        self.nh = len(self.keys); self.size = core.size+self.nh
        self.contacts = []
        for e in core.entries:
            base, wire, key = e['base'], e['port'].wire, e['port'].key
            shape, lift = contact_footprint(base.nt, felt.strike_fraction, felt.width_fraction)
            physical = np.zeros(core.size)
            physical[e['sv']] = -shape/np.sqrt(wire.modal_mass)
            physical[core.sb] = -lift*e['phi']
            d = np.zeros(self.size); d[:core.size] = core.transform.T@physical
            d[core.size+self.keys.index(key)] = 1/np.sqrt(felt.hammer_mass)
            count = sum(p.key == key for p in core.ports)
            self.contacts.append((key, d, felt.stiffness/count))
        self.damping = np.zeros((self.size, self.size))
        self.damping[:core.size, :core.size] = core.damping

    def compressions(self, q):
        return np.array([d@q for _,d,_ in self.contacts])

    def compression(self, q):
        return float(self.compressions(q).max())

    def energy(self, q, v):
        felt = sum(k/(self.felt.exponent+1)*max(float(d@q),0.)**(self.felt.exponent+1) for _,d,k in self.contacts)
        return .5*float(v@v)+self.string.potential(q[:self.string.size])+felt

    def initial_state(self, speed=0.):
        if speed != 0:
            raise ValueError('launch a specific key into the live shared state')
        q = np.zeros(self.size); q[self.string.size:] = -.01*np.sqrt(self.felt.hammer_mass)
        return q, np.zeros(self.size)

    def launch(self, state, key, speed):
        if key not in self.keys or not np.isfinite(speed) or speed < 0:
            raise ValueError('valid key and incident speed')
        q, v, r, z = state
        contacts = [(d,k) for kk,d,k in self.contacts if kk == key]
        if max(d@q for d,_ in contacts) > 1e-12:
            raise ValueError('cannot relaunch a contacting hammer')
        n = self.string.size; head = n+self.keys.index(key)
        new_q, new_v = q.copy(), v.copy()
        new_q[head] = min(-float(d[:n]@q[:n])/d[head] for d,_ in contacts)
        new_v[head] = np.sqrt(self.felt.hammer_mass)*speed
        # Everything except this separated head remains live and unchanged.
        work = self.energy(new_q,new_v)-self.energy(q,v)
        return (new_q,new_v,r,z), float(work)

    def avf_gradient(self, a, b, dt):
        n = self.string.size
        g = np.r_[self.string.linear_gradient((a[:n]+b[:n])/2)+self.string.nonlinear_avf(a[:n],b[:n]), np.zeros(self.nh)]
        heat = force_sum = 0.
        for _,d,k in self.contacts:
            d0,d1 = float(d@a),float(d@b)
            elastic,_ = felt_divided_force(d0,d1,k,self.felt.exponent)
            force = elastic*np.exp(np.clip(self.felt.rate_damping_s_per_m*(d1-d0)/dt,-40,40))
            g += d*force; heat += (force-elastic)*(d1-d0); force_sum += force
        return g, dict(force_n=float(force_sum), felt_loss_j=float(heat))

    def avf(self, a, b, dt):
        n = self.string.size
        _,Hs = self.string.avf(a[:n],b[:n])
        g,stats = self.avf_gradient(a,b,dt)
        H = np.zeros((self.size,self.size)); H[:n,:n] = Hs
        for _,d,k in self.contacts:
            d0,d1 = float(d@a),float(d@b)
            elastic,jac = felt_divided_force(d0,d1,k,self.felt.exponent)
            argument = self.felt.rate_damping_s_per_m*(d1-d0)/dt
            factor = np.exp(np.clip(argument,-40,40))
            jac = factor*(jac+(elastic*self.felt.rate_damping_s_per_m/dt if -40<argument<40 else 0))
            H += jac*np.outer(d,d)
        return g,H,stats


class SharedContactReference(FastBoundedContact):
    def avf_gradient(self, a,b,dt):
        return self.system.avf_gradient(a,b,dt)

    def _prepare(self, dt):
        if self.fast_cache is not None and self.fast_cache['dt'] == dt:
            return self.fast_cache
        h = dt/2; A = self.air.impedance(2/dt).real
        plus = np.eye(self.n)+h*self.system.damping+h*self.C.T@A@self.C
        H = np.zeros((self.n,self.n)); size = self.system.string.size
        H[:size,:size] = self.system.string.K
        wave_plan = (MappedPlan(self.air.wave,dt) if isinstance(self.air.wave,MappedWave)
                     else PadeMidpoint(self.air.wave,dt))
        self.fast_cache = dict(dt=dt,A=A,plus=plus,tangent=cho_factor(plus+.25*dt*dt*H),
            velocity_factor=cho_factor(np.eye(self.n)+h*self.system.damping),
            minus=np.eye(self.n)-h*self.system.damping,wave_plan=wave_plan,
            relaxation_alpha=dt/(2*self.air.relaxation.tau))
        return self.fast_cache


class SharedContact(SharedContactReference):
    """Exact invariant-block elimination of the dense contact preconditioner.

    Axial and horizontal quadratic blocks are diagonal. Only vertical wires
    and the board share the large Cholesky factor. Nonlinear residuals retain
    every coordinate and all contacts; this is NOT a modal reduction.
    """
    def _prepare(self,dt):
        if self.fast_cache is not None and self.fast_cache['dt']==dt:
            return self.fast_cache
        core=self.system.string;h=dt/2;Cb=core.board_transform
        A=self.air.impedance(2/dt).real
        Db=self.system.damping[core.sb,core.sb]
        E=Db+Cb.T@A@Cb
        factors=[]
        for indices in core.groups:
            block=core.K[np.ix_(indices,indices)]
            if np.all(indices<core.ns):
                if np.any(block-np.diag(np.diag(block))):
                    raise ValueError('expected invariant diagonal wire block')
                factors.append((indices,1+.25*dt*dt*np.diag(block),None))
            else:
                matrix=np.eye(len(indices))+.25*dt*dt*block
                matrix[-core.nb:,-core.nb:]+=h*E
                factors.append((indices,None,cho_factor(matrix)))
        wave_plan=(MappedPlan(self.air.wave,dt) if isinstance(self.air.wave,MappedWave)
                   else PadeMidpoint(self.air.wave,dt))
        self.fast_cache=dict(dt=dt,A=A,Db=Db,E=E,Cb=Cb,factors=factors,
            velocity_factor=cho_factor(np.eye(core.nb)+h*Db),wave_plan=wave_plan,
            relaxation_alpha=dt/(2*self.air.relaxation.tau))
        return self.fast_cache

    def step(self,state,dt,defer_radiation=False):
        if not np.isfinite(dt) or dt<=0:raise ValueError('positive contact dt')
        q,v,r,z=state;cache=self._prepare(dt);core=self.system.string;sb=core.sb
        constant,offsets=cache['wave_plan'].constant(r)
        constant-=self.air.relaxation.B@(z/(1+cache['relaxation_alpha']))
        external=np.zeros(self.n);external[sb]=cache['Cb'].T@constant
        def plus(x):
            y=x.copy();y[sb]+=dt/2*(cache['E']@x[sb]);return y
        candidate=q+dt*v
        scale=max(np.linalg.norm(candidate),np.linalg.norm(dt*v),1e-12)
        fallback=0;accepted=None
        for iteration in range(24):
            g,stats=self.avf_gradient(q,candidate,dt) if accepted is None else accepted
            accepted=None
            residual=plus(candidate-q)-dt*v+.5*dt*dt*(g+external)
            norm=np.linalg.norm(residual)
            if norm<2e-14*scale:break
            if iteration<8:
                update=residual.copy()
                for indices,diagonal,factor in cache['factors']:
                    update[indices]=(residual[indices]/diagonal if factor is None else
                        cho_solve(factor,residual[indices],check_finite=False))
            else:
                _,H,_=self.system.avf(q,candidate,dt)
                matrix=np.eye(self.n)+.5*dt*dt*H
                matrix[sb,sb]+=dt/2*cache['E']
                update=np.linalg.solve(matrix,residual);fallback+=1
            for power in range(12):
                trial=candidate-update*2.**(-power)
                tg,ts=self.avf_gradient(q,trial,dt)
                test=plus(trial-q)-dt*v+.5*dt*dt*(tg+external)
                if np.linalg.norm(test)<norm:
                    candidate=trial;accepted=(tg,ts);break
            else:raise RuntimeError('shared contact line search')
        else:raise RuntimeError('shared contact residual did not converge')
        center=(candidate-q)/dt;boundary=cache['Cb']@center[sb]
        new_r,inputs=cache['wave_plan'].update(r,boundary,offsets)
        new_z,_,relax=self.air.relaxation.midpoint(z,boundary,dt)
        load=cache['A']@boundary+constant
        next_v=v-dt*g
        next_v[sb]=cho_solve(cache['velocity_factor'],v[sb]-dt/2*cache['Db']@v[sb]
                            -dt*(g[sb]+cache['Cb'].T@load),check_finite=False)
        radiated=relax if defer_radiation else relax+self.air.wave.radiation_batch(inputs[None])[0]
        stats.update(material_loss_j=dt*float(center[sb]@cache['Db']@center[sb]),
            radiation_loss_j=dt*float(radiated),air_port_work_j=dt*float(boundary@load),
            newton_iterations=iteration+1,full_tangent_fallbacks=fallback)
        stats['loss_j']=stats['felt_loss_j']+stats['material_loss_j']+stats['radiation_loss_j']
        if defer_radiation:stats['wave_inputs']=inputs
        return (candidate,next_v,new_r,new_z),stats


class SharedCoast(ExponentialCoast):
    def __init__(self, system, air):
        BoundedContact.__init__(self,system,air)
        core = system.string; self.K = core.K
        self.blocks = []
        reconstruction = np.zeros_like(self.K)
        for indices in core.groups:
            block = self.K[np.ix_(indices,indices)]
            values,basis = eigh(block)
            if values[0] <= 0:
                raise ValueError('positive mechanical block')
            reconstruction[np.ix_(indices,indices)] = block
            self.blocks.append((indices,np.sqrt(values),basis))
        if not np.array_equal(reconstruction,self.K):
            raise ValueError('cross-block coupling omitted')
        self.Cb = self.C[:,core.sb]; self.Db = system.damping[core.sb,core.sb]
        self.pade = True; self.residue_matrix = None
        self.mechanical_cache = self.air_cache = None

    def _air_prepare(self, dt):
        cache = super()._air_prepare(dt)
        if isinstance(self.air.wave,MappedWave) and not isinstance(cache['wave_plan'],MappedPlan):
            cache['wave_plan'] = MappedPlan(self.air.wave,dt)
        return cache

    def mechanical_step(self, q,v,dt):
        n = self.system.string.size
        if dt <= 0 or self.system.compression(q) > 1e-12:
            raise ValueError('coast requires separated heads')
        C,S,Q,W2 = self.oscillator_functions(dt)
        a,p = self.project(q[:n]),self.project(v[:n])
        linear = C*a+S*p; candidate = linear.copy()
        scale = max(np.linalg.norm(a),np.linalg.norm(S*p),1e-12)
        for iteration in range(40):
            physical = self.unproject(candidate)
            gradient = self.project(self.system.string.nonlinear_avf(q[:n],physical))
            residual = candidate-linear+Q*gradient
            if np.linalg.norm(residual) < 2e-14*scale:
                break
            candidate -= residual
        else:
            raise RuntimeError('shared nonlinear coast did not converge; refine dt')
        new_q = np.r_[physical,q[n:]+dt*v[n:]]
        new_v = np.r_[self.unproject(-W2*S*a+C*p-S*gradient),v[n:]]
        if self.system.compression(new_q) > 1e-12:
            raise ValueError('shared head recontact needs contact solver')
        return new_q,new_v,dict(mechanical_iterations=iteration+1,full_tangent_fallbacks=0)


class SharedDampers:
    def __init__(self, core, heads, coefficient=.6, center=.3, width=.08, nodes=16):
        if coefficient < 0 or not 0<center-width/2<center+width/2<1:
            raise ValueError('passive interior damper patch')
        x,w = leggauss(nodes); x=center+width*x/2; w=w/2
        self.entries = []
        for key in sorted(set(p.key for p in core.ports)):
            rows = []
            for e in core.entries:
                if e['port'].key != key:
                    continue
                base,wire = e['base'],e['port'].wire
                shape = np.sin(np.pi*x[:,None]*np.arange(1,base.nt+1))/np.sqrt(wire.modal_mass)
                patch = np.zeros((2*nodes,core.size))
                patch[:nodes,e['sv']] = shape
                patch[:nodes,core.sb] = x[:,None]*e['phi']
                patch[nodes:,e['sw']] = shape
                patch = patch@core.transform*np.sqrt(coefficient*np.tile(w,2))[:,None]
                rows.append(patch)
            _,s,vt = np.linalg.svd(np.vstack(rows),full_matrices=False)
            keep=s>max(s.max(initial=0)*1e-12,1e-14)
            basis=np.zeros((core.size+heads,int(keep.sum())))
            basis[:core.size]=vt[keep].T
            self.entries.append((key,basis,s[keep]**2))

    def dissipate(self, v,dt,levels):
        if dt < 0:
            raise ValueError('positive dissipation interval')
        result=v.copy(); heat=0.
        # Symmetric composition for mutually coupled patches; all heat positive.
        for key,basis,eigenvalues in self.entries+self.entries[::-1]:
            level=levels[key]
            if not 0<=level<=1:
                raise ValueError('damper engagement')
            if level == 0:
                continue
            projection=basis.T@result
            change=np.expm1(-dt/2*level*eigenvalues)
            result += basis@(change*projection)
            heat -= .5*float(np.dot(change*(2+change),projection**2))
        return result,float(heat)
