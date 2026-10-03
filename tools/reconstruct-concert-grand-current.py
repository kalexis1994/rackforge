"""Control experiment: causal, phase-aware reconstruction of native pressure.

Input and target share a sample clock. No onset realignment, note gain, pitch
shift or Steinway data enters the fit. FIRs are shared across notes/velocities.
The least-squares operator masks samples after each captured record, avoiding
an artificial zero-output tail or circular convolution at capture boundaries.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time

import numpy as np
from scipy.fft import rfft, irfft, next_fast_len
from scipy.signal import fftconvolve
from scipy.sparse.linalg import LinearOperator, lsmr
import soundfile as sf

import concert_grand_attack_metrics as ATTACK


class CausalOperator(LinearOperator):
    def __init__(self, inputs, taps, weights=None):
        # inputs: records x samples x source channels
        self.records,self.frames,self.sources=inputs.shape
        self.taps=taps
        self.fft_size=next_fast_len(self.frames+taps-1)
        self.weights=np.ones(self.records) if weights is None else np.asarray(weights)
        assert len(self.weights)==self.records and np.all(self.weights>0)
        self.spectra=rfft(inputs.transpose(0,2,1),n=self.fft_size,axis=-1)
        self.calls=0
        self.progress=False
        self.started=time.monotonic()
        super().__init__(dtype=np.dtype('float64'),
                         shape=(self.records*2*self.frames,self.sources*2*taps))

    def _matvec(self, h):
        spectrum=rfft(h.reshape(self.sources,2,self.taps),n=self.fft_size,axis=-1)
        y=irfft(np.einsum('mrf,rcf->mcf',self.spectra,spectrum,optimize=True),
                n=self.fft_size,axis=-1)[...,:self.frames]
        self.calls+=1
        if self.progress and self.calls%100==0:
            print('SOLVER',self.calls,'forward calls',round(time.monotonic()-self.started,1),'seconds',flush=True)
        return (y*self.weights[:,None,None]).ravel()

    def _rmatvec(self, residual):
        r=residual.reshape(self.records,2,self.frames)*self.weights[:,None,None]
        spectrum=rfft(r,n=self.fft_size,axis=-1)
        h=irfft(np.einsum('mrf,mcf->rcf',self.spectra.conj(),spectrum,optimize=True),
                n=self.fft_size,axis=-1)[...,:self.taps]
        return h.ravel()


def spatial(x,regions):
    if regions==x.shape[1]: return x
    assert x.shape[1]==16 and regions==3
    positions=np.linspace(0,1,16)
    weights=np.maximum(0,1-abs(positions[:,None]-np.linspace(0,1,regions)[None,:])*(regions-1))
    return x@weights


def apply(x,kernels):
    z=spatial(x,len(kernels))
    y=np.zeros((len(x),2))
    for r in range(len(kernels)):
        for side in range(2): y[:,side]+=fftconvolve(z[:,r],kernels[r,side])[:len(x)]
    return y


def error_db(target,prediction):
    return float(10*np.log10((np.sum((target-prediction)**2)+1e-30)/(np.sum(target**2)+1e-30)))


def metrics(target,prediction,note,rate=44100):
    t,p=ATTACK.measure(target,note,rate),ATTACK.measure(prediction,note,rate)
    return dict(waveform_error_db=error_db(target,prediction),
                attack_waveform_error_db=error_db(target[:round(.05*rate)],prediction[:round(.05*rate)]),
                sustained_waveform_error_db=error_db(target[round(.05*rate):round(1.5*rate)],
                                                     prediction[round(.05*rate):round(1.5*rate)]),
                temporal=ATTACK.compare(t,p),actual_attack=t,reconstructed_attack=p,
                target_peak=float(np.max(abs(target))),predicted_peak=float(np.max(abs(prediction))))


def aggregate(rows):
    wave=np.array([r['waveform_error_db'] for r in rows.values()])
    attack=np.array([r['attack_waveform_error_db'] for r in rows.values()])
    shape=np.array([r['temporal']['envelope_shape_error_db'] for r in rows.values()])
    rises=[r['temporal']['rise_error_db'] for r in rows.values() if r['temporal']['rise_error_db'] is not None]
    checks=dict(median_wave_below_minus30_db=np.median(wave)<=-30,
                p95_wave_below_minus20_db=np.percentile(wave,95)<=-20,
                p95_attack_wave_below_minus20_db=np.percentile(attack,95)<=-20,
                mean_shape_below_point3_db=shape.mean()<=.3,
                p95_shape_below_1_db=np.percentile(shape,95)<=1)
    if rises: checks.update(mean_rise_below_point5_db=np.mean(rises)<=.5,p95_rise_below_1_db=np.percentile(rises,95)<=1)
    return dict(median_waveform_error_db=float(np.median(wave)),p95_waveform_error_db=float(np.percentile(wave,95)),
                p95_attack_waveform_error_db=float(np.percentile(attack,95)),
                mean_shape_error_db=float(shape.mean()),mean_resolved_rise_error_db=float(np.mean(rises)) if rises else None,
                checks={k:bool(v) for k,v in checks.items()},passed=bool(all(checks.values())),
                policy='Declared engineering reconstruction tolerances; not psychoacoustic thresholds.')


def fit(inputs,targets,taps,iterations,progress=False):
    # Equal recording weight in the loss only. No per-note gain is applied to
    # prediction or export. Global source scaling conditions columns without
    # changing the fitted convolution after coefficients are transformed back.
    scales=np.sqrt(np.mean(inputs**2,axis=(0,1)))
    observed=scales>0
    assert np.any(observed)
    # Unexcited bridge points have no identifiable coefficient. Their columns
    # remain exactly zero and LSMR leaves their kernels zero, rather than
    # inventing an interpolated response outside the observed range.
    scales=np.where(observed,scales,1.)
    weights=1/np.sqrt(np.sum(targets**2,axis=(1,2)))
    op=CausalOperator(inputs/scales[None,None,:],taps,weights)
    op.progress=progress
    rhs=(targets.transpose(0,2,1)*weights[:,None,None]).ravel()
    start=time.monotonic()
    result=lsmr(op,rhs,atol=1e-6,btol=1e-6,maxiter=iterations)
    kernels=result[0].reshape(inputs.shape[2],2,taps)/scales[:,None,None]
    return kernels,dict(stop_code=int(result[1]),iterations=int(result[2]),
                        residual_norm=float(result[3]),normal_residual_norm=float(result[4]),
                        condition_estimate=float(result[6]),seconds=time.monotonic()-start,
                        max_iterations=iterations,taps=taps,fft_size=op.fft_size,
                        observed_sources=observed.tolist(),
                        source_scaling=scales.tolist(),record_loss_weights=weights.tolist())


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ('work','output'): parser.add_argument('--'+name,type=Path,required=True)
    parser.add_argument('--regions',type=int,choices=(3,16),default=3)
    parser.add_argument('--taps',type=int,default=8192)
    parser.add_argument('--iterations',type=int,default=120)
    a=parser.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    spec=importlib.util.spec_from_file_location('body_control',Path(__file__).with_name('identify-concert-grand-body.py'))
    body=importlib.util.module_from_spec(spec);spec.loader.exec_module(body)
    manifest={}
    def load(n,v,audition=False):
        directory=a.work/('audition-capture' if audition else 'capture')/f'{n}-{v}'
        meta=json.loads((directory/'manifest.json').read_text())
        assert meta['binary_sha256']==body.SHA and meta['rate']==body.RATE and meta['tuning']=={}
        force=directory/'bridge-drive.f32';pressure=directory/'performance.f32'
        assert force.stat().st_size==meta['frames']*16*4 and pressure.stat().st_size==meta['frames']*2*4
        x=np.array(np.memmap(force,dtype='<f4',mode='r').reshape(-1,16),dtype=float)
        y=np.array(body.FIT.AUDIT.load(pressure),dtype=float)
        manifest[f'{n}:{v}'+(':audition' if audition else '')]=dict(metadata=meta,
                          force_sha256=body.FIT.AUDIT.sha(force),pressure_sha256=body.FIT.AUDIT.sha(pressure))
        return x,y
    train=[load(n,v) for n,v in body.TRAIN]
    inputs=np.stack([spatial(x,a.regions) for x,y in train])
    targets=np.stack([y for x,y in train])
    print('FIT START',a.regions,a.taps,a.iterations,flush=True)
    kernels,solver=fit(inputs,targets,a.taps,a.iterations,progress=True)
    print('FIT END',json.dumps(solver),flush=True)
    np.savez(a.output/'reconstruction.npz',kernels=kernels,rate=body.RATE,source_positions=np.linspace(0,1,16))
    report=dict(policy=__doc__,binary_sha256=body.SHA,solver=solver,source_channels=a.regions,
                kernel_sha256=body.FIT.AUDIT.sha(a.output/'reconstruction.npz'),
                fitting_pairs=body.TRAIN,phase='fitted causal FIR, sample clock unchanged',groups={})
    for label,pairs in [('training',body.TRAIN),('reserved-pitches',body.PITCH_TEST),('reserved-velocities',body.VELOCITY_TEST)]:
        rows={}
        for n,v in pairs:
            x,y=load(n,v);prediction=apply(x,kernels)
            rows[f'{n}:{v}']=metrics(y,prediction,n)
        report['groups'][label]=dict(summary=aggregate(rows),rows=rows)
        print(label,json.dumps(report['groups'][label]['summary']),flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    parts={'Actual':[],'Reconstruido':[]};audition_rows={}
    for n in (42,48,60,72,84,90):
        for v in (28,60,92,124):
            x,y=load(n,v,True);prediction=apply(x,kernels)
            audition_rows[f'{n}:{v}']=metrics(y,prediction,n)
            for label,z in (('Actual',y),('Reconstruido',prediction)):
                sf.write(a.output/f'{n}-{v}-{label}.wav',z,body.RATE,subtype='FLOAT')
                clip=z[:round(2.5*body.RATE)].copy();clip[-441:]*=np.linspace(1,0,441)[:,None]
                parts[label].extend((clip,np.zeros((body.RATE//2,2))))
    for label,clips in parts.items():sf.write(a.output/f'{label}-intensidades.wav',np.concatenate(clips),body.RATE,subtype='FLOAT')
    report['audition']=dict(summary=aggregate(audition_rows),rows=audition_rows)
    report['sources']=manifest
    report['passed']=all(g['summary']['passed'] for g in report['groups'].values()) and report['audition']['summary']['passed']
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('DONE',report['passed'],a.output,flush=True)


if __name__=='__main__':main()
