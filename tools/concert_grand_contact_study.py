"""Resolved SI contact study: exact cosine quadrature and moving initial states.

Fixed/simply supported ends, circular solid steel, optional Euler bending.
This is not a Timoshenko string, a real felt identification or an output voice.
"""
import numpy as np
from scipy.fft import dct,idct
from scipy.integrate import solve_ivp
from concert_grand_nonlinear_string import String
from concert_grand_coupled_contact import HammerString


class CosineString(String):
    """Same finite-dimensional potential with exact midpoint cosine quadrature.

    The strain has degree max(n_axial,2*n_transverse). Taking more midpoint
    nodes than that degree integrates its square and gradient products exactly.
    DCTs evaluate the gradient; the inherited matrix Hessian is used for AVF.
    """
    def __init__(self,wire,transverse=12,axial=24,bending=True,nodes=None):
        count=max(axial,2*transverse)+1 if nodes is None else nodes
        if count<=max(axial,2*transverse):raise ValueError('cosine quadrature aliases strain products')
        super().__init__(wire,transverse,axial,quadrature=8,bending=False)
        self.nodes=count;self.bending=bending
        self.x=(np.arange(count)+.5)*wire.length/count
        self.weight=np.full(count,wire.length/count)
        self.ku=np.pi*np.arange(1,axial+1)/wire.length/np.sqrt(wire.modal_mass)
        self.kv=np.pi*np.arange(1,transverse+1)/wire.length/np.sqrt(wire.modal_mass)
        self.du=np.cos(np.pi*(np.arange(count)[:,None]+.5)*np.arange(1,axial+1)/count)*self.ku
        self.dt=np.cos(np.pi*(np.arange(count)[:,None]+.5)*np.arange(1,transverse+1)/count)*self.kv
        k=np.pi*np.arange(1,transverse+1)/wire.length
        stiffness=wire.tension*wire.length/2*self.kv**2
        if bending:stiffness+=wire.young*(wire.area**2/(4*np.pi))*wire.length/2*k**4/wire.modal_mass
        self.kt=np.diag(stiffness)

    def synthesize(self,coefficients):
        packed=np.zeros(self.nodes);packed[1:len(coefficients)+1]=self.nodes*coefficients
        return idct(packed,type=2)

    def gradient_hessian(self,q,hessian=True):
        if hessian:return super().gradient_hessian(q,True)
        ux=self.synthesize(self.ku*q[self.su]);vx=self.synthesize(self.kv*q[self.sv]);wx=self.synthesize(self.kv*q[self.sw])
        strain=ux+.5*(vx*vx+wx*wx);factor=self.wire.axial_rigidity*self.wire.length/(2*self.nodes)
        g=np.empty(self.size)
        g[self.su]=factor*self.ku*dct(strain,type=2)[1:self.nl+1]
        g[self.sv]=self.kt@q[self.sv]+factor*self.kv*dct(strain*vx,type=2)[1:self.nt+1]
        g[self.sw]=self.kt@q[self.sw]+factor*self.kv*dct(strain*wx,type=2)[1:self.nt+1]
        return g

    def end_force(self,q):
        """Transverse tension plus Euler shear, with outward sign at x=L.

        Fixed endpoint imposes u_x(L)+(v_x(L)^2+w_x(L)^2)/2 as local strain;
        the reaction is not the historical tension-only projection.
        """
        sign_t=(-1.)**np.arange(1,self.nt+1);sign_u=(-1.)**np.arange(1,self.nl+1)
        ux=float((sign_u*self.ku)@q[self.su]);vx=float((sign_t*self.kv)@q[self.sv]);wx=float((sign_t*self.kv)@q[self.sw])
        tension=self.wire.tension+self.wire.axial_rigidity*(ux+.5*(vx*vx+wx*wx))
        k=np.pi*np.arange(1,self.nt+1)/self.wire.length
        ei=self.wire.young*self.wire.area**2/(4*np.pi) if self.bending else 0.
        shear_v=ei*float((sign_t*self.kv*k**2)@q[self.sv])
        shear_w=ei*float((sign_t*self.kv*k**2)@q[self.sw])
        return np.array([tension*vx+shear_v,tension*wx+shear_w])


def from_motion(system,q_string,p_string,speed):
    """First touch of a new incident hammer; preserve all string coordinates.

    Relaunching the hammer is external work. Its outgoing energy is removed by
    the action, then its new kinetic energy is explicitly added to the budget.
    No precontact action trajectory or backcheck collision is simulated.
    """
    q=np.r_[np.array(q_string,copy=True),0.];p=np.r_[np.array(p_string,copy=True),np.sqrt(system.felt.hammer_mass)*speed]
    q[-1]=-float(system.direction[:-1]@q[:-1])/system.direction[-1]
    return q,p


def highest_frequency(system):
    core=system.string
    axial=core.nl*np.sqrt(core.wire.young/core.wire.density)/(2*core.wire.length)
    transverse=np.sqrt(np.max(np.diag(core.kt)))/(2*np.pi)
    return max(axial,transverse)


def solve_contact(system,q,p,duration=.006,samples=1201,rtol=1e-10,points_per_cycle=12):
    n=system.size;initial=system.energy(q,p)
    def derivative(t,state):
        gradient,heat=system.continuous_force(state[:n],state[n:2*n])
        return np.r_[state[n:2*n],-gradient,heat]
    def separation(t,state):return system.compression(state[:n])
    separation.direction=-1
    result=solve_ivp(derivative,(0,duration),np.r_[q,p,0.],method='DOP853',rtol=rtol,atol=rtol*1e-4,
        max_step=1/(points_per_cycle*highest_frequency(system)),t_eval=np.linspace(0,duration,samples),events=separation)
    assert result.success
    events=[(t,state) for t,state in zip(result.t_events[0],result.y_events[0]) if t>1e-8]
    assert events,'no separation after first contact'
    core=system.string;qs=result.y[:n-1];ps=result.y[n:2*n-1]
    compression=system.direction@result.y[:n]
    factor=np.exp(np.clip(system.felt.rate_damping_s_per_m*(system.direction@result.y[n:2*n]),-40,40))
    force=system.felt.stiffness*np.maximum(compression,0.)**system.felt.exponent*factor
    ux=core.du@qs[core.su];vx=core.dt@qs[core.sv];wx=core.dt@qs[core.sw]
    strain=ux+.5*(vx*vx+wx*wx)
    energy=.5*np.sum(result.y[n:2*n]**2,axis=0)
    energy+=.5*np.sum(np.diag(core.kt)[:,None]*(qs[core.sv]**2+qs[core.sw]**2),axis=0)
    energy+=.5*core.wire.axial_rigidity*(core.weight@strain**2)
    energy+=system.felt.stiffness/(system.felt.exponent+1)*np.maximum(compression,0.)**(system.felt.exponent+1)
    balance=np.abs(energy+result.y[-1]-initial)/initial
    sign_t=(-1.)**np.arange(1,core.nt+1);sign_u=(-1.)**np.arange(1,core.nl+1)
    end_ux=(sign_u*core.ku)@qs[core.su];end_vx=(sign_t*core.kv)@qs[core.sv];end_wx=(sign_t*core.kv)@qs[core.sw]
    tension=core.wire.tension+core.wire.axial_rigidity*(end_ux+.5*(end_vx**2+end_wx**2))
    k=np.pi*np.arange(1,core.nt+1)/core.wire.length
    ei=core.wire.young*core.wire.area**2/(4*np.pi) if core.bending else 0.
    end_force=np.column_stack([tension*end_vx+ei*((sign_t*core.kv*k**2)@qs[core.sv]),
                               tension*end_wx+ei*((sign_t*core.kv*k**2)@qs[core.sw])])
    axial=.5*np.sum(ps[core.su]**2,axis=0)+.5*core.wire.axial_rigidity*(core.weight@ux**2)
    t_sep,state_sep=events[0];q_sep,p_sep=state_sep[:n],state_sep[n:2*n]
    metrics=dict(contact_time_s=float(t_sep),initial_total_energy_j=initial,
        initial_string_energy_j=system.string.energy(q[:-1],p[:-1]),incoming_hammer_energy_j=float(.5*p[-1]**2),
        final_string_energy_j=system.string.energy(result.y[:n-1,-1],result.y[n:2*n-1,-1]),
        string_energy_at_separation_j=system.string.energy(q_sep[:-1],p_sep[:-1]),
        outgoing_hammer_energy_j=float(.5*p_sep[-1]**2),heat_at_separation_j=float(state_sep[-1]),
        maximum_balance_relative_error=float(max(balance)),maximum_contact_force_n=float(max(force)),
        contact_impulse_n_s=float(np.sqrt(system.felt.hammer_mass)*(p[-1]-p_sep[-1])),
        maximum_compression_m=float(max(compression)),axial_linear_energy_at_end_j=float(axial[-1]),
        actual_contact_segments=len(events),minimum_compression_m=float(min(compression)),nfev=result.nfev,
        handoff_energy_jump_j=abs(system.energy(q_sep,p_sep)-system.string.energy(q_sep[:-1],p_sep[:-1])-.5*p_sep[-1]**2-system.felt_potential(q_sep)))
    # Unlike the coarse step audit, these are event-time states. Felt energy
    # vanishes there; removing it is checked rather than silently normalized.
    metrics['felt_energy_at_separation_j']=system.felt_potential(q_sep)
    trace=np.column_stack([result.t,compression,force,end_force,np.array(axial),result.y[-1]])
    return result,metrics,trace


def solve_free(core,q,p,times,rtol=1e-10):
    n=core.size
    def derivative(t,state):return np.r_[state[n:],-core.gradient_hessian(state[:n],False)]
    axial=core.nl*np.sqrt(core.wire.young/core.wire.density)/(2*core.wire.length)
    transverse=np.sqrt(np.max(np.diag(core.kt)))/(2*np.pi)
    result=solve_ivp(derivative,(0,max(times)),np.r_[q,p],method='DOP853',rtol=rtol,atol=rtol*1e-4,
        max_step=1/(12*max(axial,transverse)),t_eval=times)
    assert result.success
    return result
