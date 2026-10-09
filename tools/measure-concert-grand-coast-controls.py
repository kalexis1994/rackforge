"""Isolate quadratic coast dispersion and passive air subcycling.

Authenticated C4/84 living state reused once. Ablations are numerical
diagnostics, never replacements for the physical piano or new auditions.
"""
import argparse, importlib.util, json, time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_shared_strings import SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedCoast,PackedResidues
from concert_grand_coast_controls import (quadratic_system,quadratic_energy,
    MechanicalOnlyCoast,AirSubcycledCoast,WorkMeter)
from concert_grand_band_diagnostics import (native_run,capture,frequency_grid,
    pressure_spectra,compare_spectra,BANDS)
from concert_grand_cell_audio import kernel_transfer
from concert_grand_window_spectra import band_norms,hann_acceleration_spectrum

spec=importlib.util.spec_from_file_location('frozen_coast_refinement',
    Path(__file__).with_name('measure-concert-grand-coast-bands.py'))
base=importlib.util.module_from_spec(spec); spec.loader.exec_module(base)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-report',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--spectra',type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists() or args.spectra.exists(): raise ValueError('preserve evidence')
    prior=json.loads(args.base_report.read_text(encoding='utf-8'))
    if not prior['stage'].startswith('completed') or not prior['source_and_cache_unchanged']:
        raise ValueError('completed unchanged refinement required')
    if not prior['grids'][-1]['measurement_controls_passed']: raise ValueError('verified base measurement required')
    for name,sha in prior['source_sha256'].items():
        if digest(Path(__file__).with_name(name))!=sha: raise ValueError('base source changed: '+name)
    state_path=Path(prior['common_state']['path']); old_path=Path(prior['spectral_data']['path'])
    if digest(state_path)!=prior['common_state']['sha256'] or digest(old_path)!=prior['spectral_data']['sha256']:
        raise ValueError('base state/spectra changed')
    with np.load(state_path,allow_pickle=False) as stored:
        initial=tuple(stored[f'state{i}'].copy() for i in range(4))
    names=list(prior['source_sha256'])+[Path(__file__).name,'concert_grand_coast_controls.py']
    start=time.perf_counter(); report=dict(stage='construction',all_gates_passed=False,
        approved_for_audition=False,complete_polyphonic_temporal_convergence=False,
        scope='8ms from unchanged C4/84 full-contact state at 2ms, three keys, all dampers open. '
              'Quadratic and mechanical-only ablations are diagnostic. Air subcycling preserves equations. '
              'Fixed 128kHz microphone operator, no full-note/phrase/radiation certificate.',
        base_report_sha256=digest(args.base_report),base_state_sha256=digest(state_path),
        base_spectra_sha256=digest(old_path),source_sha256={n:digest(Path(__file__).with_name(n)) for n in names},
        criteria=dict(prior['criteria'],mechanical_work_relative=1e-9,replay_relative=1e-10),cases=[],grids=[])
    def save():
        report['seconds']=time.perf_counter()-start; args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save(); modal,geometry,air,kernel,paths=base.base.audit.frozen.prepare(report,save)
    original,_=base.base.audit.frozen.make_system(modal,geometry,96,3)
    full=SharedHammers(BatchedStrings(original.string),original.felt)
    system=quadratic_system(full); residues=PackedResidues(air.wave)
    physical=PackedCoast(full,air,residues); quadratic=PackedCoast(system,air,residues)
    free=MechanicalOnlyCoast(system,air,residues)
    air8=AirSubcycledCoast(system,air,residues,8)
    air4=AirSubcycledCoast(system,air,residues,4)
    expected=physical.initial_state(0.)
    if any(x.shape!=y.shape or not np.isfinite(x).all() for x,y in zip(initial,expected)):
        raise ValueError('incompatible living state')
    if full.compression(initial[0])>=-1e-12: raise ValueError('separated heads required')
    initial_hash=[digest_array(x) for x in initial]
    energy=quadratic_energy(quadratic,initial)
    report['ablation_switch']=dict(full_mechanical_energy_j=full.energy(*initial[:2]),
        quadratic_mechanical_energy_j=energy,
        intentional_energy_change_j=energy-full.energy(*initial[:2]),
        reset_arrays=False,remove_coordinates=False,full_core_nonlinear_unchanged=bool(full.string.nonlinear))
    runs={}
    schedule=[('full16',physical,16000)]+[(f'quadratic{r}',quadratic,r) for r in [16000,32000,64000,128000,512000]]+[
        ('quadratic16_air8',air8,16000),('quadratic32_air4',air4,32000),
        ('mechanical16',free,16000),('mechanical512',free,512000)]
    for label,pilot,rate in schedule:
        report['stage']=label; save(); meter=WorkMeter(pilot)
        final,trace,stats=native_run(meter,initial,.008,rate)
        stats.update(label=label,quadratic=not pilot.system.string.nonlinear,
            air_substeps=getattr(pilot,'substeps',1),air_work_j=meter.air_work_j)
        if stats['quadratic']:
            closure=quadratic_energy(pilot,final)-energy+meter.air_work_j+meter.material_heat_j
            stats.update(mechanical_work_balance_j=closure,mechanical_work_relative=abs(closure)/energy,
                         mechanical_work_passed=bool(abs(closure)/energy<report['criteria']['mechanical_work_relative']))
        report['cases'].append(stats); runs[label]=(final,trace,pilot); save()
        print('CONTROL',label,stats['seconds'],'work',stats.get('mechanical_work_relative'),flush=True)
    with np.load(old_path,allow_pickle=False) as stored:
        f=stored['frequency_hz']; H=kernel_transfer(kernel,128000,f)
        replay=pressure_spectra(runs['full16'][1],f,H)
        errors={w:float(base.base.audit.relative(replay[w],stored['native16000_'+w])) for w in replay}
    report['physical_replay']=dict(relative=errors,passed=bool(max(errors.values())<report['criteria']['replay_relative']))
    if not report['physical_replay']['passed']: raise ValueError('physical control changed')
    n=system.string.size; omega=np.sqrt(quadratic.oscillator_functions(1/128000)[3])
    def mechanical(s): return np.r_[omega*quadratic.project(s[0][:n]),s[1][:n]]
    for spacing in [6.25,3.125,1.5625]:
        f=frequency_grid(spacing); H=kernel_transfer(kernel,128000,f)
        vectors={'frequency_hz':f}; P={}; controls=[]; comparisons=[]
        report['stage']=f'spectral grid {spacing}'; save()
        for label,(_,trace,_) in runs.items():
            P[label]=pressure_spectra(trace,f,H)
            for w,x in P[label].items(): vectors[label+'_'+w]=x
            idx=[int(np.flatnonzero(f==hz)[0]) for hz in [6000.,12000.]]
            oracle=np.einsum('fcm,fm->fc',H[idx],hann_acceleration_spectrum(*trace,f[idx]))
            error=float(base.base.audit.relative(P[label]['hann'][idx],oracle))
            grid={w:{k:float(abs(np.sqrt(band_norms(f[::2],x[::2],BANDS)[k]/v)-1))
                     for k,v in band_norms(f,x,BANDS).items()} for w,x in P[label].items()}
            controls.append(dict(label=label,oracle_relative=error,
                oracle_passed=bool(error<report['criteria']['hermite_oracle_relative']),energy_grid_relative=grid,
                energy_grid_passed=bool(max(v for g in grid.values() for v in g.values())<report['criteria']['band_energy_grid_relative'])))
        keys=lambda label:{w:label+'_'+w for w in P[label]}
        def compare(label,a,b,ka,kb,states=None):
            row=dict(label=label,spectrum_keys_actual=ka,spectrum_keys_reference=kb,windows={})
            for w in a:
                m=compare_spectra(f,a[w],b[w]); c=compare_spectra(f[::2],a[w][::2],b[w][::2])
                errors=[abs(x['relative']-y['relative']) for x,y in zip(m['bands'],c['bands'])]
                m.update(grid_error_absolute=errors,grid_error_passed=bool(max(errors)<report['criteria']['band_error_grid_absolute']))
                row['windows'][w]=m
            if states:
                sa,sb=states
                row.update(mechanical_relative=float(base.base.audit.relative(mechanical(sa),mechanical(sb))),
                    wave_relative=float(base.base.audit.relative(sa[2],sb[2])),
                    relaxation_relative=float(base.base.audit.relative(sa[3],sb[3])))
            comparisons.append(row)
        for label in [f'quadratic{r}' for r in [16000,32000,64000,128000]]+['quadratic16_air8','quadratic32_air4','mechanical16']:
            reference='mechanical512' if label.startswith('mechanical') else 'quadratic512000'
            rate=next(c['rate_hz'] for c in report['cases'] if c['label']==label)
            sample_label=reference+'_capture'+str(rate)
            if sample_label not in P:
                P[sample_label]=pressure_spectra(capture(runs[reference][1],rate),f,H)
                for w,x in P[sample_label].items(): vectors[sample_label+'_'+w]=x
            compare(label+'_native',P[label],P[reference],keys(label),keys(reference),(runs[label][0],runs[reference][0]))
            compare(label+'_same_capture',P[label],P[sample_label],keys(label),keys(sample_label))
        passed=all(c['oracle_passed'] and c['energy_grid_passed'] for c in controls) and all(
            w['grid_error_passed'] for c in comparisons for w in c['windows'].values())
        report['grids'].append(dict(maximum_spacing_hz=spacing,points=len(f),controls=controls,
                                    comparisons=comparisons,measurement_controls_passed=passed))
        save(); print('GRID',spacing,passed,flush=True)
        if passed: break
    args.spectra.parent.mkdir(parents=True,exist_ok=True); np.savez_compressed(args.spectra,**vectors)
    report['spectral_data']=dict(path=str(args.spectra.resolve()),sha256=digest(args.spectra),bytes=args.spectra.stat().st_size)
    report['initial_state_unchanged']=initial_hash==[digest_array(x) for x in initial]
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==s for k,s in report['source_sha256'].items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256'] and digest(state_path)==report['base_state_sha256']
    report['stage']='completed quadratic and air subcycling coast controls'; save()
    print('COAST CONTROLS COMPLETED',args.report,flush=True)


def digest_array(x):
    import hashlib
    return hashlib.sha256(np.ascontiguousarray(x).view(np.uint8)).hexdigest()


if __name__=='__main__': main()
