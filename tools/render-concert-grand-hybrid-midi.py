"""Short original MIDI audition with live felt, hybrid strings and one-way body.

This is a fresh physical listening pilot, not a sampled-note maquette or a
complete reciprocal piano. Every approximation is recorded with the WAVs.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from scipy.linalg import solve
from scipy.io import wavfile
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_modal_viability import omega
from concert_grand_hybrid_midi import excerpt,voice_events,bridge_force


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--local',type=Path,required=True);parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--audio-dir',type=Path,required=True)
    parser.add_argument('--cut',type=float,default=8.);parser.add_argument('--tail',type=float,default=1.)
    parser.add_argument('--modes',type=int,nargs='+',default=[8],
        help='Nonlinear transverse modes, default: 8. Use --modes 8 12 for comparison.')
    parser.add_argument('--workers',type=int,default=4)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.local.exists() or args.report.exists() or args.audio_dir.exists():raise ValueError('fresh paths required')
    started=time.perf_counter();doc=root/'docs/concert-grand-physics-roadmap-2026-10-02'
    plan_path=doc/'medidas-P6-entrada-MIDI-2026-10-11/plan.json'
    plans=json.loads(plan_path.read_text(encoding='utf-8'))
    plan=next(c['plan'] for c in plans['cases'] if 'campanella' in c['plan']['midi'])
    if digest(plan['midi'])!=plan['midi_sha256']:raise ValueError('preserved MIDI changed')
    window=excerpt(plan,args.cut,args.tail)
    body_report=doc/'medidas-P7-cuerpo-cargado-2026-10-12/residual-161.json'
    body=json.loads(body_report.read_text(encoding='utf-8'))
    if digest(body['model_path'])!=body['model_sha256']:raise ValueError('body model changed')
    pressure_report=doc/'medidas-P7-presion-realtime-2026-10-13/estados-DC.json'
    pressure=json.loads(pressure_report.read_text(encoding='utf-8'))
    if pressure['body_report_sha256']!=digest(body_report):raise ValueError('pressure/body mismatch')
    coefficients=next(r for r in pressure['rates'] if r['rate_hz']==48000)
    if digest(coefficients['coefficient_path'])!=coefficients['coefficient_sha256']:raise ValueError('pressure coefficients changed')
    args.local.mkdir(parents=True);args.audio_dir.mkdir(parents=True);args.report.parent.mkdir(parents=True,exist_ok=True)
    sources=['concert-grand-phase-trajectory.rs','concert-grand-hybrid.rs','concert-grand-hybrid-audition.rs',
        'concert-grand-native-strings.rs','concert-grand-phase-forces.rs','concert-grand-batch-cosine.rs',
        'concert-grand-collocation.rs','concert-grand-phase-profile.rs','concert-grand-observation.rs',
        'concert-grand-pressure-body.rs','render-concert-grand-force-body.rs','concert_grand_hybrid_midi.py',
        'concert_grand_native_strings.py','concert_grand_modal_viability.py','tests/test_concert_hybrid_midi.py',Path(__file__).name]
    hashes={n:digest(root/'tools'/n) for n in sources}
    exe=args.local.resolve()/'strings.exe';body_exe=args.local.resolve()/'body.exe'
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
        str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)],check=True)
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native',
        str(root/'tools/render-concert-grand-force-body.rs'),'-o',str(body_exe)],check=True)
    with np.load(body['model_path']) as b:
        bw=b['omega'];bd=b['damping'];bp=b['ports'];mapping=b['mapping']
    record=dict(stage='offline hybrid MIDI audition',baseline_commit='f42618a8',source_sha256=hashes,
        midi_path=plan['midi'],midi_sha256=plan['midi_sha256'],plan_path=str(plan_path),plan_sha256=digest(plan_path),
        body_report_path=str(body_report),body_report_sha256=digest(body_report),body_model_sha256=body['model_sha256'],
        pressure_report_path=str(pressure_report),pressure_report_sha256=digest(pressure_report),
        pressure_coefficients=coefficients['coefficient_path'],pressure_coefficients_sha256=coefficients['coefficient_sha256'],
        strings_executable_sha256=digest(exe),body_executable_sha256=digest(body_exe),excerpt=window,
        sampled_notes_used=False,eq_or_compression=False,body_feedback=False,
        natural_loss='diagonal weak-load approximation: sigma_n = cross_n^2 * omega_n^2 * Re(Y_key(omega_n))',
        damper='diagonal of the same uniform 0.3L / 0.08L patch, 0.6 N s/m; 30 ms engagement',
        soft_pedal='contact areas [1-u,1,1]; instantaneous prescribed shift, no velocity/volume substitution',
        high_coast_observation_limit_hz=20000,low_readout='cubic Hermite interval acceleration plus linearly interpolated nonlinear boundary gradient',
        coast_contact_switch='same q/v, full nonlinear potential during contact, projected nonlinear potential during coast',
        acoustic_pressure=True,complete_reciprocal_piano=False,full_piano_realtime=False,
        inherited_body_fidelity_failures_preserved=True,parallel_voice_workers=min(args.workers,len(window['keys'])),
        direct_felt_bridge_lever_arm=.125,cold_prefix_controls='settled before the first strike',
        felt_total_stiffness_split_equally_between_wires=True,
        rejected_predecessor_path=str(doc/'medidas-P7-cuerdas-nativas-2026-10-14/audicion-midi.json'),
        rejected_predecessor_sha256=digest(doc/'medidas-P7-cuerdas-nativas-2026-10-14/audicion-midi.json'),variants=[])
    prepared={}
    for key in window['keys']:
        rate=16000*2**max(0,int(np.ceil((key-60)/12)))
        f=fixture(note=key,transverse=96,rate=rate,seed=None,duration=window['duration_s']);f['launch_speeds'][:]=0.
        w=omega(f)[f['axial']:f['axial']+f['transverse']]
        port=bp @ mapping[key-21]
        admittance=np.array([(1j*frequency)*port @ solve(np.diag(bw*bw-frequency*frequency)+1j*frequency*bd,port,
            check_finite=False) for frequency in w])
        if admittance.real.min() < -1e-10:raise ValueError('nonpassive driving admittance')
        n=np.arange(1,97);length,mu=f['parameters'][:2]
        cross=mu*length*(-1.)**(n+1)/(np.pi*n*np.sqrt(mu*length/2))
        sigma=cross**2*w**2*np.maximum(admittance.real,0.)
        if np.any(sigma>=2*w):raise ValueError('overdamped carrier needs another exact propagator')
        path=args.local/f'key-{key}.bin';write_fixture(path,f)
        events=args.local/f'key-{key}.events';events.write_text(''.join(f'{t:.16g} {kind} {value}\n'
            for t,kind,value in voice_events(window,key)),encoding='utf-8',newline='\n')
        loss=args.local/f'key-{key}.loss.f64';sigma.astype('<f8').tofile(loss)
        prepared[key]=(f,path,events,loss)
    for low in args.modes:
        begin=time.perf_counter();frames=round(window['duration_s']*48000);latent=np.zeros((frames,mapping.shape[1]));voices=[]
        def render_voice(item):
            key,(f,inp,events,loss)=item
            label=f'NL{low}-key-{key}';force_path=args.local/(label+'.force.f64');result_path=args.local/(label+'.json')
            command=[str(exe),str(inp.resolve()),'midi-voice',str(low),'16',str(events.resolve()),str(force_path.resolve()),str(loss.resolve())]
            start=time.perf_counter()
            with result_path.open('x',encoding='utf-8') as output:
                subprocess.run(command,stdout=output,text=True,check=True)
            data=json.loads(result_path.read_text(encoding='utf-8'))
            coast=np.fromfile(force_path,dtype='<f8')
            force=bridge_force(coast,data['contact_segments'],data['contact_rate_hz'],data['capture_rate_hz'])
            if len(force)!=frames:raise ValueError('force duration changed')
            expected_attacks=sum(k==0 for _,k,_ in voice_events(window,key))
            if data['attacks']!=expected_attacks or data['felt_heat_j']<0:raise ValueError('event or felt loss mismatch')
            entry=dict(key=key,command=command,result_path=str(result_path.resolve()),result_sha256=digest(result_path),
                fixture_path=str(inp.resolve()),fixture_sha256=digest(inp),events_path=str(events.resolve()),events_sha256=digest(events),
                loss_path=str(loss.resolve()),loss_sha256=digest(loss),force_path=str(force_path.resolve()),force_sha256=digest(force_path),
                contact_segments=[s|{'sha256':digest(s['path'])} for s in data['contact_segments']],
                attacks=data['attacks'],coast_steps=data['coast_steps'],contact_steps=data['contact_steps'],
                coast_rate_hz=data['coast_rate_hz'],capture_rate_hz=data['capture_rate_hz'],contact_rate_hz=data['contact_rate_hz'],
                sigma_min=float(np.fromfile(loss,dtype='<f8').min()),sigma_max=float(np.fromfile(loss,dtype='<f8').max()),
                native_seconds=data['seconds'],voice_wall_seconds=time.perf_counter()-start)
            print(label,'attacks',data['attacks'],'wall s',round(entry['voice_wall_seconds'],2),flush=True)
            return key,entry,force
        # Only independent fixed-bridge voices run in parallel. The common
        # body processes their sum once, in a deterministic key order.
        with ThreadPoolExecutor(max_workers=record['parallel_voice_workers']) as pool:
            for key,entry,force in pool.map(render_voice,prepared.items()):
                latent+=force[:,None]*mapping[key-21];voices.append(entry)
        input_path=args.local/f'NL{low}-body-input.f64';latent.astype('<f8').tofile(input_path)
        raw=args.local/f'NL{low}-pressure.f64'
        command=[str(body_exe),coefficients['coefficient_path'],str(input_path.resolve()),str(mapping.shape[1]),str(raw.resolve())]
        body_data=json.loads(subprocess.check_output(command,text=True));signal=np.fromfile(raw,dtype='<f8').reshape(-1,2)
        record['variants'].append(dict(nonlinear_transverse=low,voices=voices,body_input_path=str(input_path.resolve()),
            body_input_sha256=digest(input_path),raw_pressure_path=str(raw.resolve()),raw_pressure_sha256=digest(raw),
            body=body_data,render_wall_seconds=time.perf_counter()-begin,pressure_peak_pa=float(abs(signal).max())))
    # One listening gain for BOTH variants, preserving all inter-version levels.
    gain=min(.25,.8/max(v['pressure_peak_pa'] for v in record['variants']))
    record['common_listening_gain_per_pa']=gain
    for variant in record['variants']:
        low=variant['nonlinear_transverse'];raw=np.fromfile(variant['raw_pressure_path'],dtype='<f8').reshape(-1,2)
        audio=np.vstack((np.zeros((4800,2)),gain*raw));path=args.audio_dir/f'Campanella-hibrido-NL{low}.wav'
        wavfile.write(path,48000,audio.astype(np.float32))
        rate,verified=wavfile.read(path)
        if rate!=48000 or verified.dtype!=np.float32 or verified.shape!=audio.shape or not np.isfinite(verified).all():raise ValueError('WAV verification failed')
        variant.update(wav_path=str(path.resolve()),wav_sha256=digest(path),audio_duration_s=len(audio)/48000,
            audio_peak=float(abs(verified).max()),audio_rms=float(np.sqrt(np.mean(verified**2))))
    record['sources_unchanged']=all(digest(root/'tools'/n)==sha for n,sha in hashes.items())
    if not record['sources_unchanged']:raise ValueError('measured sources changed')
    record['total_wall_seconds']=time.perf_counter()-started
    record['stage']='rendered and WAV format verified'
    args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print('DONE',round(record['total_wall_seconds'],2),'seconds; shared gain',gain,flush=True)


if __name__=='__main__':
    with threadpool_limits(limits=1):main()
