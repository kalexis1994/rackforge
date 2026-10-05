"""Verify the projected board against its full parent WITH direct air return.

Sparse harmonic controls and independent angular/spatial/quadrature checks.
No dense modal-air matrices, FIR factors, causal states, WAV or plugin update.
"""
import argparse,json,time,ctypes,sys
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
import concert_grand_orthotropic_plate as plate
from concert_grand_matrixfree_air import RitzSurface,CellAir,MechanicalDynamic,solve_loaded

spec=spec_from_file_location('loaded_projection_reference',Path(__file__).with_name('audit-concert-grand-high-band-board.py'))
reference=module_from_spec(spec);spec.loader.exec_module(reference)
CRITERIA=dict(loaded_port_relative=.01,spatial_relative=.01,angular_relative=.01,
    angular_refinement_relative=.01,quadrature_relative=.001,
    force_residual=2e-10,power_balance=1e-8,reciprocity=1e-8,minimum_dissipation=-1e-10)
CHECKS=[60.,261.625565,523.251131,1200.,2400.,3600.,4800.,6000.]
ANGULAR=[60.,261.625565,1200.,3600.,6000.]


def peak_memory():
    if sys.platform!='win32':return None
    from ctypes import wintypes
    class Counters(ctypes.Structure):
        _fields_=[('cb',wintypes.DWORD),('faults',wintypes.DWORD)]+[(n,ctypes.c_size_t) for n in ['peak','working','peak_paged','paged','peak_nonpaged','nonpaged','pagefile','peak_pagefile']]
    data=Counters();data.cb=ctypes.sizeof(data)
    if not ctypes.windll.psapi.GetProcessMemoryInfo(ctypes.c_void_p(-1),ctypes.byref(data),data.cb):return None
    return int(data.peak)


def port_errors(a,b):
    return (np.linalg.norm(a-b,axis=0)/np.maximum(np.linalg.norm(b,axis=0),1e-40)).tolist()


def ports(model,notes):
    x=model['panel']['length']*(.1+.8*(np.array(notes)-21)/87)
    return plate.shape(model,x,np.full(len(x),model['panel']['width']/2)).T


def evaluate(model,surface,frequency,B,projected=None,order=8):
    mechanics=MechanicalDynamic(model['frequency'],frequency,projected=projected)
    air=CellAir(surface,mechanics.s,order)
    q,stats=solve_loaded(mechanics,air,B)
    velocity=mechanics.s*q;Y=B.T@velocity;P=surface.pressure(frequency,q,reference.MICS)
    reciprocity=reference.rel(Y,Y.T)
    eigen=np.linalg.eigvalsh((Y+Y.conj().T)/2)
    stats.update(reciprocity=reciprocity,minimum_dissipation=float(eigen.min()/max(np.linalg.norm(Y),1e-40)))
    return q,Y,P,stats


def main():
    p=argparse.ArgumentParser(description=__doc__)
    for n in ['mechanical-report','projection-report','state-report','output','report']:p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--cells',nargs=2,type=int,default=[192,144]);p.add_argument('--fine-cells',nargs=2,type=int,default=[256,192])
    p.add_argument('--limit-frequencies',type=int);p.add_argument('--real-arithmetic',action='store_true')
    a=p.parse_args()
    if a.output.exists() or a.report.exists():raise ValueError('new output/report required')
    if any(n<=0 for n in a.cells+a.fine_cells) or any(f<=c for c,f in zip(a.cells,a.fine_cells)):
        raise ValueError('fine cells must refine both positive spatial dimensions')
    Surface=RitzSurface
    if a.real_arithmetic:
        from concert_grand_real_surface import RealArithmeticSurface
        Surface=RealArithmeticSurface
    start=time.monotonic();mechanical=json.loads(a.mechanical_report.read_text());parent=json.loads(a.projection_report.read_text());state=json.loads(a.state_report.read_text())
    if (not mechanical['all_spatial_gates_passed'] or not parent['selected'] or not state['mass_velocity_passed']
        or reference.digest(a.mechanical_report)!=parent['mechanical_report_sha256']
        or reference.digest(a.projection_report)!=state['parent_report_sha256']):raise ValueError('verified frozen mechanical/state parents required')
    full_path=Path(mechanical['structural_asset_path']);small_path=Path(parent['projected_asset_path'])
    if reference.digest(full_path)!=parent['parent_asset_sha256'] or reference.digest(small_path)!=parent['projected_asset_sha256']:
        raise ValueError('mechanical assets changed')
    with np.load(full_path) as d:
        full=dict(frequency=d['frequency'],vectors=d['vectors'],nx=int(d['nx']),ny=int(d['ny']),
            panel=json.loads(str(d['panel'])),beams=json.loads(str(d['beams'])),boundary=str(d['boundary']))
    with np.load(small_path) as d:
        red={n:d[n] for n in ['projection','frequency','tail_mass','tail_stiffness','tail_damping','low_damping']}
        red['low_count']=int(d['low_count']);notes=d['test_notes'].tolist()
        small=dict(full,frequency=red['frequency'],vectors=d['ritz_vectors'])
    B=ports(full,notes);b=red['projection'].T@B
    full_surface=Surface(full,a.cells);small_surface=Surface(small,a.cells)
    resonances=full['frequency'][(full['frequency']>=60)&(full['frequency']<=6000)]
    sampled=resonances[np.unique(np.linspace(0,len(resonances)-1,14,dtype=int))]
    pitch=440*2**((np.array(notes)-69)/12)
    f=np.unique(np.r_[np.geomspace(60,6000,25),sampled,sampled*.99,sampled*1.01,pitch,CHECKS])
    f=f[(f>=60)&(f<=6000)]
    if a.limit_frequencies:
        if a.limit_frequencies<2:raise ValueError('at least two preflight points')
        f=f[np.unique(np.linspace(0,len(f)-1,a.limit_frequencies,dtype=int))]
    r=dict(criteria=CRITERIA,cells=a.cells,fine_cells=a.fine_cells,real_arithmetic=a.real_arithmetic,notes=notes,full_modes=len(full['frequency']),projected_modes=len(small['frequency']),
        frequencies_hz=f.tolist(),rows=[],spatial=[],angular=[],quadrature=[],gates={},
        inputs_sha256={str(full_path):reference.digest(full_path),str(small_path):reference.digest(small_path)},
        parent_reports_sha256={str(a.mechanical_report):reference.digest(a.mechanical_report),str(a.projection_report):reference.digest(a.projection_report),str(a.state_report):reference.digest(a.state_report)},
        source_sha256={n:reference.digest(Path(__file__).with_name(n)) for n in ['audit-concert-grand-loaded-projection.py','concert_grand_matrixfree_air.py','concert_grand_surface_impedance.py','concert_grand_cell_observer.py','concert_grand_orthotropic_plate.py']},
        solver='Right-preconditioned block GMRES with original force residual approval',
        scope='Continuous frequency-domain one-face baffled constant-cell Rayleigh load, reciprocal feedback on both parent and projected board. Sparse harmonic controls, not dense band proof, causal realization or audio.')
    if a.real_arithmetic:r['source_sha256']['concert_grand_real_surface.py']=reference.digest(Path(__file__).with_name('concert_grand_real_surface.py'))
    a.output.mkdir(parents=True);reference.write(a.report,r)
    allY=[];allP=[];allSmallY=[];allSmallP=[];cached={}
    print('loaded harmonic controls',len(f),'frequencies',len(notes),'ports',flush=True)
    for hz in f:
        begin=time.monotonic();q,Y,P,stats=evaluate(full,full_surface,hz,B)
        qq,y,p0,ss=evaluate(small,small_surface,hz,b,red)
        row=dict(frequency_hz=float(hz),mobility_relative=port_errors(y,Y),pressure_relative=port_errors(p0,P),full=stats,projected=ss,seconds=time.monotonic()-begin)
        r['rows'].append(row);allY.append(Y);allP.append(P);allSmallY.append(y);allSmallP.append(p0)
        if float(hz) in CHECKS:cached[float(hz)]=(q,Y,P,stats)
        print('loaded',round(float(hz),6),'relative',max(row['mobility_relative']),max(row['pressure_relative']),'steps',stats['iterations'][0],ss['iterations'][0],'seconds',round(row['seconds'],3),flush=True)
        r['peak_working_set_bytes']=peak_memory();reference.write(a.report,r)
    r['gates']['loaded_transfer']=max(v for row in r['rows'] for family in ['mobility_relative','pressure_relative'] for v in row[family])<CRITERIA['loaded_port_relative']
    r['gates']['solve_residual']=max(v for row in r['rows'] for which in ['full','projected'] for v in row[which]['force_residual'])<CRITERIA['force_residual']
    r['gates']['power_balance']=max(v for row in r['rows'] for which in ['full','projected'] for v in row[which]['power_balance_relative'])<CRITERIA['power_balance']
    r['gates']['reciprocity']=max(row[which]['reciprocity'] for row in r['rows'] for which in ['full','projected'])<CRITERIA['reciprocity']
    r['gates']['passive_ports']=min(row[which]['minimum_dissipation'] for row in r['rows'] for which in ['full','projected'])>=CRITERIA['minimum_dissipation']
    # A limited run is a preflight only and cannot approve spatial/angular gates.
    if not a.limit_frequencies:
        fine_surface=Surface(full,a.fine_cells)
        for hz in CHECKS:
            if hz not in cached:cached[hz]=evaluate(full,full_surface,hz,B)
            q,Y,P,stats=cached[hz]
            _,fy,fp,fs=evaluate(full,fine_surface,hz,B)
            result=dict(frequency_hz=hz,cells_fine=a.fine_cells,mobility_relative=port_errors(Y,fy),pressure_relative=port_errors(P,fp),fine_solve=fs)
            r['spatial'].append(result);print('spatial',hz,max(result['mobility_relative']),max(result['pressure_relative']),flush=True)
            if hz in ANGULAR:
                velocity=2j*np.pi*hz*q
                angular=full_surface.angular_power(hz,velocity,96,256);finer=full_surface.angular_power(hz,velocity,128,320)
                force=np.array(stats['air_power']);error=(abs(angular-force)/np.maximum(abs(force),1e-40)).tolist()
                change=(abs(angular-finer)/np.maximum(abs(finer),1e-40)).tolist()
                r['angular'].append(dict(frequency_hz=hz,rayleigh_mean_watts=force.tolist(),angular_mean_watts=angular.tolist(),fine_angular_mean_watts=finer.tolist(),relative=error,refinement=change))
                print('angular',hz,max(error),max(change),flush=True)
            if hz in [261.625565,3600.,6000.]:
                q12,y12,p12,s12=evaluate(full,full_surface,hz,B,order=12)
                r['quadrature'].append(dict(frequency_hz=hz,mobility_relative=port_errors(Y,y12),pressure_relative=port_errors(P,p12),solve=s12,
                    mic_4_to_8_relative=port_errors(P,full_surface.pressure(hz,q,reference.MICS,8))))
            r['peak_working_set_bytes']=peak_memory();reference.write(a.report,r)
        r['gates']['spatial']=max(v for row in r['spatial'] for family in ['mobility_relative','pressure_relative'] for v in row[family])<CRITERIA['spatial_relative']
        r['gates']['angular']=max(v for row in r['angular'] for v in row['relative'])<CRITERIA['angular_relative']
        r['gates']['angular_refinement']=max(v for row in r['angular'] for v in row['refinement'])<CRITERIA['angular_refinement_relative']
        r['gates']['quadrature']=max(v for row in r['quadrature'] for family in ['mobility_relative','pressure_relative','mic_4_to_8_relative'] for v in row[family])<CRITERIA['quadrature_relative']
    path=a.output/'Transferencias-carga-directa.npz'
    np.savez_compressed(path,frequency_hz=f,notes=notes,mobility_full=allY,pressure_full=allP,mobility_projected=allSmallY,pressure_projected=allSmallP)
    r.update(transfer_path=str(path),transfer_sha256=reference.digest(path),seconds=time.monotonic()-start,
        all_gates_passed=not a.limit_frequencies and all(r['gates'].values()),preflight_only=bool(a.limit_frequencies),peak_working_set_bytes=peak_memory(),
        next='Causal passive load construction and projected-loss time integration remain before new coupled listening. Sparse frequency gates do not certify the full band.')
    reference.write(a.report,r);print('finished',json.dumps(r['gates']),'seconds',r['seconds'],flush=True)

if __name__=='__main__':main()
