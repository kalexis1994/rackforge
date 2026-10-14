"""Measure the unchanged published piano against authenticated existing references.

No DSP coefficients, output gains or per-note equalizers are fitted. Reference
and model onsets are aligned only for analysis. Raw audio stays untouched.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import numpy as np
from scipy.io import wavfile
from scipy.signal import fftconvolve

RATE=44100
WINDOWS=((.15,.4),(.45,.8),(.9,1.4))


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def aligned(x):
    step=44;z=x[:min(len(x),RATE)//step*step].reshape(-1,step,2)
    power=np.mean(z*z,axis=(1,2));assert power.max()>0
    at=int(np.flatnonzero(power>power.max()*1e-4)[0]*step)
    return np.pad(x[at:],((0,at),(0,0))),at


def envelope(x,frequency,fundamental):
    width=int(round(max(8,int(np.ceil(.04*fundamental)))*RATE/fundamental))|1
    window=np.hanning(width)
    carrier=np.exp(-2j*np.pi*frequency*np.arange(len(x))/RATE)
    a=fftconvolve(x*carrier[:,None],window[:,None],mode='same',axes=0)*2/window.sum()
    indices=np.arange(round(.1*RATE),round(1.451*RATE),round(.01*RATE))
    power=np.mean(abs(a[indices])**2,axis=1)
    return indices/RATE,10*np.log10(np.maximum(power,1e-30))


def slopes(t,y):
    result=[]
    for lo,hi in WINDOWS:
        mask=(t>=lo)&(t<hi);tx=t[mask]-t[mask].mean();ly=y[mask]
        fit=np.polyfit(tx,ly,1)
        result.append(dict(window_s=[lo,hi],slope_db_per_s=float(fit[0]),
            mean_level_db=float(ly.mean()),residual_rms_db=float(np.sqrt(np.mean((ly-np.polyval(fit,tx))**2)))))
    return result


def self_check():
    t=np.arange(2*RATE)/RATE;alpha=.7
    a=.1*np.exp(-alpha*t)*np.cos(2*np.pi*440*t)
    times,levels=envelope(np.c_[a,-a],440,440)
    expected=-20*alpha/np.log(10)
    error=max(abs(r['slope_db_per_s']-expected) for r in slopes(times,levels))
    assert error<.002, error
    return dict(opposite_stereo_phase_preserves_energy=True,expected_slope_db_per_s=expected,
        maximum_slope_error_db_per_s=error)


def measure(x,note):
    assert x.ndim==2 and x.shape[1]==2 and np.isfinite(x).all()
    y,at=aligned(x);f0=440*2**((note-69)/12)
    section=y[round(.05*RATE):round(.5*RATE)];win=np.hanning(len(section))
    size=131072;hz=np.fft.rfftfreq(size,1/RATE)
    spectrum=np.mean(abs(np.fft.rfft(section*win[:,None],n=size,axis=0))**2,axis=1)
    partials=[]
    for harmonic in (1,2,3):
        bins=np.flatnonzero(abs(hz-harmonic*f0)<.18*f0)
        frequency=float(hz[bins[np.argmax(spectrum[bins])]])
        t,level=envelope(y,frequency,f0)
        early=(t>=.1)&(t<.2);late=(t>=.5)&(t<1.4)
        partials.append(dict(harmonic=harmonic,frequency_hz=frequency,slopes=slopes(t,level),
            fall_from_100_200ms_to_500_1400ms_db=float(level[late].mean()-level[early].mean())))
    stages=[]
    for lo,hi in [(0,.1),(.15,.8),(.9,1.4),(1.5,1.9),(2.3,2.8)]:
        p=np.mean(y[round(lo*RATE):round(hi*RATE)]**2)
        stages.append(dict(window_s=[lo,hi],stereo_rms_db=float(10*np.log10(max(p,1e-30)))))
    return dict(analysis_onset_frame=at,peak=float(abs(x).max()),partials=partials,stages=stages)


def read_wav(path):
    rate,x=wavfile.read(path);assert rate==RATE and x.ndim==2 and x.shape[1]==2
    if np.issubdtype(x.dtype,np.integer):x=x.astype(float)/(-float(np.iinfo(x.dtype).min))
    return x.astype(float)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--probe',type=Path,required=True);p.add_argument('--research-root',type=Path,required=True)
    p.add_argument('--work',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    a=p.parse_args();root=Path(__file__).resolve().parents[1]
    if a.work.exists() or a.report.exists():raise ValueError('fresh output paths required')
    check=self_check();a.work.mkdir(parents=True);a.report.parent.mkdir(parents=True,exist_ok=True)
    paths=['plugins/concert-grand/src/lib.rs','plugins/concert-grand/src/math.rs',
        'crates/rackforge-plugin-sdk/src/lib.rs','crates/rackforge-plugin-sdk/src/portable.rs',
        'tools/concert-grand-release-probe.rs','tools/measure-concert-grand-release-body.py']
    hashes={s:digest(root/s) for s in paths}
    references=a.research_root/'local/concert-grand-identified-body'
    manifest_path=references/'reference-manifest.json';provenance=json.loads(manifest_path.read_text(encoding='utf-8'))
    old_binary=a.research_root/'local/concert-grand-decay/build/renderer.exe'
    expected='a42697f5e87c444105b82b1d833653157229d669f69e48f14a1a5378497e5831'
    assert digest(old_binary)==expected
    report=dict(stage='unchanged release baseline; no candidate fitted',release='0.171.35',research_comparison='0.172.0',
        release_ref='v0.1.32',release_commit=subprocess.check_output(['git','-C',str(root),'rev-parse','v0.1.32']).decode().strip(),
        source_sha256=hashes,probe_sha256=digest(a.probe),research_binary_sha256=expected,
        reference_manifest_sha256=digest(manifest_path),rate_hz=RATE,block_frames=256,warmup_frames=RATE//5,
        duration_s=4,note_off_s=2,output_gain=1.,per_note_normalization=False,self_check=check,
        alignment='Analysis only: first 1 ms stereo energy block above -40 dB of first-second peak; raw audio untouched',
        reference_scope='Previously used Steinway ORTF corpus, not new or blind validation. Recordings do not uniquely identify mechanics.',
        cases=[])
    started=time.perf_counter()
    for note in (48,57,60,63,72,84):
        for velocity in (60,110):
            key=f'{note}:{velocity}';folder=a.work/f'{note}-{velocity}';folder.mkdir()
            events=folder/'events.txt';events.write_text(f'0 144 {note} {velocity}\n{2*RATE} 128 {note} 64\n',encoding='utf-8',newline='\n')
            raw=folder/'release.f32'
            command=[str(a.probe.resolve()),str(events.resolve()),str(raw.resolve()),str(RATE),str(4*RATE),'256','concert']
            native=json.loads(subprocess.check_output(command,text=True))
            x=np.fromfile(raw,dtype='<f4').reshape(-1,2).astype(float)
            assert len(x)==4*RATE and np.isfinite(x).all() and abs(x).max()<1.
            research_folder=folder/'research-0.172';research_folder.mkdir()
            env={k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
            env.update(CG_EVENT_FILE=str(events.resolve()),CG_TOTAL_FRAMES=str(4*RATE),CG_RENDER_DIR=str(research_folder.resolve()),
                CG_PRESET='concert',CG_RATE=str(RATE),RUST_MIN_STACK='33554432')
            log=research_folder/'render.log'
            with log.open('x',encoding='utf-8') as stream:
                subprocess.run([str(old_binary.resolve()),'tests::render_reference_wavs','--exact','--ignored','--nocapture','--test-threads=1'],
                    env=env,stdout=stream,stderr=subprocess.STDOUT,check=True)
            old_raw=research_folder/'performance.f32';old=np.fromfile(old_raw,dtype='<f4').reshape(-1,2).astype(float)
            assert len(old)==4*RATE
            ref=references/'reference'/f'{note}-{velocity}.wav';meta_path=ref.with_suffix('.json')
            meta=json.loads(meta_path.read_text(encoding='utf-8'));assert meta['source']==provenance['sources'][key]
            assert meta['source']['transposition']==0
            assert all(meta[k]==provenance[k] for k in ('mapping_sha256','sfz_sha256','dll_sha256'))
            assert digest(meta['source']['file'])==meta['source']['sha256']
            row=dict(note=note,velocity=velocity,native=native,command=command,events_sha256=digest(events),
                release_raw_path=str(raw.resolve()),release_raw_sha256=digest(raw),research_raw_path=str(old_raw.resolve()),
                research_raw_sha256=digest(old_raw),reference_path=str(ref.resolve()),reference_sha256=digest(ref),
                reference_metadata=meta,reference_metadata_sha256=digest(meta_path),
                release=measure(x,note),research=measure(old,note),reference=measure(read_wav(ref),note))
            report['cases'].append(row)
            print(key, 'H1 fall release/research/reference',*[round(row[k]['partials'][0]['fall_from_100_200ms_to_500_1400ms_db'],2)
                for k in ('release','research','reference')],flush=True)
    assert all(digest(root/s)==h for s,h in hashes.items())
    report['sources_unchanged']=True;report['total_seconds']=time.perf_counter()-started
    a.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8',newline='\n')
    print('DONE',round(report['total_seconds'],2),'seconds',flush=True)


if __name__=='__main__':main()
