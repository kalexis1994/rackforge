"""Trace the short treble tone; distinguish loss, projection and MIDI gate.

Follows analyze-concert-grand-short-attacks.py. Losses disabled here are
counterfactual diagnostics, not a proposed voicing or replacement physics.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import numpy as np
from scipy.io import wavfile
from scipy.signal import fftconvolve


def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    root=Path(__file__).resolve().parents[1]
    work=root/'local/concert-grand-short-attacks-2026-10-14'
    stage=work/'decay';stage.mkdir(exist_ok=False)
    docs=root/'docs/concert-grand-release-body-2026-10-14'
    prior_path=docs/'short-attacks.json'
    prior=json.loads(prior_path.read_text(encoding='utf-8'))
    build=json.loads((docs/'string-tension-verification.json').read_text(encoding='utf-8'))
    for path,h in build['files_sha256'].items():assert sha(path)==h,path
    probe=Path(build['full_render']['command'][0]); tests=probe.parent/'tests.exe'
    helper=root/'tools/measure-concert-grand-release-body.py'
    spec=importlib.util.spec_from_file_location('body',helper)
    body=importlib.util.module_from_spec(spec);spec.loader.exec_module(body)
    audio=Path('C:/SHARED/Piano-Ticks-Campanella-2026-10-14')
    rate=44100; files=[Path(__file__),helper,prior_path,probe,tests,root/'plugins/concert-grand/src/lib.rs']

    def render(stem,events,frames,tuning=None):
        raw=stage/f'{stem}.f32'
        command=[str(probe),str(events),str(raw),str(rate),str(frames),'256','concert']
        if tuning:command.append(str(tuning))
        native=json.loads(subprocess.check_output(command,cwd=root,text=True))
        x=np.fromfile(raw,dtype='<f4').reshape(-1,2)
        assert len(x)==frames and np.isfinite(x).all() and abs(x).max()<1
        return x,dict(command=command,native=native,raw_sha256=sha(raw))

    def wav(name,x):
        path=audio/f'{name}.wav';assert not path.exists()
        wavfile.write(path,rate,x)
        sr,y=wavfile.read(path);assert sr==rate and np.array_equal(x,y)
        return dict(path=str(path),sha256=sha(path),export_gain=1,rate=rate)

    def trace(folder):
        rows=[]
        for path in sorted(folder.glob('*.csv')):
            files.extend([path,path.with_suffix('.f32')])
            d=np.genfromtxt(path,delimiter=',',names=True)
            z=np.stack([d['s0']+1j*d['c0'],d['s1']+1j*d['c1'],d['s2']+1j*d['c2'],d['sh']+1j*d['ch']],1)
            t=d['frame']/rate
            intervals=[[.005,.02],[.08,.12],[.15,.2],[.3,.4]]
            def levels(a):return [float(np.mean(20*np.log10(np.maximum(a[(t>=lo)&(t<hi)],1e-30)))) for lo,hi in intervals]
            r=float(np.hypot(d['rc0'][0],d['rs0'][0]));drain=float(d['drain'][0]);h=float(d['horizontal'][0])
            rows.append(dict(note=int(path.stem.split('-')[0]),velocity=int(path.stem.split('-')[1]),
                trace_path=str(path),trace_sha256=sha(path),windows_seconds=intervals,
                fundamental_hz=float(np.arctan2(d['rs0'][0],d['rc0'][0])*rate/(2*np.pi)),
                intrinsic_T60_seconds=float(-np.log(1000)/(rate*np.log(r))),
                coherent_T60_seconds=float(-np.log(1000)/(rate*np.log(r*(1-drain*(3+h*h))))),
                joint_norm_db=levels(np.sqrt(np.sum(abs(z)**2,axis=1))),
                partial_projection_db=levels(abs(z.sum(axis=1)+d['sb']+1j*d['cb'])),
                bridge_port_db=levels(abs(z[:,:3].sum(axis=1)+h*z[:,3]))))
        return rows

    traces=dict(original=trace(work/'trace')); experiments=[]
    for label,text in [('sin_radiacion','RADIATION_RATE = 0\n'),('sin_perdida_viscoelastica','STRING_HF_LOSS = 0\n')]:
        tuning=stage/f'{label}.txt';tuning.write_text(text,encoding='utf-8',newline='\n');files.append(tuning)
        folder=stage/f'trace-{label}'
        env=dict(os.environ,RUST_MIN_STACK='33554432',CG_BODY_TRACE_DIR=str(folder),CG_BODY_TRACE_GRID='96:82,97:82,98:82',CG_BODY_TRACE_TUNING=str(tuning))
        command=[str(tests),'--exact','tests::export_release_body_trace','--ignored','--nocapture','--test-threads=1']
        result=subprocess.run(command,cwd=root,env=env,capture_output=True,text=True)
        assert result.returncode==0,result.stdout+result.stderr
        traces[label]=trace(folder)
        x,rendered=render('prefix-'+label,work/'original-0-52s.txt',52*rate,tuning)
        experiments.append(dict(label=label,tuning=text,render=rendered,
            output=wav(f'Diagnostico-46.8-51.2-{label}',x[round(46.8*rate):round(51.2*rate)]),
            trace_command=command,trace_output=result.stdout))

    # Match the state trace's rendered prefix to the independent public probe.
    a=np.fromfile(work/'trace/97-82.f32',dtype='<f4').reshape(-1,2)
    b=np.fromfile(work/'single-97-82-500ms-original.f32',dtype='<f4').reshape(-1,2)
    assert np.array_equal(a[:rate//2],b[:rate//2])
    # With mechanical noises off, early note-off and long hold have the same
    # tonal signal: this note is above the last damper. Exclude the long off.
    a=np.fromfile(work/'single-97-82-26ms-sin_mecanica.f32',dtype='<f4').reshape(-1,2)
    b=np.fromfile(work/'single-97-82-500ms-sin_mecanica.f32',dtype='<f4').reshape(-1,2)
    assert np.array_equal(a[:rate//2],b[:rate//2])

    # Exact recorded C7, not transposed C#7. Known edited ORTF corpus,
    # two velocity layers; relative envelopes only, not calibrated SPL.
    references=[]; envelope_windows=[[.025,.045],[.125,.145],[.2,.24],[.3,.4],[.5,.7]]
    def envelope(x,note):
        y,onset=body.aligned(x.astype(float))
        f0=440*2**((note-69)/12);n=131072
        spectrum=np.mean(abs(np.fft.rfft(y[round(.01*rate):round(.1*rate)]*np.hanning(round(.1*rate)-round(.01*rate))[:,None],n=n,axis=0))**2,axis=1)
        hz=np.fft.rfftfreq(n,1/rate);bins=np.flatnonzero(abs(hz-f0)<.03*f0)
        f=float(hz[bins[np.argmax(spectrum[bins])]])
        carrier=np.exp(-2j*np.pi*f*np.arange(len(y))/rate)
        w=np.hanning(883)
        env=fftconvolve(y*carrier[:,None],w[:,None],mode='same',axes=0)*2/w.sum()
        level=10*np.log10(np.maximum(np.mean(abs(env)**2,axis=1),1e-30))
        levels=[float(level[round(lo*rate):round(hi*rate)].mean()) for lo,hi in envelope_windows]
        return dict(frequency_hz=f,analysis_onset_frame=onset,level_db=levels,relative_db=(np.array(levels)-levels[0]).tolist())
    refroot=Path('C:/Users/kalex/OneDrive/Documents/rackforge/local/concert-grand-identified-body/reference')
    for velocity in (60,92):
        meta_path=refroot/f'96-{velocity}.json';meta=json.loads(meta_path.read_text(encoding='utf-8'))
        path=Path(meta['source']['file']);assert sha(path)==meta['source']['sha256'] and meta['source']['transposition']==0
        files.extend([meta_path,path])
        sr,x=wavfile.read(path);assert sr==rate and x.ndim==2 and x.dtype==np.int16
        x=x[:4*rate].astype(float)/32768
        events=stage/f'C7-{velocity}.txt';events.write_text(f'0 144 96 {velocity}\n88200 128 96 0\n',encoding='utf-8',newline='\n')
        y,rendered=render(f'C7-{velocity}',events,4*rate)
        references.append(dict(note=96,velocity=velocity,source=meta['source'],render=rendered,
            reference=envelope(x,96),model=envelope(y,96)))
    # Check whether the fast acoustic fall was already present in the release.
    release_path=docs/'campanella-baseline.json';release=json.loads(release_path.read_text(encoding='utf-8'))
    original_path=Path(release['output']);assert sha(original_path)==release['output_sha256']
    sr,x=wavfile.read(original_path);assert sr==rate
    release_crop=wav('Pasaje-46.8-51.2-release-publicada',x[round(46.8*rate):round(51.2*rate)])
    early_late=[]
    current=np.fromfile(work/'prefix-original.f32',dtype='<f4').reshape(-1,2)
    for target in prior['target_notes']:
        t=target['midi']['onset_seconds'];row=dict(onset_seconds=t)
        for label,y in [('release',x),('current',current)]:
            row[label]=[float(10*np.log10(np.mean(y[round((t+lo)*rate):round((t+hi)*rate)].astype(float)**2))) for lo,hi in [[0,.015],[.015,.04],[.04,.08],[.08,.14]]]
        early_late.append(row)
    report=dict(scope=__doc__,files_sha256={str(p):sha(p) for p in files},traces=traces,experiments=experiments,
        trace_audio_matches_public_probe_first_500ms=True,short_vs_long_gate_without_mechanics_identical_first_500ms=True,
        reference_scope='Known edited ORTF recordings, exact C7 only; no exact C#7 recording used; no blind validation',
        envelope_windows_seconds=envelope_windows,reference_checks=references,
        published_release_report_sha256=sha(release_path),published_release_output=release_crop,release_vs_current=early_late,
        limitations='Internal phasor norms are not SI energy. Coherent T60 inferred from frozen actual poles and rank-one drain for three active strings. Loss-off controls are unphysical diagnostics, not proposed fixes. Reference envelope does not identify a unique bridge admittance.')
    (docs/'short-decay.json').write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')
    print('Trace, loss-off controls, gate equivalence and known C7 reference complete',flush=True)


if __name__=='__main__':main()
