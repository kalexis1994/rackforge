"""Exact RAM contraction audit, then a complete shared physical phrase.

Compare the SAME accepted finite WAV reference at identical events/gain.
This tests reproduction of a coarse temporal control, not convergence against
continuous truth or the full-air operator. Preserve old WAVs and all failures.
"""
import argparse, cProfile, importlib.util, json, pstats, time
from pathlib import Path
import numpy as np
from scipy.io import wavfile
from scipy.signal import resample_poly
from concert_grand_wave_cache import digest
from concert_grand_shared_strings import SharedHammers, SharedDampers
from concert_grand_shared_kernels import BatchedStrings, BatchedCoast
from concert_grand_exponential_contact import ExponentialContact
from concert_grand_packed_air import PackedResidues, PackedContact, PackedCoast
from concert_grand_integrated_phrase import trajectory
from concert_grand_cell_audio import pressure_fft

spec=importlib.util.spec_from_file_location('integrated_audit',Path(__file__).with_name('measure-concert-grand-shared-kernels.py'))
audit=importlib.util.module_from_spec(spec); spec.loader.exec_module(audit)
DOC=Path(__file__).resolve().parents[1]/'docs/concert-grand-physics-roadmap-2026-10-02'


def compare_audio(audio, reference, rate):
    if audio.shape != reference.shape: raise ValueError('same waveform duration/channels required')
    error=audit.relative(audio.astype(float),reference.astype(float))
    a=np.fft.rfft(audio.astype(float),axis=0); b=np.fft.rfft(reference.astype(float),axis=0)
    f=np.fft.rfftfreq(len(audio),1/rate)
    weights=np.full(len(f),2.); weights[0]=1.
    if len(audio)%2==0: weights[-1]=1.
    energy=np.sum(weights[:,None]*abs(b)**2,axis=1)
    delta=np.sum(weights[:,None]*abs(a-b)**2,axis=1)
    total=float(energy.sum()); bands=[]
    for lo,hi in [(0,60),(60,800),(800,2500),(2500,6000),(6000,12000),(12000,24001)]:
        mask=(f>=lo)&(f<hi); base=float(energy[mask].sum()); diff=float(delta[mask].sum())
        bands.append(dict(lower_hz=lo,upper_hz=hi,reference_energy_share=base/max(total,1e-300),
                          relative_error=float(np.sqrt(diff/max(base,1e-300))),
                          absolute_error_relative_to_full=float(np.sqrt(diff/max(total,1e-300)))))
    return dict(waveform_relative=error,bands=bands,
                waveform_1_percent_passed=error<=.01,
                bands_60_12000_1_percent_passed=all(b['relative_error']<=.01 for b in bands[1:5]))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--audio-dir',type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists(): raise ValueError('preserve previous report')
    files=[args.audio_dir/'Frase-integrada-pedal.wav',args.audio_dir/'Frase-integrada-sin-pedal.wav']
    if any(p.exists() for p in files): raise ValueError('preserve existing WAVs')
    names=[Path(__file__).name,'concert_grand_integrated_phrase.py','concert_grand_packed_air.py',
           'concert_grand_exponential_contact.py','concert_grand_shared_kernels.py',
           'concert_grand_shared_strings.py','concert_grand_damper_control.py',
           'render-concert-grand-shared-phrase.py','concert_grand_audition_air.py']
    start=time.perf_counter()
    report=dict(stage='construction',all_gates_passed=False,approved_for_audition=False,
                complete_polyphonic_temporal_convergence=False,complete_energy_certificate=False,
                sampled_audio_used=False,keys=[60,67,72],wires_per_key=3,
                contact_rate_hz=128000,coast_rate_hz=16000,capture_rate_hz=128000,
                output_rate_hz=48000,pressure_gain_per_pa=.25,lead_s=.1,
                source_sha256={n:digest(Path(__file__).with_name(n)) for n in names},
                kernel_cases=[],cases=[],audio_files=[])
    def save():
        report['seconds']=time.perf_counter()-start
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    references=[]
    for name,pedal in [('frase-bloques-96.json',True),('frase-control-corregido-96.json',False)]:
        path=DOC/'medidas-P6-compartida-2026-10-08'/name
        frozen=json.loads(path.read_text())
        entry=next(x for x in frozen['audio_files'] if ('sin-pedal' not in Path(x['path']).name)==pedal)
        metrics=next(x for x in frozen['cases'] if x['pedal']==pedal)
        wav=Path(entry['path'])
        if digest(wav)!=entry['sha256']: raise ValueError('changed accepted reference WAV')
        references.append(dict(pedal=pedal,path=wav,sha256=entry['sha256'],report=path,
                               report_sha256=digest(path),metrics=metrics))
    report['references']=[{k:str(v) if isinstance(v,Path) else v for k,v in r.items() if k!='metrics'} for r in references]
    save(); modal,geometry,air,kernel,paths=audit.frozen.prepare(report,save)
    original,_=audit.frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    old_contact=ExponentialContact(system,air); old_coast=BatchedCoast(system,air)
    packing_start=time.perf_counter(); residues=PackedResidues(air.wave)
    report['packing']=dict(bytes=residues.matrix.nbytes,seconds=time.perf_counter()-packing_start,
                           shared_by_contact_and_coast=True,disk_cache_written=False)
    new_contact=PackedContact(system,air,residues); new_coast=PackedCoast(system,air,residues)
    dampers=SharedDampers(system.string,system.nh)
    warm_report_path=DOC/'medidas-P6-contacto-portadoras-2026-10-09/contacto-release.json'
    warm_report=json.loads(warm_report_path.read_text()); warm_path=Path(warm_report['warm_state']['path'])
    if digest(warm_path)!=warm_report['warm_state']['sha256']: raise ValueError('changed paired warm capsule')
    for key in ['board_sha256','cache_R_sha256','reduction_basis_sha256']:
        if warm_report[key]!=report[key]: raise ValueError('incompatible warm physics/operator')
    with np.load(warm_path) as data: warm=tuple(data[k].copy() for k in ['q','v','wave','relaxation'])
    report['paired_warm_state']=dict(path=str(warm_path),sha256=digest(warm_path),history_converged=False)
    omega=np.sqrt(old_coast.oscillator_functions(1/128000)[3]); n=system.string.size
    def metric(state): return np.r_[omega*old_coast.project(state[0][:n]),state[1][:n]]
    def pair(label,old,new,state,duration,rate,profile=False):
        report['stage']=label; save()
        # Prepare factors outside the timing comparison, from the same state.
        old.step(audit.clone(state),1/rate,True); new.step(audit.clone(state),1/rate,True)
        profiler=cProfile.Profile() if profile else None
        a,ta,sa=audit.run(old,state,duration,rate)
        b,tb,sb=audit.run(new,state,duration,rate)
        if profiler:
            # Profiling overhead must not inflate the speed comparison.
            profiler.enable()
            audit.run(old,state,min(.002,duration),rate)
            profiler.disable()
        result=dict(label=label,rate_hz=rate,duration_s=duration,
                    reference_seconds=sa['seconds'],packed_seconds=sb['seconds'],speed_ratio=sa['seconds']/sb['seconds'],
                    state_relative=[audit.relative(x,y) for x,y in zip(b,a)],
                    mechanical_relative=audit.relative(metric(b),metric(a)),
                    pressure_relative=audit.relative(pressure_fft(*tb,kernel)[0],pressure_fft(*ta,kernel)[0]),
                    felt_heat_absolute_difference_j=abs(sb['felt_heat_j']-sa['felt_heat_j']),
                    material_heat_absolute_difference_j=abs(sb['board_material_heat_j']-sa['board_material_heat_j']))
        result['equivalence_1e_8_passed']=max(result['state_relative']+[result['mechanical_relative'],result['pressure_relative']])<1e-8
        if profiler:
            result['baseline_profile_duration_s']=min(.002,duration)
            stats=pstats.Stats(profiler)
            rows=sorted(stats.stats.items(),key=lambda item:item[1][3],reverse=True)[:18]
            result['baseline_profile']=[dict(file=Path(k[0]).name,line=k[1],function=k[2],calls=v[1],
                                              own_seconds=v[2],cumulative_seconds=v[3]) for k,v in rows]
        report['kernel_cases'].append(result); save(); print('KERNEL',result,flush=True)
        if not result['equivalence_1e_8_passed']: raise ValueError('reject changed packed dynamics')
        return result
    coast_pair=pair('living_coast_packed_16k',old_coast,new_coast,warm,.008,16000,True)
    pair('living_coast_packed_64k',old_coast,new_coast,warm,.002,64000)
    cold=old_contact.initial_state(0.); cold,_=system.launch(cold,60,5*(84/124)**1.5)
    pair('contact_packed_128k',old_contact,new_contact,cold,.002,128000)
    use_packed=coast_pair['speed_ratio']>1.
    report['backend']='packed exact RAM residues' if use_packed else 'original batched residues; packing slower'
    contact,coast=(new_contact,new_coast) if use_packed else (old_contact,old_coast)
    save(); args.audio_dir.mkdir(parents=True,exist_ok=True)
    for index,reference in enumerate(references):
        pedal=reference['pedal']; report['stage']='complete integrated phrase pedal '+str(pedal); save()
        def progress(value): report['progress']=value; save()
        final,trace,metrics=trajectory(system,contact,coast,dampers,pedal,progress=progress)
        pressure=pressure_fft(*trace,kernel)[0][:len(trace[0])+len(kernel)-1]
        audio=resample_poly(pressure*.25,3,8,axis=0).astype(np.float32)
        audio=np.vstack([np.zeros((4800,2),np.float32),audio])
        if not np.isfinite(audio).all() or abs(audio).max()>=.98: raise ValueError('nonfinite or clipped fixed gain')
        rate,reference_audio=wavfile.read(reference['path'])
        if rate!=48000 or reference_audio.dtype!=np.float32: raise ValueError('frozen float32 physical audio required')
        metrics.update(compare_audio(audio,reference_audio,rate))
        metrics.update(audio_peak=float(abs(audio).max()),final_rms=float(np.sqrt(np.mean(audio[-4800:].astype(float)**2))),
                       full_trajectory_speed_ratio=reference['metrics']['seconds']/metrics['seconds'],
                       comparison_scope='Reproduction of accepted coarse-16k reference; not a temporal convergence certificate.')
        destination=files[index]
        wavfile.write(destination,48000,audio)
        entry=dict(path=str(destination.resolve()),sha256=digest(destination),frames=len(audio),
                   seconds=len(audio)/48000,pedal=pedal)
        report['cases'].append(metrics); report['audio_files'].append(entry); save()
        print('COMPLETE PHRASE',metrics,entry,flush=True)
        del final,trace,pressure,audio,reference_audio
    report['reference_reproduction_passed']=all(c['waveform_1_percent_passed'] and c['bands_60_12000_1_percent_passed'] for c in report['cases'])
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==h for k,h in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256']
    report['stage']='completed packed-air and full integrated phrase comparison'; save()
    print('INTEGRATION COMPLETED',args.report,flush=True)


if __name__=='__main__': main()
