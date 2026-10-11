"""One cold live C4/84 with physical note-off, then local release refinement.

Reuse the frozen three-key/nine-string physical bank. No samples, tonal
correction, output fade or new large cache. WAV only in SHARED, evidence
in the repo. Local precision controls do not certify a complete instrument.
"""
import argparse,importlib.util,json,time
from pathlib import Path
import numpy as np
from scipy.io import wavfile
from scipy.signal import resample_poly
from concert_grand_wave_cache import digest
from concert_grand_shared_strings import SharedHammers,SharedDampers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedResidues,PackedContact,PackedCoast
from concert_grand_note_trace import NoteTrace,release_segment
from concert_grand_cell_audio import pressure_fft,kernel_transfer
from concert_grand_band_diagnostics import frequency_grid,pressure_spectra,compare_spectra

spec=importlib.util.spec_from_file_location('note_integration',Path(__file__).with_name('render-concert-grand-integrated-phrase.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
DOC=base.DOC


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--audio-dir',type=Path,required=True)
    args=parser.parse_args()
    destination=args.audio_dir/'C4-84-sin-pedal-128k.wav'
    spectra_path=args.report.parent/'release.npz'
    if args.report.exists() or destination.exists() or spectra_path.exists():
        raise ValueError('preserve existing evidence/audio')
    prior_path=DOC/'medidas-P6-bandas-nativas-2026-10-09/coast-refinado.json'
    prior=json.loads(prior_path.read_text())
    names=dict(prior['source_sha256'])
    for n,s in names.items():
        if digest(Path(__file__).with_name(n))!=s:raise ValueError('frozen source changed: '+n)
    names.update({n:digest(Path(__file__).with_name(n)) for n in [
        Path(__file__).name,'concert_grand_note_trace.py','concert_grand_integrated_phrase.py',
        'render-concert-grand-integrated-phrase.py','concert_grand_damper_control.py']})
    start=time.perf_counter();report=dict(stage='construction',all_gates_passed=False,
        approved_for_audition=False,complete_polyphonic_temporal_convergence=False,
        complete_energy_certificate=False,diagnostic_listening=True,sampled_audio_used=False,
        keys=[60,67,72],wires_per_key=3,key=60,velocity=84,pedal=False,
        contact_rate_hz=256000,coast_rate_hz=128000,capture_rate_hz=128000,
        output_rate_hz=48000,pressure_gain_per_pa=.25,lead_s=.1,
        schedule=[(0.,'on',60,84),(.16,'off',60,0)],
        initial_duration_s=.4,maximum_duration_s=.8,
        tail_rms_relative_limit=.01,criteria=dict(prior['criteria'],release_replay_relative=1e-9),
        reference_report_sha256=digest(prior_path),source_sha256=names,release_cases=[])
    def save():
        report['seconds']=time.perf_counter()-start
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();modal,geometry,air,kernel,paths=base.audit.frozen.prepare(report,save)
    original,_=base.audit.frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    residues=PackedResidues(air.wave)
    contact=PackedContact(system,air,residues);coast=PackedCoast(system,air,residues)
    dampers=SharedDampers(system.string,system.nh)
    report.update(mechanical_coordinates=system.size,air_states=air.states,
                  packed_residue_ram_bytes=residues.matrix.nbytes,new_disk_cache=False)
    tracker=NoteTrace(256000,[.16,.184,.192,.4])
    def progress(value):report['progress']=value;save()
    report['stage']='complete C4 with physical release';save()
    final,trace,metrics=tracker.run(system,contact,coast,dampers,pedal=False,duration=.4,
                                  coast_rate=128000,schedule=report['schedule'],progress=progress)
    report['trajectory']=metrics
    if metrics['coast_steps']==0 or metrics['contact_steps']+2*metrics['coast_steps']!=102400:
        raise ValueError('incorrect contact/coast routing')
    report['accepted_checkpoint_times_s']=sorted(tracker.states)
    expected=metrics['external_head_work_j']
    balance=metrics['final_mechanical_j']-expected+metrics['felt_heat_j']+metrics['material_heat_j']+metrics['damper_heat_j']+tracker.air_work_j
    report['mechanical_work_ledger']=dict(air_port_work_j=tracker.air_work_j,
        residual_j=balance,relative=abs(balance)/expected,scope='Mechanical/felt/damper/air-port work; radiation ledger deferred.')
    duration=.4;extensions=[]
    while True:
        report['stage']='pressure reconstruction';save()
        pressure=pressure_fft(*trace,kernel)[0][:len(trace[0])+len(kernel)-1]
        audio=resample_poly(pressure*.25,3,8,axis=0).astype(np.float32)
        if not np.isfinite(audio).all() or abs(audio).max()>=.98:raise ValueError('nonfinite or clipped fixed-gain audio')
        held_rms=float(np.sqrt(np.mean(audio[2400:7200].astype(float)**2)))
        tail_rms=float(np.sqrt(np.mean(audio[-960:].astype(float)**2)))
        tail_relative=tail_rms/max(held_rms,1e-300)
        report['tail_check']=dict(duration_s=duration,held_rms=held_rms,last20ms_rms=tail_rms,
            relative=tail_relative,passed=bool(tail_relative<=report['tail_rms_relative_limit']))
        save()
        if report['tail_check']['passed'] or duration>=.8:break
        # Continue the SAME living state; no re-render, zeroing or duplicated note.
        report['stage']='continue physical tail';save()
        next_state,extra,stats=release_segment(coast,dampers,final,duration,.2,128000)
        trace=(np.r_[trace[0],duration+extra[0][1:]],
               np.vstack([trace[1],extra[1][1:]]),np.vstack([trace[2],extra[2][1:]]))
        final=next_state;duration=round(duration+.2,10);extensions.append(stats)
        del pressure,audio,extra
    report['tail_extensions']=extensions
    report['physical_duration_s']=duration
    # Delivery is a listening diagnostic even if the bounded tail test fails.
    # Preserve that failure; no artificial envelope or normalisation.
    audio=np.vstack([np.zeros((4800,2),np.float32),audio])
    args.audio_dir.mkdir(parents=True,exist_ok=True);wavfile.write(destination,48000,audio)
    report['audio_file']=dict(path=str(destination.resolve()),sha256=digest(destination),
        bytes=destination.stat().st_size,frames=len(audio),seconds=len(audio)/48000,
        peak=float(abs(audio).max()),finite=True,format='FLOAT32 stereo',no_output_fade=True)
    print('NOTE AUDIO READY',report['audio_file'],report['tail_check'],flush=True);save()
    # Compare the first 120ms after the same launch, before either note-off.
    old_path=Path('C:/SHARED/Piano-contacto-integrado-2026-10-09/Frase-integrada-sin-pedal.wav')
    old_report_path=DOC/'medidas-P6-frase-integrada-2026-10-09/integracion.json'
    # Locate the report by its actual registered audio entry, avoiding guessed names.
    for candidate in (DOC/'medidas-P6-frase-integrada-2026-10-09').glob('*.json'):
        old_report=json.loads(candidate.read_text())
        entry=next((e for e in old_report.get('audio_files',[]) if Path(e['path'])==old_path),None)
        if entry:
            if digest(old_path)!=entry['sha256']:raise ValueError('accepted audio changed')
            sr,old_audio=wavfile.read(old_path)
            if sr!=48000 or old_audio.dtype!=np.float32:raise ValueError('same accepted format required')
            prefix=round(.22*48000)
            report['prior16k_prefix_comparison']=dict(base.compare_audio(audio[:prefix],old_audio[:prefix],48000),
                path=str(old_path),sha256=entry['sha256'],report_sha256=digest(candidate),
                physical_prefix_s=.12,scope='Same physical launch/events before note-off. Different temporal discretization, not Steinway fidelity.')
            break
    del audio,pressure
    # Refine the actual closing ramp, not a generic all-open warm tuple.
    initial=tracker.states[.184];end128=tracker.states[.192]
    i=round(.184*128000);j=round(.192*128000)
    actual_trace=(trace[0][i:j+1]-.184,trace[1][i:j+1],trace[2][i:j+1])
    # Rebuild exact uniform local times to remove subtraction roundoff.
    actual_trace=(np.arange(j-i+1)/128000,*actual_trace[1:])
    f=frequency_grid(6.25);H=kernel_transfer(kernel,128000,f)
    actual_pressure=pressure_spectra(actual_trace,f,H)
    vectors={'frequency_hz':f,**{'actual128_'+w:x for w,x in actual_pressure.items()}}
    n=system.string.size;omega=np.sqrt(coast.oscillator_functions(1/128000)[3])
    def mechanical(s):return np.r_[omega*coast.project(s[0][:n]),s[1][:n]]
    def relative(x,y):return float(np.linalg.norm(x-y)/max(np.linalg.norm(y),1e-300))
    cases=[]
    for rate in [64000,128000,256000]:
        report['stage']='release control '+str(rate);save()
        end,T,stats=release_segment(coast,dampers,initial,.184,.008,rate)
        P=pressure_spectra(T,f,H)
        for w,x in P.items():vectors[f'release{rate}_'+w]=x
        if rate==128000:
            replay=dict(states=max(relative(x,y) for x,y in zip(end,end128)),
                        trace=max(relative(x,y) for x,y in zip(T[1:],actual_trace[1:])))
            report['release_replay']=dict(replay,passed=bool(max(replay.values())<1e-9))
            if not report['release_replay']['passed']:raise ValueError('release did not reproduce actual accepted trajectory')
        stats['mechanical_work_relative']=abs(system.energy(*end[:2])-system.energy(*initial[:2])+stats['air_work_j']+stats['material_heat_j']+stats['damper_heat_j'])/system.energy(*initial[:2])
        cases.append((end,P,stats));report['release_cases'].append(stats);save()
    ref,P_ref,_=cases[-1]
    for end,P,stats in cases:
        stats['windows']={w:compare_spectra(f,x,P_ref[w]) for w,x in P.items()}
        stats['state_relative_to256']=dict(mechanical=relative(mechanical(end),mechanical(ref)),
            wave=relative(end[2],ref[2]),relaxation=relative(end[3],ref[3]))
        stats['local_1_percent_passed']=bool(max(stats['state_relative_to256'].values())<.01 and
            all(w['all_bands_1_percent_passed'] for w in stats['windows'].values()))
    np.savez_compressed(spectra_path,**vectors)
    report['release_spectral_data']=dict(path=str(spectra_path.resolve()),sha256=digest(spectra_path),bytes=spectra_path.stat().st_size)
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(n))==s for n,s in names.items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256']
    report['approved_for_audition']=True
    report['stage']='completed C4 physical note, release and diagnostic audio';save()
    print('NOTE RELEASE COMPLETED',args.report,flush=True)


if __name__=='__main__':main()
