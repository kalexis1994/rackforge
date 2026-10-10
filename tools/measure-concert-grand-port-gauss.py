"""Full-physics control of half carriers sharing two Gauss air stages.

Reuse authenticated living C4/84 state and frozen 512kHz pressure evidence.
State errors use an explicit triangle bound through a replayed 128kHz state.
No new WAV, room fit, altered physical coefficient or duplicate large cache.
"""
import argparse,importlib.util,json,time
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest
from concert_grand_shared_strings import SharedHammers
from concert_grand_shared_kernels import BatchedStrings
from concert_grand_packed_air import PackedCoast,PackedResidues
from concert_grand_port_gauss import PortGaussCoast
from concert_grand_coast_controls import WorkMeter
from concert_grand_band_diagnostics import native_run,pressure_spectra,compare_spectra,BANDS
from concert_grand_cell_audio import kernel_transfer
from concert_grand_window_spectra import band_norms,hann_acceleration_spectrum

spec=importlib.util.spec_from_file_location('frozen_coast_controls',Path(__file__).with_name('measure-concert-grand-coast-controls.py'))
base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-report',type=Path,required=True)
    parser.add_argument('--controls-report',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--spectra',type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists() or args.spectra.exists(): raise ValueError('preserve measured evidence')
    prior=json.loads(args.base_report.read_text()); controls=json.loads(args.controls_report.read_text())
    if not prior['source_and_cache_unchanged'] or not controls['source_and_cache_unchanged']:
        raise ValueError('unchanged completed input required')
    if not controls['stage'].startswith('completed') or not prior['grids'][-1]['measurement_controls_passed']:
        raise ValueError('completed controls and verified base grid required')
    names={**prior['source_sha256'],**controls['source_sha256']}
    for name,sha in names.items():
        if digest(Path(__file__).with_name(name))!=sha: raise ValueError('input source changed: '+name)
    state_path=Path(prior['common_state']['path']); input_path=Path(prior['spectral_data']['path'])
    if digest(state_path)!=prior['common_state']['sha256'] or digest(input_path)!=prior['spectral_data']['sha256']:
        raise ValueError('input state/data changed')
    with np.load(state_path,allow_pickle=False) as data: initial=tuple(data[f'state{i}'].copy() for i in range(4))
    with np.load(input_path,allow_pickle=False) as data: old={k:data[k].copy() for k in data.files}
    names.update({n:digest(Path(__file__).with_name(n)) for n in [Path(__file__).name,'concert_grand_gauss_air.py','concert_grand_port_gauss.py','concert_grand_port_avf_coast.py']})
    start=time.perf_counter(); report=dict(stage='construction',all_gates_passed=False,
        approved_for_audition=False,complete_polyphonic_temporal_convergence=False,
        scope='Full nonlinear C4/84, three keys, 8ms after unchanged 2ms contact state. '
              'All dampers open; fixed 128kHz microphone operator. Local pressure and endpoint state bounds only.',
        base_report_sha256=digest(args.base_report),controls_report_sha256=digest(args.controls_report),
        base_state_sha256=digest(state_path),base_spectra_sha256=digest(input_path),
        source_sha256=names,criteria=dict(prior['criteria'],mechanical_work_relative=1e-9,
            replay_relative=1e-10,state_bound_relative=.01),cases=[])
    def save():
        report['seconds']=time.perf_counter()-start;args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    save();modal,geometry,air,kernel,paths=base.base.base.audit.frozen.prepare(report,save)
    original,_=base.base.base.audit.frozen.make_system(modal,geometry,96,3)
    system=SharedHammers(BatchedStrings(original.string),original.felt)
    residues=PackedResidues(air.wave); reference=PackedCoast(system,air,residues)
    coupled=PortGaussCoast(system,air,residues)
    if any(x.shape!=y.shape or not np.isfinite(x).all() for x,y in zip(initial,reference.initial_state(0.))):
        raise ValueError('compatible finite complete state required')
    initial_hash=[base.digest_array(x) for x in initial]
    f=old['frequency_hz']; H=kernel_transfer(kernel,128000,f); vectors={'frequency_hz':f}
    for w in ['rectangular','hann']: vectors['reference512_'+w]=old['native512000_'+w]
    envelope=next(c for c in prior['grids'][-1]['comparisons'] if c['label']=='coast128000_to512k')
    n=system.string.size; omega=np.sqrt(reference.oscillator_functions(1/128000)[3])
    def mechanical(s):return np.r_[omega*reference.project(s[0][:n]),s[1][:n]]
    reference_state=None; initial_energy=system.energy(*initial[:2])
    for label,pilot,rate in [('reference128',reference,128000)]+[(f'coupled{r}',coupled,r) for r in [32000,64000,128000,256000]]:
        report['stage']=label;save();meter=WorkMeter(pilot)
        final,trace,stats=native_run(meter,initial,.008,rate)
        P=pressure_spectra(trace,f,H); keys={w:label+'_'+w for w in P}
        for w,x in P.items():vectors[keys[w]]=x
        closure=system.energy(*final[:2])-initial_energy+meter.air_work_j+meter.material_heat_j
        idx=[int(np.flatnonzero(f==hz)[0]) for hz in [6000.,12000.]]
        oracle=np.einsum('fcm,fm->fc',H[idx],hann_acceleration_spectrum(*trace,f[idx]))
        oracle_error=float(base.base.base.audit.relative(P['hann'][idx],oracle))
        row=dict(stats,label=label,spectrum_keys=keys,full_nonlinear_geometry=bool(system.string.nonlinear),
            initial_mechanical_energy_j=initial_energy,final_mechanical_energy_j=system.energy(*final[:2]),
            air_work_j=meter.air_work_j,material_heat_j=meter.material_heat_j,
            mechanical_work_balance_j=closure,mechanical_work_relative=abs(closure)/initial_energy,
            mechanical_work_passed=bool(abs(closure)/initial_energy<report['criteria']['mechanical_work_relative']),
            hann_oracle_relative=oracle_error,hann_oracle_passed=bool(oracle_error<report['criteria']['hermite_oracle_relative']),
            windows={})
        for w,x in P.items():
            b=old['native512000_'+w]; metrics=compare_spectra(f,x,b); c=compare_spectra(f[::2],x[::2],b[::2])
            errors=[abs(a['relative']-b['relative']) for a,b in zip(metrics['bands'],c['bands'])]
            fine=band_norms(f,x,BANDS); coarse=band_norms(f[::2],x[::2],BANDS)
            grid={k:float(abs(np.sqrt(coarse[k]/v)-1)) for k,v in fine.items()}
            metrics.update(grid_error_absolute=errors,grid_error_passed=bool(max(errors)<report['criteria']['band_error_grid_absolute']),
                energy_grid_relative=grid,energy_grid_passed=bool(max(grid.values())<report['criteria']['band_energy_grid_relative']))
            row['windows'][w]=metrics
        if label=='reference128':
            reference_state=final
            replay={w:float(base.base.base.audit.relative(P[w],old['native128000_'+w])) for w in P}
            row['replay_relative']=replay
            if max(replay.values())>=report['criteria']['replay_relative']:raise ValueError('128k reference replay changed')
            row['state_bound_to512']=dict(mechanical=envelope['mechanical_relative'],wave=envelope['wave_relative'],relaxation=envelope['relaxation_relative'])
        else:
            diffs=dict(mechanical=float(base.base.base.audit.relative(mechanical(final),mechanical(reference_state))),
                wave=float(base.base.base.audit.relative(final[2],reference_state[2])),
                relaxation=float(base.base.base.audit.relative(final[3],reference_state[3])))
            bounds={k:a*(1+envelope[k+'_relative'])+envelope[k+'_relative'] for k,a in diffs.items()}
            row.update(state_relative_to128=diffs,state_bound_to512=bounds,
                speedup_vs_replayed128=report['cases'][0]['seconds']/stats['seconds'])
        row['state_bound_passed']=bool(max(row['state_bound_to512'].values())<report['criteria']['state_bound_relative'])
        row['measurement_controls_passed']=bool(row['hann_oracle_passed'] and all(
            m['grid_error_passed'] and m['energy_grid_passed'] for m in row['windows'].values()))
        row['local_controls_passed']=bool(row['measurement_controls_passed'] and row['state_bound_passed'] and row['mechanical_work_passed'] and all(
            m['all_bands_1_percent_passed'] for m in row['windows'].values()))
        report['cases'].append(row);save()
        print('GAUSS AIR PORT COAST',label,stats['seconds'],'Hann', [b['relative'] for b in row['windows']['hann']['bands']],
              'state bounds',row['state_bound_to512'],'local',row['local_controls_passed'],flush=True)
    args.spectra.parent.mkdir(parents=True,exist_ok=True);np.savez_compressed(args.spectra,**vectors)
    report['spectral_data']=dict(path=str(args.spectra.resolve()),sha256=digest(args.spectra),bytes=args.spectra.stat().st_size)
    report['state_bound_method']='e(actual,512) <= e(actual,128)*(1+e(128,512))+e(128,512); all local endpoint norms, same physical metric.'
    report['initial_state_unchanged']=initial_hash==[base.digest_array(x) for x in initial]
    report['source_and_cache_unchanged']=all(digest(Path(__file__).with_name(k))==s for k,s in names.items()) and digest(paths.board)==report['board_sha256'] and digest(paths.cache/'R.npy')==report['cache_R_sha256'] and digest(state_path)==report['base_state_sha256']
    report['stage']='completed full-physics coupled Gauss air port coast controls';save()
    print('COUPLED GAUSS AIR PORT COAST COMPLETED',args.report,flush=True)


if __name__=='__main__':main()
