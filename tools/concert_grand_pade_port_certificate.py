"""Positive Padé certificate from compact port histories, with exact replay.

Keep endpoint memories and each midpoint port velocity. Regenerate centered
section inputs in bounded chunks for each angular block, without storing
the entire sections-by-time tensor. No reset, reduction or ledger inference.
"""
import numpy as np
from scipy.fft import rfft, irfft, next_fast_len
from concert_grand_pade_midpoint import PadeMidpoint


def certificate_ports(wave, states, segments, progress=None, input_block=16):
    provider = wave.base.provider
    states = np.asarray(states,dtype=float)
    if (states.ndim!=4 or states.shape[1:]!=(wave.order,2,wave.modes)
            or not np.isfinite(states).all() or not isinstance(input_block,int) or input_block<1):
        raise ValueError('finite endpoint memories and positive integer chunk required')
    plans = []
    for segment in segments:
        v = np.asarray(segment['velocity'],dtype=float)
        first,last = segment['initial_index'],segment['final_index']
        if (v.ndim!=2 or v.shape[1]!=wave.modes or not np.isfinite(v).all()
                or not isinstance(first,int) or not isinstance(last,int)
                or not 0<=first<len(states) or not 0<=last<len(states)):
            raise ValueError('finite port history and matching endpoint indices required')
        plans.append((PadeMidpoint(wave,segment['dt']),first,last,v))
    order = provider.taps-1
    size = next_fast_len(2*order)
    memory = states.transpose(0,2,1,3).reshape(-1,order,provider.modes)
    S = rfft(memory[:,::-1],n=size,axis=1)
    energies = np.zeros(len(memory))
    losses = [np.zeros(len(v)) for _,_,_,v in plans]
    replay_relative = np.zeros(len(plans))
    for block,H in enumerate(provider.blocks()):
        F = rfft(H,n=size,axis=0)
        field = np.einsum('fom,bfm->bfo',F,S,optimize=True)
        tails = irfft(field,n=size,axis=1)[:,order:2*order]
        energies += np.sum(tails*tails,axis=(1,2))/(2*provider.rate)
        observer = np.ascontiguousarray(H.transpose(1,0,2)).reshape(H.shape[1],-1)
        for index,((plan,first,last,v),loss) in enumerate(zip(plans,losses)):
            state = states[first].copy()
            for begin in range(0,len(v),input_block):
                chunk = v[begin:begin+input_block]
                inputs = np.empty((len(chunk),provider.taps,provider.modes))
                for i,velocity in enumerate(chunk):
                    state,inputs[i] = plan.update(state,velocity)
                field = inputs.reshape(len(chunk),-1)@observer.T
                loss[begin:begin+len(chunk)] += np.sum(field*field,axis=1)/2
            if block==0:
                replay_relative[index] = np.linalg.norm(state-states[last])/max(
                    np.linalg.norm(states[first]),np.linalg.norm(states[last]),1e-30)
                if replay_relative[index]>=1e-11:
                    raise ValueError('port history does not reproduce recorded acoustic endpoint')
        if progress is not None:
            progress(block+1)
    energies = energies.reshape(len(states),2)@np.array([144.,12.])
    return energies,losses,replay_relative
