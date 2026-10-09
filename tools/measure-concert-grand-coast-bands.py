"""Refine failed coast grids and isolate Hermite reconstruction from dynamics.

Same cold C4/84 history and three-key physics as the native band audit.
All rates share its regenerated 3072 kHz separated state. No new WAV.
"""
import argparse, importlib.util, json, time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_shared_strings import SharedContact, SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedCoast, PackedResidues
from concert_grand_cell_audio import kernel_transfer
from concert_grand_band_diagnostics import (native_run, capture, frequency_grid,
    pressure_spectra, compare_spectra, BANDS)
from concert_grand_window_spectra import band_norms, hann_acceleration_spectrum

spec=importlib.util.spec_from_file_location('coast_native_audit',
    Path(__file__).with_name('measure-concert-grand-native-bands.py'))
base=importlib.util.module_from_spec(spec); spec.loader.exec_module(base)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-report',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--spectra',type=Path,required=True)
    parser.add_argument('--state',type=Path,required=True)
    args=parser.parse_args()
    if any(p.exists() for p in [args.report,args.spectra,args.state]):
        raise ValueError('preserve measured evidence')
    prior=json.loads(args.base_report.read_text(encoding='utf-8'))
    if not prior['stage'].startswith('completed') or not prior['source_and_cache_unchanged']:
        raise ValueError('completed unchanged native audit required')
    for name,sha in prior['source_sha256'].items():
        if digest(Path(__file__).with_name(name))!=sha: raise ValueError('base source changed: '+name)
    old_spectra=Path(prior['spectral_data']['path'])
    if digest(old_spectra)!=prior['spectral_data']['sha256']: raise ValueError('base spectra changed')
    names=list(prior['source_sha256'])+[Path(__file__).name]
    start=time.perf_counter()
    report=dict(stage='construction',all_gates_passed=False,approved_for_audition=False,
        complete_polyphonic_temporal_convergence=False,
        scope='8ms C4 coast from identical cold C4/84 2ms midpoint3072 state; all dampers open. '
              'Fixed 128kHz microphone operator. No full note, phrase, release or acoustic certificate.',
        base_report_sha256=digest(args.base_report),base_spectra_sha256=digest(old_spectra),
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in names},
        criteria=prior['criteria'].copy(),cases=[],grids=[])
    def save():
        report['seconds']=time.perf_counter()-start
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save(); modal,geometry,air,kernel,paths=base.audit.frozen.prepare(report,save)
    original,_=base.audit.frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    contact=SharedContact(system,air); coast=PackedCoast(system,air,PackedResidues(air.wave))
    initial=contact.initial_state(0.)
    initial,work=system.launch(initial,60,5*(84/124)**1.5)
    report['launch_work_j']=work; report['stage']='regenerate common separated state'; save()
    separated,contact_trace,stats=native_run(contact,initial,.002,3072000)
    if system.compression(separated[0])>=-1e-12: raise ValueError('separated reference required')
    with np.load(old_spectra,allow_pickle=False) as stored:
        old_f=stored['frequency_hz']; H=kernel_transfer(kernel,128000,old_f)
        check=pressure_spectra(contact_trace,old_f,H)
        reference=next(c for c in prior['cases'] if c['label']=='key60_velocity84_midpoint3072000')
        errors={w:float(base.audit.relative(check[w],stored[reference['spectrum_keys'][w]['native']])) for w in check}
    if max(errors.values())>1e-10: raise ValueError('regenerated contact disagrees with base spectra')
    report['common_state']=dict(physical_time_s=.002,rate_hz=3072000,
        preceding_history_converged=False,contact_stats=stats,
        base_contact_spectrum_relative=errors,base_contact_spectrum_passed=True)
    args.state.parent.mkdir(parents=True,exist_ok=True)
    np.savez_compressed(args.state,**{f'state{i}':x for i,x in enumerate(separated)})
    report['common_state'].update(path=str(args.state.resolve()),sha256=digest(args.state),
                                 bytes=args.state.stat().st_size)
    del contact_trace,check,H
    n=system.string.size; omega=np.sqrt(coast.oscillator_functions(1/128000)[3])
    def mechanical(s): return np.r_[omega*coast.project(s[0][:n]),s[1][:n]]
    runs={}
    for rate in [16000,32000,64000,128000,256000,512000]:
        report['stage']=f'coast{rate}'; save()
        final,trace,stats=native_run(coast,separated,.008,rate)
        runs[rate]=(final,trace); report['cases'].append(stats); save()
        print('COAST',rate,stats['seconds'],flush=True)
    fine_rate=512000; fine=runs[fine_rate]
    for spacing in [6.25,3.125,1.5625]:
        f=frequency_grid(spacing); H=kernel_transfer(kernel,128000,f)
        vectors={'frequency_hz':f}; spectra={}; controls=[]; comparisons=[]
        report['stage']=f'frequency grid {spacing}Hz'; save()
        for rate,(_,trace) in runs.items():
            P=pressure_spectra(trace,f,H); spectra[rate]=P
            for w,p in P.items(): vectors[f'native{rate}_{w}']=p
            idx=[int(np.flatnonzero(f==hz)[0]) for hz in [6000.,12000.]]
            oracle=np.einsum('fcm,fm->fc',H[idx],hann_acceleration_spectrum(*trace,f[idx]))
            error=float(base.audit.relative(P['hann'][idx],oracle))
            grid={w:{k:float(abs(np.sqrt(band_norms(f[::2],p[::2],BANDS)[k]/v)-1))
                     for k,v in band_norms(f,p,BANDS).items()} for w,p in P.items()}
            controls.append(dict(rate_hz=rate,oracle_relative=error,
                oracle_passed=bool(error<report['criteria']['hermite_oracle_relative']),
                energy_grid_relative=grid,
                energy_grid_passed=bool(max(v for g in grid.values() for v in g.values())<report['criteria']['band_energy_grid_relative'])))
        def compare(label,a,b,keys_a,keys_b):
            row=dict(label=label,spectrum_keys_actual=keys_a,spectrum_keys_reference=keys_b,windows={})
            for w in a:
                metrics=compare_spectra(f,a[w],b[w]); coarse=compare_spectra(f[::2],a[w][::2],b[w][::2])
                errors=[abs(x['relative']-y['relative']) for x,y in zip(metrics['bands'],coarse['bands'])]
                metrics.update(grid_error_absolute=errors,grid_error_passed=bool(max(errors)<report['criteria']['band_error_grid_absolute']))
                row['windows'][w]=metrics
            comparisons.append(row)
            return row
        native_keys=lambda rate:{w:f'native{rate}_{w}' for w in spectra[rate]}
        for rate in runs:
            if rate==fine_rate: continue
            a=spectra[rate]; b=spectra[fine_rate]
            row=compare(f'coast{rate}_to512k',a,b,native_keys(rate),native_keys(fine_rate))
            row.update(mechanical_relative=float(base.audit.relative(mechanical(runs[rate][0]),mechanical(fine[0]))),
                wave_relative=float(base.audit.relative(runs[rate][0][2],fine[0][2])),
                relaxation_relative=float(base.audit.relative(runs[rate][0][3],fine[0][3])))
            sampled=pressure_spectra(capture(fine[1],rate),f,H)
            keys={w:f'fine512_captured{rate}_{w}' for w in sampled}
            for w,p in sampled.items(): vectors[keys[w]]=p
            compare(f'reconstruction512_to{rate}',sampled,b,keys,native_keys(fine_rate))
            compare(f'dynamics_at_capture{rate}',a,sampled,native_keys(rate),keys)
        passed=all(c['oracle_passed'] and c['energy_grid_passed'] for c in controls) and all(
            w['grid_error_passed'] for c in comparisons for w in c['windows'].values())
        report['grids'].append(dict(maximum_spacing_hz=spacing,points=len(f),controls=controls,
                                    comparisons=comparisons,measurement_controls_passed=passed))
        save(); print('GRID',spacing,'controls',passed,flush=True)
        if passed: break
    args.spectra.parent.mkdir(parents=True,exist_ok=True)
    np.savez_compressed(args.spectra,**vectors)
    report['spectral_data']=dict(path=str(args.spectra.resolve()),sha256=digest(args.spectra),bytes=args.spectra.stat().st_size)
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==sha for k,sha in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256']
    report['stage']='completed coast grid refinement and reconstruction attribution'; save()
    print('COAST BAND REFINEMENT COMPLETED',args.report,flush=True)


if __name__=='__main__': main()
