"""Offline identification of a causal force-to-microphone observation.

The frozen physical strings and their existing bridge loading stay unchanged.
These empirical FIRs replace the downstream acoustic rendering for diagnosis;
they do not identify mechanical admittance, wood constants, or acoustic feedback.
Magnitude fitting avoids cancelling unrelated phases of recorded/native tones.
Minimum phase is a modelling choice, not a measured microphone phase.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
from scipy.linalg import lstsq
from scipy.signal import fftconvolve, lfilter
import soundfile as sf

TOOLS = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('body_fit', TOOLS/'fit-concert-grand-current-physics.py')
FIT = importlib.util.module_from_spec(spec); spec.loader.exec_module(FIT)
ATTACK = FIT.module(TOOLS/'concert_grand_attack_metrics.py', 'body_attack')
RATE = 44100
SHA = 'f1ee7a153219a9ae3b0e35863f75957234f1111363b1a51c91512dd73d37beff'
TRAIN = [(n,v) for n in range(36,97,6) for v in (60,110)]
PITCH_TEST = [(n,v) for n in range(39,94,6) for v in (60,110)]
VELOCITY_TEST = [(n,v) for n in range(36,97,3) for v in (28,92,124)]
WINDOWS = ((0,.12),(.12,.5),(.5,1.5))
FREQ = np.geomspace(35,12000,64)
NFFT, TAPS = 65536,32768
HP_POLE = np.exp(-2*np.pi*20/RATE)
HP_B = (1+HP_POLE)/2


def highpass_db(hz):
    z=np.exp(-2j*np.pi*np.asarray(hz)/RATE)
    return 20*np.log10(abs(HP_B*(1-z)/(1-HP_POLE*z)))


def weights(positions, regions):
    positions = np.asarray(positions)
    if regions == 1: return np.ones((len(positions),1))
    return np.maximum(0,1-abs(positions[:,None]-np.linspace(0,1,regions)[None,:])*(regions-1))


def capture(binary, directory, note, velocity, seconds=1.7):
    directory.mkdir(parents=True,exist_ok=True)
    frames = round(RATE*seconds)
    events = f'0 144 {note} {velocity}\n'
    if seconds > 2: events += f'{2*RATE} 128 {note} 64\n'
    signature = dict(binary_sha256=FIT.AUDIT.sha(binary),frames=frames,rate=RATE,
                     events=events,preset='concert',tuning={},warmup_ms=200,block_frames=256,
                     capture_channels=16,capture_point='after shared bed, before board projection',
                     capture_units='native calibrated bridge-drive units, not measured newtons')
    manifest=directory/'manifest.json'
    files=(directory/'performance.f32',directory/'bridge-drive.f32')
    valid=(manifest.exists() and json.loads(manifest.read_text()) == signature
           and all(p.exists() and p.stat().st_size==frames*c*4 for p,c in zip(files,(2,16))))
    if not valid:
        (directory/'events.txt').write_text(events)
        (directory/'tuning.txt').write_text('')
        env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str((directory/'events.txt').resolve()),CG_TOTAL_FRAMES=str(frames),
                   CG_RENDER_DIR=str(directory.resolve()),CG_TUNING=str((directory/'tuning.txt').resolve()),
                   CG_PRESET='concert',CG_RATE=str(RATE),CG_BRIDGE_CAPTURE='1',RUST_MIN_STACK='33554432')
        with (directory/'render.log').open('w') as log:
            subprocess.run([str(binary.resolve()),'tests::render_reference_wavs','--exact','--ignored',
                            '--nocapture','--test-threads=1'],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        assert 'complaints: []' in (directory/'render.log').read_text()
        assert all(p.stat().st_size==frames*c*4 for p,c in zip(files,(2,16)))
        manifest.write_text(json.dumps(signature,indent=2))
    y=np.array(FIT.AUDIT.load(files[0]),dtype=np.float64)
    x=np.array(np.memmap(files[1],dtype='<f4',mode='r').reshape(-1,16),dtype=np.float64)
    assert np.isfinite(x).all() and np.isfinite(y).all() and np.max(abs(x))>0
    return x,y


def spectral_rows(x,actual,target,note):
    # The fitted magnitudes have no common travel delay; exported convolution
    # uses the captured sample timeline without shifting individual notes.
    source=x.sum(axis=1)
    sx,_=FIT.EVAL.aligned(np.repeat(source[:,None],2,axis=1))
    ay,_=FIT.EVAL.aligned(actual)
    ty,_=FIT.EVAL.aligned(target)
    rows=[]
    for lo,hi in WINDOWS:
        sl=slice(round(lo*RATE),round(hi*RATE))
        hz,p=FIT.EVAL.psd(sx[sl])
        window=np.hanning(len(ty[sl]))
        size=max(32768,1 << (len(window)-1).bit_length())
        def power(z):
            fft=np.fft.rfft(z*window[:,None],n=size,axis=0)
            return 2*abs(fft)**2/(size*np.sum(window*window))
        t,b=power(ty[sl]),power(ay[sl])
        cells=[]
        for f in FREQ:
            mask=abs(hz-f)<max(8.,.075*f)
            cells.append((p[mask].sum(),t[mask].sum(axis=0),b[mask].sum(axis=0)))
        ceiling=max(float(np.max(q[1])) for q in cells)
        source_ceiling=max(q[0] for q in cells)
        for f,(u,t,b) in zip(FREQ,cells):
            # Inversion has no evidence at input nulls. Reference-only support
            # rejects recording noise; both channels use one reliability mask.
            if u < source_ceiling*1e-4 or np.mean(t) < ceiling*1e-4: continue
            target_db=10*np.log10((t+1e-30)/(u+1e-30))
            prior_db=10*np.log10((b+1e-30)/(u+1e-30))
            rows.append((f,(note-21)/87,target_db,prior_db))
    # A high soft note can expose only its fundamental in each window.
    # Keep the reliability mask; do not invent extra harmonic evidence.
    assert len(rows)>=len(WINDOWS),(note,len(rows))
    return rows


def identify(records,regions,regularization):
    rows=[row for record in records for row in record['rows']]
    f=np.array([r[0] for r in rows])
    ix=np.searchsorted(FREQ,f)
    # Observations lie on these fixed spectral cells. Keeping explicit rows
    # permits joint fitting and smoothness instead of independent note filters.
    a=np.zeros((len(rows),len(FREQ)*regions))
    w=weights([r[1] for r in rows],regions)
    for r in range(regions): a[np.arange(len(rows)),r*len(FREQ)+ix]=w[:,r]
    target=np.array([r[2] for r in rows]); prior=np.array([r[3] for r in rows])
    prior_curve=np.stack([np.median(prior[ix==j],axis=0) if np.any(ix==j) else [np.nan,np.nan]
                          for j in range(len(FREQ))])
    for side in range(2):
        good=np.isfinite(prior_curve[:,side])
        prior_curve[:,side]=np.interp(np.log(FREQ),np.log(FREQ[good]),prior_curve[good,side])
    prior_curve=np.tile(prior_curve,(regions,1))
    # Each record gets equal total weight, independent of how many cells its
    # spectrum supports. No per-note gain adjustment or normalization.
    row_weights=np.concatenate([np.full(len(r['rows']),1/np.sqrt(len(r['rows']))) for r in records])
    d=np.diff(np.eye(len(FREQ)),n=2,axis=0)
    smooth=np.kron(np.eye(regions),d)*regularization
    spatial=np.kron(np.diff(np.eye(regions),axis=0),np.eye(len(FREQ)))*regularization*.3
    ridge=np.eye(len(FREQ)*regions)*.05
    matrix=np.vstack((a*row_weights[:,None],smooth,spatial,ridge))
    rhs=np.vstack((target*row_weights[:,None],np.zeros((len(smooth)+len(spatial),2)),ridge@prior_curve))
    gain=lstsq(matrix,rhs,lapack_driver='gelsy')[0]
    # Prevent poorly supported inversions from requesting arbitrary boosts.
    gain=np.clip(gain,prior_curve-24,prior_curve+24)
    return gain.reshape(regions,len(FREQ),2)


def kernels(gain):
    hz=np.fft.rfftfreq(NFFT,1/RATE)
    result=np.empty((gain.shape[0],2,TAPS))
    tail=[]
    for r in range(gain.shape[0]):
        for side in range(2):
            # Keep the exact DC zero out of the log cepstrum. A tiny arbitrary
            # DC floor creates a long circular tail. Construct the finite
            # minimum-phase body first, then apply a known stable 20Hz HP.
            db=np.interp(np.log(np.maximum(hz,1)),np.log(FREQ),gain[r,:,side]-highpass_db(FREQ))
            magnitude=10**(db/20)
            cep=np.fft.irfft(np.log(np.maximum(magnitude,1e-14)),n=NFFT)
            minimum=np.zeros(NFFT); minimum[0]=cep[0]
            minimum[1:NFFT//2]=2*cep[1:NFFT//2]; minimum[NFFT//2]=cep[NFFT//2]
            full=np.fft.irfft(np.exp(np.fft.rfft(minimum)),n=NFFT)
            full=lfilter([HP_B,-HP_B],[1.,-HP_POLE],full)
            # Finite causal FIR, no fitted unstable poles or output feedback.
            result[r,side]=full[:TAPS]
            tail.append(float(10*np.log10((np.sum(full[TAPS:]**2)+1e-30)/(np.sum(full**2)+1e-30))))
    assert np.isfinite(result).all()
    return result,dict(discarded_tail_energy_db=tail,taps=TAPS,fft_size=NFFT,
                       maximum_l1_norm=float(np.max(np.sum(abs(result),axis=-1))),
                       maximum_dc_gain=float(np.max(abs(result.sum(axis=-1)))),
                       phase='minimum phase chosen from fitted magnitude; not measured phase',
                       dc_zero='analytic stable 20Hz highpass before FIR truncation')


def apply(x,h):
    spatial=x@weights(np.linspace(0,1,16),len(h))
    y=np.zeros((len(x),2))
    for r in range(len(h)):
        for side in range(2): y[:,side]+=fftconvolve(spatial[:,r],h[r,side])[:len(x)]
    assert np.isfinite(y).all()
    return y


def validation(records,h):
    rows={}; features={}; peaks={}
    for record in records:
        y=apply(record['x'],h)
        key=record['key']
        features[key]=FIT.measure(y,record['note'])
        features[key]['temporal_attack']=ATTACK.measure(y,record['note'])
        rows[key]=FIT.score(record['target_features'],features[key])
        rows[key]['temporal_attack']=ATTACK.compare(record['target_features']['temporal_attack'],
                                                   features[key]['temporal_attack'])
        peaks[key]=float(np.max(abs(y)))
    return rows,features,peaks


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('binary','work','output'): p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--expected-sha256',default=SHA)
    a=p.parse_args(); a.work.mkdir(parents=True,exist_ok=True); a.output.mkdir(parents=True,exist_ok=True)
    assert FIT.AUDIT.sha(a.binary)==a.expected_sha256
    mapping_path=TOOLS.parent/'local/steinway-midi/render-stage/mapping.json'
    mapping=json.loads(mapping_path.read_text()); sfz=mapping_path.parent/'SteinwayD-StageORTF.sfz'
    dll=TOOLS.parent/'local/steinway-midi/sfizz/library/bin/Release/sfizz.dll'
    reference_manifest=dict(mapping_sha256=FIT.AUDIT.sha(mapping_path),sfz_sha256=FIT.AUDIT.sha(sfz),
                            dll_sha256=FIT.AUDIT.sha(dll),sources={})
    for n,v in sorted(set(TRAIN+PITCH_TEST+VELOCITY_TEST)):
        entry=next(r for r in mapping['layers'] if r['root']==n and r['layer']==v//8+1)
        file=Path(entry['file']); digest=FIT.AUDIT.sha(file)
        assert digest==mapping['sources_sha256'][str(file.resolve())]
        reference_manifest['sources'][f'{n}:{v}']=dict(file=str(file),sha256=digest,layer=v//8+1,transposition=0)
    (a.work/'reference-manifest.json').write_text(json.dumps(reference_manifest,indent=2))
    synth=FIT.SAMPLER.Engine(dll,sfz)
    def records(pairs):
        with ThreadPoolExecutor(max_workers=2) as pool:
            captures=list(pool.map(lambda nv:capture(a.binary,a.work/'capture'/f'{nv[0]}-{nv[1]}',*nv),pairs))
        result=[]
        for (n,v),(x,actual) in zip(pairs,captures):
            path=a.work/'reference'/f'{n}-{v}.wav'; path.parent.mkdir(exist_ok=True)
            meta=path.with_suffix('.json')
            signature={k:v for k,v in reference_manifest.items() if k!='sources'}
            signature['source']=reference_manifest['sources'][f'{n}:{v}']
            if not (path.exists() and meta.exists() and json.loads(meta.read_text())==signature):
                sf.write(path,FIT.EVAL.render_reference(synth,n,v),RATE,subtype='FLOAT')
                meta.write_text(json.dumps(signature,indent=2))
            target,rate=sf.read(path,always_2d=True); assert rate==RATE
            target_features=FIT.measure(target,n)
            target_features['temporal_attack']=ATTACK.measure(target,n)
            baseline=FIT.score(target_features,FIT.measure(actual,n))
            baseline['temporal_attack']=ATTACK.compare(target_features['temporal_attack'],ATTACK.measure(actual,n))
            result.append(dict(key=f'{n}:{v}',note=n,velocity=v,x=x,actual=actual,
                               target_features=target_features,baseline=baseline,
                               rows=spectral_rows(x,actual,target,n)))
        return result
    try:
        train=records(TRAIN)
        print('CAPTURED TRAIN',len(train),flush=True)
        baseline={r['key']:r['baseline'] for r in train}
        cv={}
        for regions in (1,3):
            for regularization in (.3,3.,30.):
                name=f'regions-{regions}-smooth-{regularization:g}'
                rows={}; peak=0.; tails=[]
                for fold in range(4):
                    held=[r for r in train if ((r['note']-36)//6)%4==fold]
                    fit=[r for r in train if ((r['note']-36)//6)%4!=fold]
                    gain=identify(fit,regions,regularization); h,checks=kernels(gain)
                    scored,_,peaks=validation(held,h); rows.update(scored)
                    peak=max(peak,*peaks.values()); tails.extend(checks['discarded_tail_energy_db'])
                stats=FIT.aggregate(rows,baseline)
                attack_guard=ATTACK.guards({k:r['temporal_attack'] for k,r in rows.items()},
                                          {k:r['temporal_attack'] for k,r in baseline.items()})
                cv[name]=dict(regions=regions,regularization=regularization,metrics=stats,rows=rows,
                              temporal_attack_guard=attack_guard,
                              maximum_peak=peak,worst_discarded_tail_energy_db=max(tails))
                print('CV',name,json.dumps(stats),flush=True)
                (a.work/'cv-progress.json').write_text(json.dumps(cv,indent=2))
        attack_eligible=[k for k in cv if cv[k]['temporal_attack_guard']['passed']]
        selected=min(attack_eligible or list(cv),key=lambda k:cv[k]['metrics']['objective'])
        config=cv[selected]
        gain=identify(train,config['regions'],config['regularization']); h,checks=kernels(gain)
        np.savez(a.work/'identified-body.npz',kernels=h,gain_db=gain,frequency_hz=FREQ,rate=RATE,
                 drive_positions=np.linspace(0,1,16))
        selection=dict(algorithm_version=3,name=selected,regions=config['regions'],regularization=config['regularization'],
                       temporal_attack_eligible_cv=attack_eligible,
                       diagnostic_only=not bool(attack_eligible),
                       kernel_sha256=FIT.AUDIT.sha(a.work/'identified-body.npz'),numerics=checks,
                       training_pairs=TRAIN,pitch_test=PITCH_TEST,velocity_test=VELOCITY_TEST,
                       cv= cv,selection_policy='Four folds grouped by pitch, both velocities stay together. '
                       'Select among configurations passing the CV temporal attack guards. If none pass, '
                       'export the best harmonic score for diagnosis only. Selection frozen before reserved rendering. '
                       'Temporal attack and reserved-set guards are additionally required for acceptance.')
        (a.work/'selection.json').write_text(json.dumps(selection,indent=2))
        print('FROZEN SELECTION',selected,flush=True)
        report=dict(policy=__doc__,binary_sha256=a.expected_sha256,reference=reference_manifest,
                    selection=selection,groups={},sample_rate=RATE,export_gain_db=0,
                    caveats=['Internal reserved corpus, not independent recordings.',
                             'Reference layer mapping is not measured physical hammer velocity.',
                             'Phase, mechanical loading and source dynamics are not fitted.',
                             'Prototype replaces downstream acoustics; no old room is added twice.'])
        for label,data in [('training',train),('reserved-pitches',records(PITCH_TEST)),
                           ('reserved-velocities',records(VELOCITY_TEST))]:
            base={r['key']:r['baseline'] for r in data}
            rows,features,peaks=validation(data,h)
            bm,cm=FIT.aggregate(base),FIT.aggregate(rows,base)
            acceptance=FIT.acceptance(cm,rows,bm,base)
            attack_guard=ATTACK.guards({k:r['temporal_attack'] for k,r in rows.items()},
                                      {k:r['temporal_attack'] for k,r in base.items()})
            acceptance['checks'].update({f'temporal_attack_{k}':v for k,v in attack_guard['checks'].items()})
            acceptance['checks']['cv_temporal_attack_passed']=bool(attack_eligible)
            acceptance['checks']['no_clipping']=max(peaks.values())<1
            acceptance['checks']['small_discarded_tail']=max(checks['discarded_tail_energy_db'])<-40
            acceptance['passed']=all(acceptance['checks'].values())
            report['groups'][label]=dict(actual=bm,identified=cm,acceptance=acceptance,rows=rows,
                                         temporal_attack_guard=attack_guard,
                                         baseline_rows=base,features=features,maximum_peak=max(peaks.values()))
            print('RESULT',label,'actual',bm['total'],'identified',cm['total'],'passed',acceptance['passed'],flush=True)
            (a.output/'report.json').write_text(json.dumps(report,indent=2))
        # Reviewable independent sequences retain physical damper release,
        # and every layer is exported with the same unity gain.
        audition=[(n,v) for n in (42,48,60,72,84,90) for v in (28,60,92,124)]
        parts={k:[] for k in ('Actual','Cuerpo-identificado','Steinway')}; agenda=[]
        for i,(n,v) in enumerate(audition):
            x,actual=capture(a.binary,a.work/'audition-capture'/f'{n}-{v}',n,v,4.)
            generated=apply(x,h); target=FIT.EVAL.render_reference(synth,n,v)
            for label,y in zip(parts,(actual,generated,target)):
                assert np.isfinite(y).all()
                # FLOAT preserves any over-range diagnostic rather than hiding
                # it with a per-note normalizer or a new output limiter.
                subtype='PCM_24' if np.max(abs(y))<1 else 'FLOAT'
                sf.write(a.output/f'{n}-{v}-{label}.wav',y,RATE,subtype=subtype)
                clip=y[:round(2.5*RATE)].copy(); clip[-441:]*=np.linspace(1,0,441)[:,None]
                parts[label].extend((clip,np.zeros((RATE//2,2))))
            agenda.append(dict(note=n,velocity=v,start_seconds=3*i,note_off_seconds=2))
        for label,clips in parts.items():
            y=np.concatenate(clips)
            sf.write(a.output/f'{label}-intensidades.wav',y,RATE,subtype='PCM_24' if np.max(abs(y))<1 else 'FLOAT')
            sf.write(a.output/f'C6-fuerte-{label}.wav',sf.read(a.output/f'84-124-{label}.wav',always_2d=True)[0],RATE,subtype='FLOAT')
        (a.output/'audition.json').write_text(json.dumps(dict(sequence=agenda,export_gain_db=0,normalization='none'),indent=2))
        (a.output/'selection.json').write_text(json.dumps(selection,indent=2))
        np.savez(a.output/'identified-body.npz',kernels=h,gain_db=gain,frequency_hz=FREQ,rate=RATE,
                 drive_positions=np.linspace(0,1,16))
        print('DONE',a.output,flush=True)
    finally: synth.close()


if __name__=='__main__': main()
