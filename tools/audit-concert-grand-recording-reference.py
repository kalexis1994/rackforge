"""Separate raw recording measurements from sampler dynamics and release rules."""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.ndimage import gaussian_filter1d
from scipy.signal import find_peaks


def load(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    m=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def stats(values):
    z=np.asarray(values,dtype=float)
    return dict(count=len(z),median=float(np.median(z)),mean_absolute=float(abs(z).mean()),
                min=float(z.min()),max=float(z.max())) if len(z) else None


def modulation(features,reference,support):
    envelope=gaussian_filter1d(np.asarray(features['envelope_10ms_db']),2)
    ref=gaussian_filter1d(np.asarray(reference['envelope_10ms_db']),2)
    # Keep late noise and the PCM16 floor out of the modulation interpretation.
    floor=max(float(ref[5:30].max())-35,-80)
    minima,props=find_peaks(-envelope,prominence=3,distance=12)
    events=[dict(time_s=float(i*.01),prominence_db=float(props['prominences'][j]))
            for j,i in enumerate(minima) if .15<i*.01<min(11.5,support-.1)
            and ref[i]>floor and envelope[i]>floor]
    return dict(kind='Broadband 10ms RMS envelope, smoothed 20ms; recovered minima >=3dB above reference initial minus35dB and -80dBFS. Not individual-string beats or a perceptual roughness score.',
        events=events,counts=[sum(lo<=x['time_s']<hi for x in events) for lo,hi in
                             ((.15,.5),(.5,1.5),(1.5,3.5),(3.5,7.5),(7.5,11.5))])


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--work',type=Path,required=True)
    a=p.parse_args()
    e=load(Path(__file__).with_name('evaluate-concert-grand-full-sound.py'),'record_evaluation')
    report=json.loads((a.output/'evaluation.json').read_text())
    source_rows=[r for r in report['rows'].values() if r['transposition_semitones']==0]
    gains=[]
    for r in source_rows:
        if r['calibration_pair']:
            original,rate=sf.read(r['source_file'],frames=e.RATE*16,always_2d=True)
            native=e.read_model(a.work/f"{r['note']}-{r['velocity']}"/'performance.f32')
            x,_=e.aligned(original); y,_=e.aligned(native)
            lo,hi=round(.05*rate),round(.3*rate)
            gains.append(float(e.db(e.power(x[lo:hi]))-e.db(e.power(y[lo:hi]))))
    gain_db=float(np.median(gains)); gain=10**(gain_db/20)
    raw_report=dict(policy='Unmodified original Stage ORTF recordings, no SFZ velocity gain or onset envelope, no transposition. One separate common gain calibrated on the same 14 pairs. No raw-recording release comparison because original note-off times are unknown.',
        gain_db=gain_db,calibration_pairs=report['calibration_pairs'],rows={},summary={})
    for i,r in enumerate(source_rows):
        original,rate=sf.read(r['source_file'],frames=e.RATE*16,always_2d=True)
        assert rate==e.RATE
        support=min(11.5,r['source_seconds']-e.onset(original)/rate)
        original=np.pad(original,((0,max(0,e.RATE*16-len(original))),(0,0)))
        native=e.read_model(a.work/f"{r['note']}-{r['velocity']}"/'performance.f32')*gain
        fr=e.features(original,r['note']); fm=e.features(native,r['note'],fr['centers_hz'])
        delta=e.compare(fr,fm,support)
        for d,rr,mm in zip(delta,fr['stages'],fm['stages']):
            d['subfundamental_relative_total_error_db']=(mm['subfundamental_db']-mm['total_db'])-(rr['subfundamental_db']-rr['total_db'])
            d['subfundamental_reliable']=rr['subfundamental_db']>max(x['total_db'] for x in fr['stages'][:4])-55
            d['band_reliable']=[value>max(x['total_db'] for x in fr['stages'][:4])-55 for value in rr['bands_db']]
        raw_report['rows'][f"{r['note']}:{r['velocity']}"]=dict(note=r['note'],velocity=r['velocity'],
            source_file=r['source_file'],source_sha256=r['source_sha256'],reference=fr,model=fm,differences=delta,
            source_support_seconds=support,modulation=dict(reference=modulation(fr,fr,support),model=modulation(fm,fr,support)))
        if (i+1)%80==0:
            print('RAW RECORDING',i+1,'/480',flush=True)
    z=raw_report['rows']
    dynamic=[]
    per_note={}
    for n in e.ROOTS:
        weak,strong=z[f'{n}:28'],z[f'{n}:124']
        dr=strong['reference']['stages'][2]['total_db']-weak['reference']['stages'][2]['total_db']
        dm=strong['model']['stages'][2]['total_db']-weak['model']['stages'][2]['total_db']
        dynamic.append(dm-dr)
        per_note[str(n)]=dict(original_range_db=dr,model_range_db=dm,error_db=dm-dr)
    raw_report['summary']['dynamics_28_to_124']=dict(aggregate=stats(dynamic),per_note=per_note,
        sampler_comparison=json.loads((a.output/'summary.json').read_text())['dynamics_error_stats'],
        interpretation='The SFZ player extends layer dynamics by 4dB plus within-layer gain. Its range mismatch must not be attributed entirely to the physical model.')
    raw_report['summary']['registers']={}
    for name,low,high in [('bajos',21,47),('medios',48,71),('agudos',72,108)]:
        selected=[r for r in z.values() if low<=r['note']<=high]
        raw_report['summary']['registers'][name]=dict(
            initial_level_delta=stats([r['differences'][2]['level_delta_db'] for r in selected]),
            body_level_delta=stats([r['differences'][4]['level_delta_db'] for r in selected if r['differences'][4]['recorded_support']]),
            strong_early_subfundamental_relative_total_error=stats([r['differences'][1]['subfundamental_relative_total_error_db'] for r in selected
                if r['velocity']>=100 and r['differences'][1]['subfundamental_reliable']]),
            late_modulation_reference=sum(sum(r['modulation']['reference']['counts'][2:]) for r in selected),
            late_modulation_model=sum(sum(r['modulation']['model']['counts'][2:]) for r in selected))
    (a.output/'raw-recordings.json').write_text(json.dumps(raw_report,indent=2),encoding='utf-8')
    print(json.dumps(dict(gain_db=gain_db,summary=raw_report['summary']),indent=2))


if __name__=='__main__':
    main()
