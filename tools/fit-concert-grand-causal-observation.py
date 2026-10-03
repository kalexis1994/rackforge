"""Relative empirical observation fit around the verified native causal chain.

The original impulse response and auxiliary resonances are retained. Only a
shared, regularized relative observation correction is tested. This does not
identify wood properties, real-piano phase or new mechanical feedback. Alpha
is selected by four pitch-grouped folds, with temporal attack guards, before
reserved pitches/velocities are evaluated. Alpha zero is the control baseline.
"""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
from scipy.signal import fftconvolve
import soundfile as sf

import concert_grand_attack_metrics as ATTACK

ALPHAS=(0.,.125,.25,.5,.75,1.)


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    result=importlib.util.module_from_spec(spec);spec.loader.exec_module(result);return result


def relative_kernel(delta_db,frequencies,rate=44100,nfft=65536,taps=32768):
    # A relative minimum-phase correction, not a replacement of the complete
    # body's phase. There is no DC zero or arbitrary small DC floor here.
    hz=np.fft.rfftfreq(nfft,1/rate)
    result=np.zeros((len(delta_db),2,taps));tails=[]
    for r in range(len(delta_db)):
        for side in range(2):
            logmag=np.interp(np.log(np.maximum(hz,1)),np.log(frequencies),delta_db[r,:,side])*np.log(10)/20
            cep=np.fft.irfft(logmag,n=nfft)
            minimum=np.zeros(nfft);minimum[0]=cep[0];minimum[1:nfft//2]=2*cep[1:nfft//2];minimum[nfft//2]=cep[nfft//2]
            h=np.fft.irfft(np.exp(np.fft.rfft(minimum)),n=nfft)
            result[r,side]=h[:taps]
            tails.append(float(10*np.log10((np.sum(h[taps:]**2)+1e-30)/(np.sum(h**2)+1e-30))))
    assert np.isfinite(result).all()
    return result,tails


def corrected(regional,auxiliary,kernels):
    y=auxiliary.copy()
    for r in range(len(regional)):
        for side in range(2):
            y[:,side]+=fftconvolve(regional[r,:,side],kernels[r,side])[:len(y)]
    return y


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('original','control','oracle','oracle-audit','audition-reference','output'):p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    tools=Path(__file__).resolve().parent
    b=module(tools/'identify-concert-grand-body.py','causal_body')
    c=module(tools/'reconstruct-concert-grand-current.py','causal_control')
    meta=json.loads((a.oracle/'oracle.json').read_text())
    raw=a.oracle/'board-room-kernels.f32'
    h=np.fromfile(raw,dtype='<f4').reshape(meta['kernel_count'],meta['frames'],2).transpose(0,2,1)
    audit=json.loads(a.oracle_audit.read_text())
    assert audit['passed'] and audit['kernel_sha256']==b.FIT.AUDIT.sha(raw)
    spatial_weights=b.weights(np.linspace(0,1,16),3)
    sources={}
    observer_spectra={}
    checked_fast_path=False
    def record(n,v,audition=False):
        folder='audition-capture' if audition else 'capture'
        directory=a.control/folder/f'{n}-{v}'
        key=f'{n}:{v}'+(':audition' if audition else '')
        input_hashes={filename:b.FIT.AUDIT.sha(directory/filename)
                      for filename in ('bridge-drive.f32','performance.f32','auxiliary-drive.f32')}
        assert input_hashes==audit['sources'][key],f'Control source changed: {key}'
        x=np.array(np.memmap(directory/'bridge-drive.f32',dtype='<f4').reshape(-1,16),dtype=float)
        original_force=np.fromfile(directory/'auxiliary-drive.f32',dtype='<f4').astype(float)
        actual=np.array(b.FIT.AUDIT.load(directory/'performance.f32'),dtype=float)
        # Known native MIDI-owner gate, not fitted note coefficients.
        size=c.next_fast_len(len(x)+h.shape[-1]-1)
        if size not in observer_spectra:observer_spectra[size]=c.rfft(h,n=size,axis=-1)
        spectrum=observer_spectra[size]
        drive=c.rfft(x.T,n=size,axis=-1)
        regional_spectrum=np.einsum('pf,pcf,pr->rcf',drive,spectrum[:16],spatial_weights,optimize=True)
        region=c.irfft(regional_spectrum,n=size,axis=-1)[...,:len(x)].transpose(0,2,1)
        aux=c.irfft(c.rfft(original_force,n=size)[None,:]*spectrum[17+n-meta['muted_owner_first']],
                   n=size,axis=-1)[...,:len(x)].T
        nonlocal checked_fast_path
        if not checked_fast_path:
            slow=np.stack([c.apply(x*spatial_weights[None,:,r],h[:16]) for r in range(3)])
            assert np.max(abs(slow-region))<1e-12
            checked_fast_path=True
        reference,rate=sf.read(a.original/'reference'/f'{n}-{v}.wav',always_2d=True);assert rate==b.RATE
        if audition:
            # The cached reference clips use the same two-second held key and
            # sampler release as the historical audition sequence.
            reference,rate=sf.read(a.audition_reference/f'{n}-{v}-Steinway.wav',always_2d=True)
            assert rate==b.RATE
        reference_path=(a.audition_reference/f'{n}-{v}-Steinway.wav' if audition else a.original/'reference'/f'{n}-{v}.wav')
        sources[key]=dict(inputs_sha256=input_hashes,reference_sha256=b.FIT.AUDIT.sha(reference_path),reference_path=str(reference_path.resolve()))
        target=b.FIT.measure(reference,n);target['temporal_attack']=ATTACK.measure(reference,n)
        baseline_audio=region.sum(axis=0)+aux
        baseline=score_audio(baseline_audio,target,n)
        return dict(key=f'{n}:{v}',note=n,velocity=v,regional=region,auxiliary=aux,actual=actual,
                    reference=reference,target=target,baseline=baseline,
                    rows=b.spectral_rows(x,actual,reference,n))
    def score_audio(y,target,n):
        measured=b.FIT.measure(y,n);attack=ATTACK.measure(y,n)
        result=b.FIT.score(target,measured)
        result['temporal_attack']=ATTACK.compare(target['temporal_attack'],attack)
        result['peak']=float(np.max(abs(y)))
        return result
    def curves(records):
        fit=b.identify(records,3,3.)
        native=[dict(r,rows=[(f,pos,prior,prior) for f,pos,target,prior in r['rows']]) for r in records]
        prior=b.identify(native,3,3.)
        return np.clip(fit-prior,-12,12)
    def evaluate(records,kernels):
        return {r['key']:score_audio(corrected(r['regional'],r['auxiliary'],kernels),r['target'],r['note']) for r in records}
    def attack_guard(rows,base):
        return ATTACK.guards({k:r['temporal_attack'] for k,r in rows.items()},
                            {k:r['temporal_attack'] for k,r in base.items()})
    train=[record(n,v) for n,v in b.TRAIN]
    base={r['key']:r['baseline'] for r in train}
    cv={str(alpha):{} for alpha in ALPHAS}
    for fold in range(4):
        fit=[r for r in train if ((r['note']-36)//6)%4!=fold]
        held=[r for r in train if ((r['note']-36)//6)%4==fold]
        delta=curves(fit)
        for alpha in ALPHAS:
            kernels,_=relative_kernel(delta*alpha,b.FREQ)
            cv[str(alpha)].update(evaluate(held,kernels))
        print('CV FOLD',fold,'complete',flush=True)
    summaries={}
    for alpha,rows in cv.items():
        summaries[alpha]=dict(metrics=b.FIT.aggregate(rows,base),temporal_guard=attack_guard(rows,base),
                              maximum_peak=max(r['peak'] for r in rows.values()))
    # Identity correction is an explicit fallback; it is not a new candidate.
    eligible=[k for k in summaries if summaries[k]['temporal_guard']['passed'] and summaries[k]['maximum_peak']<.9]
    if '0.0' not in eligible:eligible.append('0.0')
    selected=min(eligible,key=lambda k:summaries[k]['metrics']['objective'])
    alpha=float(selected);delta=curves(train);kernels,tails=relative_kernel(delta*alpha,b.FREQ)
    np.savez(a.output/'relative-observation.npz',kernels=kernels,gain_delta_db=delta*alpha,alpha=alpha,frequency_hz=b.FREQ)
    selection=dict(alpha=alpha,cv=summaries,eligible=eligible,training_pairs=b.TRAIN,
                    kernel_sha256=b.FIT.AUDIT.sha(a.output/'relative-observation.npz'),
                    original_observer_sha256=b.FIT.AUDIT.sha(raw),correction_tail_energy_db=tails,
                    policy='Select CV harmonic objective among temporal guard passes. Zero is baseline, not an improvement.')
    (a.output/'selection.json').write_text(json.dumps(selection,indent=2),encoding='utf-8')
    print('FROZEN SELECTION',alpha,flush=True)
    report=dict(policy=__doc__,selection=selection,groups={},export_gain_db=0,sources=sources,
                caveats=['Minimum phase is chosen only for the relative correction; real Steinway phase remains unidentified.',
                         'Mechanical string/contact/bridge parameters remain unchanged.',
                         'Internal held-out notes reuse the historical corpus.',
                         'Offline held-key owner masks do not implement arbitrary MIDI gating.'])
    for label,pairs in [('training',b.TRAIN),('reserved-pitches',b.PITCH_TEST),('reserved-velocities',b.VELOCITY_TEST)]:
        records=train if label=='training' else [record(n,v) for n,v in pairs]
        baseline={r['key']:r['baseline'] for r in records};rows=evaluate(records,kernels)
        bm,cm=b.FIT.aggregate(baseline),b.FIT.aggregate(rows,baseline)
        guards=b.FIT.acceptance(cm,rows,bm,baseline);temporal=attack_guard(rows,baseline)
        guards['checks'].update({f'temporal_{k}':v for k,v in temporal['checks'].items()})
        guards['checks']['nonzero_candidate']=alpha>0
        guards['checks']['small_correction_tail']=max(tails)<-40
        guards['checks']['linear_output_range']=max(r['peak'] for r in rows.values())<.9
        guards['passed']=all(guards['checks'].values())
        report['groups'][label]=dict(actual=bm,corrected=cm,temporal_guard=temporal,acceptance=guards,
                                     rows=rows,baseline_rows=baseline)
        print(label,'actual',bm['total'],'corrected',cm['total'],'passed',guards['passed'],flush=True)
        (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    report['accepted']=all(v['acceptance']['passed'] for v in report['groups'].values())
    # Export is diagnostic even when rejected, labelled as such. Factory is
    # never updated automatically by this tool.
    parts={k:[] for k in ('Actual','Observacion-causal','Steinway')};schedule=[]
    for n in (42,48,60,72,84,90):
        for v in (28,60,92,124):
            r=record(n,v,True);prediction=corrected(r['regional'],r['auxiliary'],kernels)
            for label,y in (('Actual',r['actual']),('Observacion-causal',prediction),('Steinway',r['reference'])):
                sf.write(a.output/f'{n}-{v}-{label}.wav',y,b.RATE,subtype='FLOAT')
                clip=y[:round(2.5*b.RATE)].copy();clip[-441:]*=np.linspace(1,0,441)[:,None]
                parts[label].extend((clip,np.zeros((b.RATE//2,2))))
            schedule.append(dict(note=n,velocity=v,start_seconds=3*(len(schedule)),note_off_seconds=2))
    for label,clips in parts.items():sf.write(a.output/f'{label}-intensidades.wav',np.concatenate(clips),b.RATE,subtype='FLOAT')
    report['audition']=schedule
    (a.output/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print('DONE',report['accepted'],a.output,flush=True)


if __name__=='__main__':main()
