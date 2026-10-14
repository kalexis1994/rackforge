"""Verify MIDI provenance, native traces, common gain and produced audition WAVs."""
import argparse
import hashlib
import json
from pathlib import Path
import numpy as np
from scipy.io import wavfile


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--measurement',type=Path,required=True);parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.report.exists():raise ValueError('fresh verification path required')
    d=json.loads(args.measurement.read_text(encoding='utf-8'))
    for name,sha in d['source_sha256'].items():
        if digest(root/'tools'/name)!=sha:raise ValueError('measured source changed: '+name)
    for prefix in ['midi','plan','body_report','pressure_report']:
        if digest(d[prefix+'_path'])!=d[prefix+'_sha256']:raise ValueError(prefix+' changed')
    if digest(d['pressure_coefficients'])!=d['pressure_coefficients_sha256']:raise ValueError('pressure coefficients changed')
    body=json.loads(Path(d['body_report_path']).read_text(encoding='utf-8'))
    if digest(body['model_path'])!=d['body_model_sha256']:raise ValueError('body model changed')
    planning=json.loads(Path(d['plan_path']).read_text(encoding='utf-8'))
    plan=next(c['plan'] for c in planning['cases'] if c['plan']['midi_sha256']==d['midi_sha256'])
    window=d['excerpt'];first=next(e['tick'] for e in plan['events'] if e['kind']=='on')
    end=first+round(window['cut_s']*plan['contact_rate_hz'])
    original=[dict(e,time_s=(e['tick']-first)/plan['contact_rate_hz']) for e in plan['events'] if first<=e['tick']<end]
    if original!=window['original_events']:raise ValueError('original MIDI events altered')
    expected_attacks=sum(e['kind']=='on' for e in original);checks=[]
    soft_values=[e['value'] for e in window['events'] if e['kind']=='cc' and e['key']==67]
    first_wire_unstruck=bool(soft_values) and all(v==127 for v in soft_values)
    for variant in d['variants']:
        attacks=0
        for voice in variant['voices']:
            for prefix in ['result','fixture','events','loss','force']:
                if prefix=='force' and not voice.get('force_retained',True):continue
                if digest(voice[prefix+'_path'])!=voice[prefix+'_sha256']:raise ValueError('native artifact changed')
            if 'audio_force_path' in voice and digest(voice['audio_force_path'])!=voice['audio_force_sha256']:
                raise ValueError('compact audio force changed')
            for segment in voice['contact_segments']:
                if digest(segment['path'])!=segment['sha256']:raise ValueError('contact trace changed')
            data=json.loads(Path(voice['result_path']).read_text(encoding='utf-8'))
            if not np.isfinite(np.r_[data['q'],data['v']]).all():raise ValueError('nonfinite final state')
            key=voice['key'];expected=sum(e['kind']=='on' and e['key']==key for e in original)
            if 'undamped_from_midi' in d:
                scale=int(key<d['undamped_from_midi'])
                if [a for a in voice['command'] if a.startswith('--damper-scale=')]!=[f'--damper-scale={scale}']:
                    raise ValueError('voice damper differs from declared register')
                if 'damper_scale' in voice and voice['damper_scale']!=scale:
                    raise ValueError('voice damper scale metadata differs')
            declared=[]
            for e in window['events']:
                if e['kind'] in ('on','off') and e['key']==key:kind=0 if e['kind']=='on' else 1
                elif e['kind']=='cc' and e['key'] in (64,67):kind=e['key']
                else:continue
                declared.append([e['time_s'],kind,e['value']])
            parsed=np.loadtxt(voice['events_path'],ndmin=2)
            # The producer serializes seconds with .16g. Compare that exact
            # representation: its final digit can differ by >1e-14 s once
            # timestamps exceed 100 seconds, without changing a native tick.
            serialized=np.asarray([[float(f'{t:.16g}'),kind,value] for t,kind,value in declared])
            np.testing.assert_array_equal(parsed,serialized)
            if data['attacks']!=expected or len(data['launch_work_j'])!=expected:raise ValueError('strike count changed')
            if data['felt_heat_j']<0 or data['peak_contact_force_n']<=0:raise ValueError('felt control failed')
            # This excerpt starts at full soft pedal, so the unstruck first wire
            # must remain exactly at rest. It is retained in the full state.
            if first_wire_unstruck and not np.array_equal(np.asarray(data['q'])[:384],np.zeros(384)):
                raise ValueError('unstruck wire was excited')
            target=int(np.floor(window['duration_s']*data['coast_rate_hz']+.5))*(data['capture_rate_hz']//data['coast_rate_hz'])
            if data['capture_frames']!=target:raise ValueError('native capture duration changed')
            if voice.get('force_retained',True) and Path(voice['force_path']).stat().st_size!=8*target:
                raise ValueError('native force trace changed')
            if data.get('discarded_transverse_energy_j',0.)>data.get('sleep_threshold_j',0.)*data.get('sleep_episodes',0):
                raise ValueError('sleep energy threshold exceeded')
            attacks+=expected
        if attacks!=expected_attacks:raise ValueError('piece strikes omitted')
        for prefix in ['body_input','raw_pressure','wav']:
            if digest(variant[prefix+'_path'])!=variant[prefix+'_sha256']:raise ValueError('audio artifact changed')
        raw=np.fromfile(variant['raw_pressure_path'],dtype='<f8').reshape(-1,2)
        rate,audio=wavfile.read(variant['wav_path'])
        expected=np.vstack([np.zeros((4800,2)),raw*d['common_listening_gain_per_pa']]).astype(np.float32)
        if rate!=48000 or not np.array_equal(audio,expected):raise ValueError('WAV differs from declared pressure/gain')
        if not np.isfinite(audio).all() or abs(audio).max()>.800001 or np.linalg.norm(audio)==0:raise ValueError('invalid audio level')
        if len(audio)!=4800+round(window['duration_s']*48000):raise ValueError('audio duration changed')
        checks.append(dict(nonlinear_transverse=variant['nonlinear_transverse'],strikes=attacks,frames=len(audio),
            rate_hz=rate,stereo=True,float32=True,common_gain_exact=True,peak=float(abs(audio).max()),
            render_wall_seconds=variant['render_wall_seconds'],wav_path=variant['wav_path']))
    report=dict(measurement_path=str(args.measurement.resolve()),measurement_sha256=digest(args.measurement),
        verifier_sha256=digest(__file__),all_retained_artifact_hashes_passed=True,original_MIDI_events_preserved=True,
        no_clipping=True,common_gain_verified=True,body_feedback=False,complete_reciprocal_piano=False,
        perceptual_quality_decided_by_listener=True,variants=checks)
    args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print(json.dumps(checks,indent=2))


if __name__=='__main__':main()
