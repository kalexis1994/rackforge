"""Observe selected complete angular blocks without mutating the physical wave.

The established observer remains the certificate. Compare complex finite-window
fields and Parseval flux against temporal replay; do not adopt a new default.
"""
import time
import numpy as np
from scipy.fft import rfft,next_fast_len
from concert_grand_pade_air import PadeAir
from concert_grand_pade_midpoint import PadeMidpoint
from concert_grand_bounded_observer import _ObservationSections
from concert_grand_pade_window_spectrum import section_input_spectrum


def probe_field(H,wave,initial,final,ports,dt,frequency_block=16):
    """Actual field phase and positive flux, including nonperiodic endpoints."""
    if frequency_block<1:raise ValueError('positive frequency block required')
    observer=np.ascontiguousarray(H.transpose(1,0,2)).reshape(H.shape[1],-1)
    plan=PadeMidpoint(wave,dt);state=initial.copy();history=[]
    start=time.perf_counter()
    for port in ports:
        state,inputs=plan.update(state,port);history.append(inputs)
    history=np.array(history)
    replay_s=time.perf_counter()-start
    closure=float(np.linalg.norm(state-final)/max(np.linalg.norm(initial),np.linalg.norm(final),1e-30))
    if closure>=1e-11:raise ValueError('probe port history does not reproduce its endpoint')
    start=time.perf_counter()
    temporal=history.reshape(len(history),-1)@observer.T
    reference_energy=float(dt*np.sum(temporal*temporal)/2)
    temporal_product_s=time.perf_counter()-start
    del history
    trials=[];N=len(ports)
    for size in [N,next_fast_len(N+7)]:
        expected=rfft(temporal,n=size,axis=0)
        start=time.perf_counter();U=rfft(ports,n=size,axis=0)
        weights=np.full(size//2+1,2.);weights[0]=1.
        if size%2==0:weights[-1]=1.
        error=norm=energy=0.
        for first in range(0,len(U),frequency_block):
            last=min(first+frequency_block,len(U))
            inputs=section_input_spectrum(wave,dt,initial,final,U,N,size,first,last)
            field=inputs.reshape(last-first,-1)@observer.T
            w=weights[first:last,None]
            error+=float(np.sum(w*abs(field-expected[first:last])**2))
            norm+=float(np.sum(w*abs(expected[first:last])**2))
            energy+=float(dt*np.sum(w*abs(field)**2)/(2*size))
        trials.append(dict(fft_size=size,steps=N,frequency_block=frequency_block,
            complex_field_relative=float(np.sqrt(error/max(norm,1e-300))),
            radiation_j=energy,reference_radiation_j=reference_energy,
            radiation_relative=abs(energy-reference_energy)/max(reference_energy,1e-30),
            spectral_kernel_s=time.perf_counter()-start))
    return dict(replay_relative=closure,replay_preparation_s=replay_s,
                temporal_field_product_s=temporal_product_s,trials=trials,
                timing_scope='Temporal product uses stored inputs; spectral kernel includes port FFT, '
                'section reconstruction and field products. Common H/projection costs excluded; probe only.')


class ProbeProvider:
    def __init__(self,provider,wave,states,segments,Q,blocks,frequency_block=16):
        self.original=provider;self.sections=_ObservationSections(wave,Q.shape[1])
        self.memories=states@Q;self.segments=[{**s,'velocity':s['velocity']@Q} for s in segments]
        self.Q=Q;self.selected=tuple(blocks);self.frequency_block=frequency_block;self.results=[]
    def __getattr__(self,name):return getattr(self.original,name)
    def blocks(self):
        for index,H in enumerate(self.original.blocks()):
            if index in self.selected:
                hq=(H.reshape(-1,self.original.modes)@self.Q).reshape(*H.shape[:2],self.Q.shape[1])
                rows=[]
                for segment in self.segments:
                    rows.append(probe_field(hq,self.sections,self.memories[segment['initial_index']],
                        self.memories[segment['final_index']],segment['velocity'],segment['dt'],self.frequency_block))
                self.results.append(dict(block=index,segments=rows))
            yield H
        if len(self.results)!=len(self.selected):raise ValueError('not all requested angular probe blocks were observed')


class _ProviderBaseView:
    def __init__(self,base,provider):self.original=base;self.provider=provider
    def __getattr__(self,name):return getattr(self.original,name)


def observation_wave_view(wave,provider):
    """New immutable-provider view; physical base/cache and trajectory untouched."""
    return PadeAir(_ProviderBaseView(wave.base,provider))
