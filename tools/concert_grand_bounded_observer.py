"""Independent field observation in a data basis, with omitted-field bounds.

The mechanical/acoustic trajectory is never reduced. Full residual inputs
are replayed once; R[0] supplies ||H||_F squared for an analytic field bound.
Angular factors, taps and outputs stay complete. No inference from port work.
Intervals bound projection error in exact arithmetic; floating-point margins
are numerical safeguards, not an interval-arithmetic proof of rounding.
"""
import numpy as np
from scipy.linalg import qr,svd
from scipy.fft import rfft,irfft,next_fast_len
from concert_grand_pade_air import PadeAir
from concert_grand_pade_midpoint import PadeMidpoint


class _ObservationSections(PadeAir):
    """Only the scalar section recurrence; never a reduced physical load."""
    def __init__(self,wave,columns):
        self.rate,self.order,self.modes = wave.rate,wave.order,columns
    def impedance(self,s):
        raise RuntimeError('observation sections cannot supply forces')


def observation_basis(wave,states,segments,relative=1e-12):
    states = np.asarray(states,dtype=float)
    if (states.ndim!=4 or states.shape[1:]!=(wave.order,2,wave.modes)
            or not np.isfinite(states).all() or not 0<relative<1):
        raise ValueError('finite memories and relative basis tolerance required')
    rows = [12*states[:,:,0,:].reshape(-1,wave.modes),
            np.sqrt(12)*states[:,:,1,:].reshape(-1,wave.modes)]
    for segment in segments:
        v = np.asarray(segment['velocity'],dtype=float)
        if v.ndim!=2 or v.shape[1]!=wave.modes or not np.isfinite(v).all():
            raise ValueError('finite full port histories required')
        rows.append(v)
    # QR/SVD, not eigenvalues of X.T X: keep small singular directions.
    data = np.concatenate(rows)
    R = qr(data,mode='r',overwrite_a=True,check_finite=False)[0][:wave.modes].copy()
    _,singular,Vh = svd(R,full_matrices=False,check_finite=False)
    tails = np.r_[np.cumsum(singular[::-1]**2)[::-1],0.]
    rank = max(1,next(i for i in range(len(singular)+1) if tails[i]<=relative**2*tails[0]))
    Q = Vh[:rank].T.copy()
    return Q,dict(rank=rank,full_modes=wave.modes,requested_relative=relative,
                  retained_tail_relative=float(np.sqrt(tails[rank]/tails[0])) if tails[0]>0 else 0.,
                  singular_values=singular.tolist(),basis_data_bytes=data.nbytes)


def energy_interval(estimate,amplitude_bound):
    root = np.sqrt(max(float(estimate),0.))
    b = float(amplitude_bound)
    # Norms cannot be negative. This is the triangle-inequality lower bound,
    # not a repair to eigenvalues, forces or measured physical energy.
    lo,hi = max(root-b,0.)**2,(root+b)**2
    margin = 1e-11*max(hi,1e-30)
    return dict(estimate_j=float(estimate),lower_j=max(lo-margin,0.),upper_j=hi+margin,
                amplitude_bound=float(b),roundoff_allowance_j=margin,
                error_bound_j=max(float(estimate)-max(lo-margin,0.),hi+margin-float(estimate)))


def certificate_observer(wave,states,segments,Q,progress=None,input_block=32):
    provider = wave.base.provider
    states = np.asarray(states,dtype=float)
    Q = np.asarray(Q,dtype=float)
    if (states.ndim!=4 or states.shape[1:]!=(wave.order,2,wave.modes)
            or not np.isfinite(states).all() or Q.ndim!=2 or Q.shape[0]!=wave.modes
            or not 0<Q.shape[1]<=wave.modes or not np.isfinite(Q).all()
            or np.linalg.norm(Q.T@Q-np.eye(Q.shape[1]))>1e-11 or input_block<1):
        raise ValueError('finite endpoint memories, orthonormal observation basis and positive chunk required')
    small = _ObservationSections(wave,Q.shape[1])
    projected = states@Q
    residual = states-projected@Q.T
    gram_norm2 = float(np.trace(wave.R[0]))
    if gram_norm2<=0:
        raise ValueError('positive factor norm from authenticated R required')
    gamma = (provider.taps*wave.modes*np.finfo(float).eps)
    storage_residual = np.sqrt(144*np.sum(residual[:,:,0,:]**2,axis=(1,2))+
                               12*np.sum(residual[:,:,1,:]**2,axis=(1,2)))
    storage_size = np.sqrt(144*np.sum(projected[:,:,0,:]**2,axis=(1,2))+
                           12*np.sum(projected[:,:,1,:]**2,axis=(1,2)))
    storage_b = np.sqrt(provider.taps*gram_norm2/(2*provider.rate))*(storage_residual+gamma*storage_size)
    plans,residual_input_integrals,replay = [],[],[]
    for segment in segments:
        first,last,dt = segment['initial_index'],segment['final_index'],segment['dt']
        v = np.asarray(segment['velocity'],dtype=float)
        if (not isinstance(first,int) or not isinstance(last,int) or not 0<=first<len(states)
                or not 0<=last<len(states) or v.ndim!=2 or v.shape[1]!=wave.modes
                or not np.isfinite(v).all()):
            raise ValueError('matching full port history and endpoint indices required')
        plan,mini = PadeMidpoint(wave,dt),PadeMidpoint(small,dt)
        full_state,reduced = states[first].copy(),projected[first].copy()
        integral = 0.
        for velocity in v:
            full_state,u = plan.update(full_state,velocity)
            reduced,uq = mini.update(reduced,velocity@Q)
            error = np.linalg.norm(u-uq@Q.T)+gamma*np.linalg.norm(uq)
            integral += dt*float(error**2)
        closure = float(np.linalg.norm(full_state-states[last])/max(
            np.linalg.norm(states[first]),np.linalg.norm(states[last]),1e-30))
        if closure>=1e-11:
            raise ValueError('full port replay does not reproduce endpoint memory')
        replay.append(closure)
        residual_input_integrals.append(integral)
        plans.append((mini,first,dt,v@Q))
    order = provider.taps-1
    size = next_fast_len(2*order)
    memory = projected.transpose(0,2,1,3).reshape(-1,order,Q.shape[1])
    S = rfft(memory[:,::-1],n=size,axis=1)
    energies = np.zeros(len(memory))
    losses = [np.zeros(len(v)) for _,_,_,v in plans]
    factor_norm_measured = 0.
    for block,H in enumerate(provider.blocks()):
        factor_norm_measured += float(np.sum(H*H))
        hq = (H.reshape(-1,wave.modes)@Q).reshape(H.shape[0],H.shape[1],Q.shape[1])
        F = rfft(hq,n=size,axis=0)
        field = np.einsum('fom,bfm->bfo',F,S,optimize=True)
        tails = irfft(field,n=size,axis=1)[:,order:2*order]
        energies += np.sum(tails*tails,axis=(1,2))/(2*provider.rate)
        observer = np.ascontiguousarray(hq.transpose(1,0,2)).reshape(H.shape[1],-1)
        for (plan,first,dt,v),loss in zip(plans,losses):
            state = projected[first].copy()
            for begin in range(0,len(v),input_block):
                chunk = v[begin:begin+input_block]
                inputs = np.empty((len(chunk),provider.taps,Q.shape[1]))
                for i,velocity in enumerate(chunk):
                    state,inputs[i] = plan.update(state,velocity)
                field = inputs.reshape(len(chunk),-1)@observer.T
                loss[begin:begin+len(chunk)] += np.sum(field*field,axis=1)/2
        if progress is not None:
            progress(block+1)
    norm_error = abs(factor_norm_measured-gram_norm2)/gram_norm2
    if norm_error>=1e-10:
        raise ValueError('regenerated factor differs from authenticated R norm')
    energies = energies.reshape(len(states),2)@np.array([144.,12.])
    storage = [energy_interval(e,b) for e,b in zip(energies,storage_b)]
    radiation = [energy_interval(float(power.sum()*dt),np.sqrt(gram_norm2*integral/2))
                 for power,(_,_,dt,_),integral in zip(losses,plans,residual_input_integrals)]
    return energies,losses,dict(rank=Q.shape[1],full_modes=wave.modes,storage_intervals=storage,
        radiation_intervals=radiation,full_endpoint_replay_relative=replay,
        residual_input_integral=residual_input_integrals,factor_norm_relative_error=norm_error,
        factor_norm_squared=gram_norm2,roundoff_scope='analytic projection bounds with numerical '
        'roundoff safeguards, not a formal interval-arithmetic certificate')
