"""Matched SI microphone-pressure transforms of frozen 35/59-mode contacts.

Finite 4/8ms acceleration windows only; Pa*s, not a sustained piano or SPL.
Canonical report in repository; unique derived numerical NPZ in SHARED.
"""
import argparse,hashlib,json,platform,time
from pathlib import Path
import numpy as np
import scipy
from concert_grand_cell_observer import CellObserver,acceleration_spectrum,spectral_relative
from concert_grand_surface_impedance import RectangularLoad
from concert_grand_si_observer import Surface,RayleighObserver

CRITERIA=dict(cell_quadrature=1e-5,spatial_transfer=.01,spatial_pressure=.01,
    capture_pressure=.001,mechanical_pressure=.01,join=1e-10,nested_transfer=1e-10,decomposition=1e-10)
BANDS=dict(Low=(60.,800.),AddedBand=(800.,1200.),High=(1200.,6000.),Full=(60.,6000.))
GEOMETRY=np.array([[.55,.5,.66],[.85,.5,.66],[.55,.5,2.],[.85,.5,2.]])


def digest(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def write(path,data):path.write_text(json.dumps(data,indent=2,allow_nan=False),encoding='utf-8')
def relative(a,b):return float(np.linalg.norm(a-b)/max(np.linalg.norm(b),1e-30))
def project(H,A):return np.einsum('fcm,fm->fc',H,A)
def bands(f,a,b):return {name:spectral_relative(f,a,b,*span) for name,span in BANDS.items()}


def read_sources(report,folder):
    out={}
    modes=report['provenance']['support']['modes']
    for rate in [384000,768000]:
        segments={}
        for phase in ['Cold','Relaunch']:
            key=f'T72-{rate}-{phase}';path=folder/(key+'.npz')
            if digest(path)!=report['trajectory_sha256'][key]:raise ValueError('frozen trajectory changed')
            with np.load(path) as data:
                t=data['trace'][:,0];B=data['board_position'];V=data['board_velocity']
            if B.shape!=(513,modes) or V.shape!=B.shape or not np.allclose(np.diff(t),1/128000,rtol=1e-12,atol=1e-16) or t[0]!=0 or t[-1]!=.004:
                raise ValueError('frozen 4ms/128k physical board trace required')
            segments[phase]=(t,B,V)
        a,b=segments['Cold'],segments['Relaunch']
        if not np.array_equal(a[1][-1],b[1][0]) or not np.array_equal(a[2][-1],b[2][0]):raise ValueError('board restart at contact join')
        segments['Sequence']=(np.r_[a[0],b[0][1:]+a[0][-1]],np.vstack([a[1],b[1][1:]]),np.vstack([a[2],b[2][1:]]))
        out[rate]=segments
    return out


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['asset','old-report','new-report','old-data','new-data','output']:p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--report',type=Path,default=Path(__file__).resolve().parents[1]/'docs'/'concert-grand-physics-roadmap-2026-10-02'/'resumen-P6-observacion-1200.json')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True);args.report.parent.mkdir(parents=True,exist_ok=True)
    target=args.output/'spectra.npz'
    if target.exists() or args.report.exists():raise ValueError('preserve measured experiment; choose new destinations')
    started=time.monotonic();old=json.loads(args.old_report.read_text());new=json.loads(args.new_report.read_text())
    for doc in [old,new]:
        if not all(doc['gates'].values()):raise ValueError('validated contact banks required')
        if digest(args.asset)!=doc['provenance']['asset_sha256']:raise ValueError('physical structural asset changed')
    if any(old['provenance'][k]!=new['provenance'][k] for k in ['input_hashes','felt','wire','incident_speed_m_s','transverse','axial']):raise ValueError('matched physical contacts required')
    if old['provenance']['support']['modes']!=35 or new['provenance']['support']['modes']!=59:raise ValueError('frozen 35/59 mode banks required')
    source={35:read_sources(old,args.old_data),59:read_sources(new,args.new_data)}
    with np.load(args.asset) as data:
        frequency=data['frequency'];mask=frequency<=1200.
        model=dict(panel=json.loads(str(data['panel'])),nx=int(data['nx']),ny=int(data['ny']),boundary=str(data['boundary']),frequency=frequency[mask],vectors=data['vectors'][:,mask])
    if not np.array_equal(model['frequency'],new['provenance']['support']['frequencies_hz']):raise ValueError('modal order does not match source')
    low_model={**model,'frequency':model['frequency'][:35],'vectors':model['vectors'][:,:35]}
    surface=RectangularLoad.from_plate(model,128,96);small=RectangularLoad.from_plate(low_model,128,96)
    f=np.arange(60.,6001.,20.);probes=np.array([60.,261.6255653,800.,1200.,2000.,4500.,6000.])
    report=dict(baseline_commit='42e0b3c',criteria=CRITERIA,geometry_m=GEOMETRY.tolist(),density_kg_m3=surface.density,speed_m_s=surface.speed,
        pressure_units='Pa*s; Fourier transforms of finite acceleration windows; not SPL',
        source_units='Physical intrinsic B sqrt(kg)*m, V sqrt(kg)*m/s, A-transform sqrt(kg)*m/s',gain=1.,normalization='none',
        source_durations_s=dict(Cold=.004,Relaunch=.004,Sequence=.008),frequency_band_hz=[60,6000],frequency_step_hz=20.,
        scope='Same P6 constant-cell surface and baffled upper half-space; pressure readout only, source already includes reciprocal air load. No new damping, room, lid, pattern or EQ.',
        assumptions=['Hypothetical P2 geometry and losses, not identified Steinway',
            'First two microphones reuse P3 SI geometry; other two are a separate 2m height control, not rescaled primary audio',
            'Acceleration is observed only inside 4/8ms windows, with no endpoint impulses; full sustain and audition remain untested'],
        inputs={str(path):digest(path) for path in [args.asset,args.old_report,args.new_report]},
        source_trajectory_sha256={'35':old['trajectory_sha256'],'59':new['trajectory_sha256']},gates={},checks={},basis_changes={},
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__,blas_threads=1))
    write(args.report,report)
    observer=CellObserver(surface,GEOMETRY,4);H=observer.acceleration_transfer(f);H35=CellObserver(small,GEOMETRY,4).acceleration_transfer(f)
    report['checks']['nested_transfer']=relative(H[:,:,:35],H35);report['gates']['nested_transfer']=report['checks']['nested_transfer']<CRITERIA['nested_transfer']
    Hprobe=observer.acceleration_transfer(probes);H8=CellObserver(surface,GEOMETRY,8).acceleration_transfer(probes)
    report['checks']['cell_quadrature_max_point']=max(relative(a,b) for a,b in zip(Hprobe,H8))
    report['gates']['cell_quadrature']=report['checks']['cell_quadrature_max_point']<CRITERIA['cell_quadrature']
    spatial_surface=RectangularLoad.from_plate(model,192,144);Hspatial=CellObserver(spatial_surface,GEOMETRY,4).acceleration_transfer(f)
    # Independent continuous plate quadrature checks the finite-cell basis
    # approximation. It is not silently substituted for the coupled surface.
    Hcontinuous=RayleighObserver(Surface.from_plate(model,96,72),GEOMETRY).acceleration_transfer(probes)
    Hcontinuous_ref=RayleighObserver(Surface.from_plate(model,128,96),GEOMETRY).acceleration_transfer(probes)
    report['checks']['continuous_plate_quadrature_max_point']=max(relative(a,b) for a,b in zip(Hcontinuous,Hcontinuous_ref))
    report['checks']['cell_to_continuous_plate_max_point']=max(relative(a,b) for a,b in zip(Hprobe,Hcontinuous_ref))
    report['checks']['spatial_transfer']=bands(f,H,Hspatial)
    report['gates']['spatial_transfer']=max(report['checks']['spatial_transfer'].values())<CRITERIA['spatial_transfer']
    write(args.report,report);print('transfer',json.dumps(report['checks']),flush=True)
    saved=dict(frequency_hz=f,microphones_m=GEOMETRY,transfer128=H,transfer192=Hspatial,probe_frequency_hz=probes,transfer_probe_order8=H8)
    accelerations={};pressures={};capture=[];mechanical=[];spatial=[];joins=[];closure=[]
    for modes in [35,59]:
        transfer=H35 if modes==35 else H
        for rate,segments in source[modes].items():
            local={}
            for phase,(t,B,V) in segments.items():
                key=f'M{modes}-R{rate}-{phase}';A=acceleration_spectrum(t,B,V,f)
                coarse=acceleration_spectrum(t[::2],B[::2],V[::2],f);P=project(transfer,A)
                local[phase]=A;accelerations[key]=A;pressures[key]=P
                saved['acceleration_'+key]=A;saved['pressure_'+key]=P
                capture.append(dict(case=key,errors=bands(f,project(transfer,coarse),P)))
                Pspatial=project(Hspatial[:,:,:modes],A)
                spatial.append(dict(case=key,errors=bands(f,P,Pspatial)))
            joined=local['Cold']+np.exp(-2j*np.pi*f*.004)[:,None]*local['Relaunch']
            joins.append(dict(modes=modes,rate_hz=rate,relative=relative(joined,local['Sequence'])))
        for phase in ['Cold','Relaunch','Sequence']:
            coarse=pressures[f'M{modes}-R384000-{phase}'];fine=pressures[f'M{modes}-R768000-{phase}']
            mechanical.append(dict(modes=modes,phase=phase,errors=bands(f,coarse,fine)))
    for phase in ['Cold','Relaunch','Sequence']:
        A=accelerations[f'M59-R768000-{phase}'];Pold=pressures[f'M35-R768000-{phase}'];Pnew=pressures[f'M59-R768000-{phase}']
        common=project(H[:,:,:35],A[:,:35]);added=project(H[:,:,35:],A[:,35:])
        closure.append(relative(Pnew-Pold,(common-Pold)+added))
        report['basis_changes'][phase]={}
        for label,selection in dict(Close=slice(0,2),Height2m=slice(2,4),CloseMono=slice(0,2)).items():
            a,b,c,d=Pnew[:,selection],Pold[:,selection],common[:,selection],added[:,selection]
            if label=='CloseMono':a,b,c,d=[x.mean(axis=1,keepdims=True) for x in [a,b,c,d]]
            changes=bands(f,a,b)
            # Norm ratio is separate from complex spectral difference.
            ratios={name:float(np.sqrt(np.trapezoid(np.sum(abs(a[(f>=lo)&(f<=hi)])**2,axis=1),f[(f>=lo)&(f<=hi)])/np.trapezoid(np.sum(abs(b[(f>=lo)&(f<=hi)])**2,axis=1),f[(f>=lo)&(f<=hi)]))) for name,(lo,hi) in BANDS.items()}
            report['basis_changes'][phase][label]=dict(pressure_spectral_difference=changes,pressure_norm_ratio=ratios,
                common_modes_change=bands(f,c,b),additional_modes_norm_relative=bands(f,b+d,b))
        saved['pressure_common_R768000_'+phase]=common;saved['pressure_added_R768000_'+phase]=added
    report['checks'].update(capture=capture,mechanical=mechanical,spatial_pressure=spatial,join=joins,decomposition_max=max(closure))
    maximum=lambda rows:max(max(row['errors'].values()) for row in rows)
    report['gates'].update(capture_pressure=maximum(capture)<CRITERIA['capture_pressure'],mechanical_pressure=maximum(mechanical)<CRITERIA['mechanical_pressure'],
        spatial_pressure=maximum(spatial)<CRITERIA['spatial_pressure'],join=max(row['relative'] for row in joins)<CRITERIA['join'],decomposition=max(closure)<CRITERIA['decomposition'])
    # Independent 10Hz frequency-integration refinement: evaluate the midpoint
    # frequencies, interleave the already-measured 20Hz spectra, and compare
    # the resulting basis-difference metrics. Diagnostic, not gain correction.
    midpoint=f[:-1]+10.;Hm=observer.acceleration_transfer(midpoint)
    allf=np.sort(np.r_[f,midpoint]);frequency_check={}
    for phase in ['Cold','Relaunch','Sequence']:
        fine={}
        for modes in [35,59]:
            t,B,V=source[modes][768000][phase];Am=acceleration_spectrum(t,B,V,midpoint)
            Pm=project(Hm[:,:,:modes],Am);P=np.empty((len(allf),4),dtype=complex)
            P[::2]=pressures[f'M{modes}-R768000-{phase}'];P[1::2]=Pm;fine[modes]=P
        frequency_check[phase]=dict(step10=bands(allf,fine[59][:,:2],fine[35][:,:2]),step20=report['basis_changes'][phase]['Close']['pressure_spectral_difference'])
    report['frequency_integration_diagnostic']=frequency_check
    np.savez_compressed(target,**saved);report['measurement_sha256']=digest(target)
    report['source_sha256']={name:digest(Path(__file__).with_name(name)) for name in ['audit-concert-grand-cell-observation.py','concert_grand_cell_observer.py','concert_grand_surface_impedance.py','concert_grand_si_observer.py']}
    report['seconds']=time.monotonic()-started;report['all_observation_gates_passed']=all(report['gates'].values())
    report['decision']='Finite attack pressure comparison only. Structural convergence, wider notes/velocities, sustain and listening remain open; factory unchanged.'
    write(args.report,report);print('gates',json.dumps(report['gates']),flush=True)
    print('basis',json.dumps(report['basis_changes']),flush=True)


if __name__=='__main__':main()
