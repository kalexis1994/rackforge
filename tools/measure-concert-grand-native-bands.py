"""Native contact/capture/coast attribution against refined temporal controls.

Unchanged C4/C5/84 physics, board and rank-192 air. Source-window pressure,
rectangular and Hann, with a fixed microphone operator. This is not a new
WAV, full-phrase certificate or Steinway fit. Persist only small spectra.
"""
import argparse, importlib.util, json, time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_shared_strings import SharedHammers,SharedContact
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedResidues,PackedContact,PackedCoast
from concert_grand_cell_audio import kernel_transfer
from concert_grand_window_spectra import band_norms,hann_acceleration_spectrum
from concert_grand_band_diagnostics import (frequency_grid,pressure_spectra,
    capture,compare_spectra,native_run,BANDS)

spec=importlib.util.spec_from_file_location('native_band_audit',Path(__file__).with_name('measure-concert-grand-shared-kernels.py'))
audit=importlib.util.module_from_spec(spec); spec.loader.exec_module(audit)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--spectra',type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists() or args.spectra.exists(): raise ValueError('preserve completed evidence')
    start=time.perf_counter(); vectors={}; results={}
    names=[Path(__file__).name,'concert_grand_band_diagnostics.py','concert_grand_uniform_spectra.py',
           'concert_grand_window_spectra.py','concert_grand_cell_audio.py',
           'concert_grand_shared_strings.py','concert_grand_shared_kernels.py',
           'concert_grand_exponential_contact.py','concert_grand_packed_air.py',
           'render-concert-grand-shared-phrase.py','concert_grand_audition_air.py']
    report=dict(stage='construction',all_gates_passed=False,approved_for_audition=False,
                complete_polyphonic_temporal_convergence=False,
                scope='Cold C4/C5 at velocity 84: 2ms source windows, all dampers open. '
                      'Coast 8ms starts from the identical separated fine C4 state. '
                      'Not full microphone history, full phrase, keyboard, room or identified piano.',
                criteria=dict(band_relative=.01,hermite_oracle_relative=1e-10,
                              band_energy_grid_relative=.001,band_error_grid_absolute=.0001),
                source_sha256={n:digest(Path(__file__).with_name(n)) for n in names},
                cases=[],comparisons=[])
    def save():
        report['seconds']=time.perf_counter()-start
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
        if vectors:
            args.spectra.parent.mkdir(parents=True,exist_ok=True)
            np.savez_compressed(args.spectra,**vectors)
    save(); modal,geometry,air,kernel,paths=audit.frozen.prepare(report,save)
    f=frequency_grid(12.5); transfer=kernel_transfer(kernel,128000,f)
    report['frequency_grid']=dict(points=len(f),maximum_spacing_hz=float(np.diff(f).max()),
                                  coarse_points=len(f[::2]),bands=BANDS)
    vectors['frequency_hz']=f
    original,_=audit.frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    midpoint=SharedContact(system,air); residues=PackedResidues(air.wave)
    carrier=PackedContact(system,air,residues); coast=PackedCoast(system,air,residues)
    n=system.string.size; omega=np.sqrt(coast.oscillator_functions(1/128000)[3])
    def mechanical(state): return np.r_[omega*coast.project(state[0][:n]),state[1][:n]]
    def execute(label,pilot,initial,duration,rate):
        report['stage']=label; save()
        final,trace,stats=native_run(pilot,initial,duration,rate)
        before=time.perf_counter(); native=pressure_spectra(trace,f,transfer)
        sampled=capture(trace)
        captured=native if sampled is trace else pressure_spectra(sampled,f,transfer)
        prefix=f'case{len(report["cases"]):02}'
        keys={}
        for window in ['rectangular','hann']:
            native_key=prefix+'_'+window+'_native'; vectors[native_key]=native[window]
            captured_key=native_key if captured is native else prefix+'_'+window+'_capture128'
            if captured is not native: vectors[captured_key]=captured[window]
            keys[window]=dict(native=native_key,capture128=captured_key)
        # Different per-segment moment implementation, actual stored spacings.
        indices=[int(np.flatnonzero(f==hz)[0]) for hz in [6000.,12000.]]
        oracle=hann_acceleration_spectrum(*trace,f[indices])
        oracle_pressure=np.einsum('fcm,fm->fc',transfer[indices],oracle)
        oracle_error=audit.relative(native['hann'][indices],oracle_pressure)
        grid_errors={}
        for window,P in native.items():
            fine=band_norms(f,P,BANDS); coarse_grid=band_norms(f[::2],P[::2],BANDS)
            grid_errors[window]={k:abs(np.sqrt(coarse_grid[k]/v)-1) for k,v in fine.items()}
        stats.update(label=label,spectrum_keys=keys,spectral_seconds=time.perf_counter()-before,
                     hann_hermite_oracle_relative=oracle_error,
                     hann_hermite_oracle_passed=bool(oracle_error<report['criteria']['hermite_oracle_relative']),
                     native_band_energy_grid_relative=grid_errors,
                     native_band_energy_grid_passed=bool(max(v for g in grid_errors.values() for v in g.values())<report['criteria']['band_energy_grid_relative']))
        report['cases'].append(stats)
        result=dict(final=final,native=native,capture128=captured,stats=stats)
        results[label]=result; save()
        print('CASE',label,rate,stats['seconds'],'oracle',oracle_error,'grid',grid_errors,flush=True)
        return result
    def compare(label,item,reference,kind='native',reference_kind='native',state_metrics=True):
        row=dict(label=label,actual_case=item['stats']['label'],reference_case=reference['stats']['label'],
                 actual_source=kind,reference_source=reference_kind,windows={})
        for window in ['rectangular','hann']:
            a=item[kind][window]; b=reference[reference_kind][window]
            metrics=compare_spectra(f,a,b); coarse=compare_spectra(f[::2],a[::2],b[::2])
            differences=[abs(x['relative']-y['relative']) for x,y in zip(metrics['bands'],coarse['bands'])]
            metrics.update(grid_error_absolute=differences,
                           grid_error_passed=max(differences)<report['criteria']['band_error_grid_absolute'])
            row['windows'][window]=metrics
        if state_metrics:
            a=item['final']; b=reference['final']
            row.update(mechanical_relative=audit.relative(mechanical(a),mechanical(b)),
                       wave_relative=audit.relative(a[2],b[2]),relaxation_relative=audit.relative(a[3],b[3]))
        report['comparisons'].append(row); save()
        print('COMPARE',label,'Hann bands',[(b['lower_hz'],b['upper_hz'],b['relative']) for b in row['windows']['hann']['bands']],flush=True)
    separated=None
    for key in [60,72]:
        initial=midpoint.initial_state(0.); initial,work=system.launch(initial,key,5*(84/124)**1.5)
        prefix=f'key{key}_velocity84'; report.setdefault('launch_work_j',{})[prefix]=work
        refs=[execute(prefix+'_midpoint'+str(rate),midpoint,initial,.002,rate) for rate in [768000,1536000,3072000]]
        compare(prefix+'_midpoint768_to3072',refs[0],refs[2])
        compare(prefix+'_reference1536_to3072',refs[1],refs[2])
        candidates=[]
        for rate in [128000,256000]:
            item=execute(prefix+'_carrier'+str(rate),carrier,initial,.002,rate)
            compare(prefix+'_carrier'+str(rate)+'_to3072',item,refs[2]); candidates.append(item)
        compare(prefix+'_render_capture_comparison',candidates[0],refs[0],'capture128','capture128')
        for item in refs+candidates:
            compare(item['stats']['label']+'_capture_effect',item,item,'capture128','native',False)
        if key==60:
            separated=refs[-1]['final']
    if separated is None or system.compression(separated[0])>=-1e-12: raise ValueError('separated C4 required')
    report['coast_common_state']=dict(physical_time_s=.002,prior_midpoint_rate_hz=3072000,
                                    all_dampers_open=True,preceding_history_converged=False)
    coasts=[execute('C4_coast'+str(rate),coast,separated,.008,rate) for rate in [16000,32000,64000,128000]]
    for item in coasts[:-1]: compare(item['stats']['label']+'_to128k',item,coasts[-1])
    for item in coasts: compare(item['stats']['label']+'_capture_effect',item,item,'capture128','native',False)
    report['observer_controls_passed']=all(c['hann_hermite_oracle_passed'] and c['native_band_energy_grid_passed'] for c in report['cases'])
    report['comparison_grid_controls_passed']=all(w['grid_error_passed'] for c in report['comparisons'] for w in c['windows'].values())
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==h for k,h in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256']
    report['stage']='completed native contact capture and coast band attribution'; save()
    report['spectral_data']=dict(path=str(args.spectra.resolve()),sha256=digest(args.spectra),bytes=args.spectra.stat().st_size,
                                contents='Frequency grid and compact complex pressure vectors only; no states, WAV or duplicate R.')
    args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    print('NATIVE BAND AUDIT COMPLETED',args.report,flush=True)


if __name__=='__main__': main()
