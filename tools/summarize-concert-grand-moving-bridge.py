"""Reproduce comparisons and provenance for the reciprocal bridge pilot."""
import argparse,hashlib,json,platform,zipfile
from pathlib import Path
import numpy as np
import scipy


def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()


def compare(root,left_folder,left_key,right_folder,right_key,reports):
    left=reports[left_folder]['cases'][left_key];right=reports[right_folder]['cases'][right_key]
    fields=('final_string_energy_j','final_support_energy_j','final_work_into_support_j','contact_time_s')
    relative={k:abs(left[k]-right[k])/max(abs(right[k]),1e-30) for k in fields}
    a=np.loadtxt(root/left_folder/f'{left_key}.csv',delimiter=',',skiprows=1)
    b=np.loadtxt(root/right_folder/f'{right_key}.csv',delimiter=',',skiprows=1)
    np.testing.assert_array_equal(a[:,0],b[:,0])
    for column,name in ((1,'bridge_displacement'),(2,'bridge_velocity'),(3,'raw_interface_force')):
        relative[name+'_relative_rms']=float(np.linalg.norm(a[:,column]-b[:,column])/np.linalg.norm(b[:,column]))
    passed=(all(relative[k]<.002 for k in fields[:3]) and relative['contact_time_s']<.001
            and relative['bridge_displacement_relative_rms']<.01 and relative['bridge_velocity_relative_rms']<.01)
    return dict(left=left_key,right=right_key,relative_errors=relative,mechanical_output_gate_passed=passed)


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--input',type=Path,required=True);parser.add_argument('--build',type=Path,required=True)
    parser.add_argument('--p2-asset',type=Path,required=True);args=parser.parse_args()
    folders=('scalar','resolution','resolution-fine','p2','p2-fine','p2-finest')
    reports={f:json.loads((args.root/f/'report.json').read_text(encoding='utf-8')) for f in folders}
    scalar=[]
    for a,b in ((12,24),(24,48),(48,72)):
        scalar.append(compare(args.root,'resolution',f'60-124-Coupled-loss-T{a}',
            'resolution-fine' if b==72 else 'resolution',f'60-124-Coupled-loss-T{b}',reports))
    p2=[compare(args.root,a,f'60-124-P2-low-band-T{n}',b,f'60-124-P2-low-band-T{m}',reports)
        for a,n,b,m in (('p2',12,'p2-fine',48),('p2-fine',48,'p2-finest',72))]
    all_cases=[c for r in reports.values() for c in r['cases'].values()]
    tools=Path(__file__).resolve().parent
    sources=[tools/name for name in ('concert_grand_moving_bridge.py','audit-concert-grand-moving-bridge.py',
        'summarize-concert-grand-moving-bridge.py','concert_grand_coupled_contact.py','concert_grand_contact_study.py',
        'concert_grand_nonlinear_string.py','concert_grand_orthotropic_plate.py','audit-concert-grand-coupled-contact.py',
        'tests/test_concert_moving_bridge.py')]
    package=Path('C:/SHARED/RF-Concert-Grand-0.172.0.rfplugin')
    summary=dict(scope='Mechanical vertical bridge pilot, not an identified piano or audible candidate',
        limitations=['Euler bending; one string; fixed axial/horizontal endpoints','Unidentified support geometry and felt footprint',
                     'P2 uses only 35 modes below 800 Hz; no acoustic pressure observer','Four milliseconds from rest; no backcheck or pedals'],
        solver=dict(method='DOP853',rtol=1e-9,atol=1e-13,duration_s=.004,max_step='1/(8*highest linear structural frequency)'),
        gates=dict(energy_and_work_relative=.002,contact_time_relative=.001,bridge_displacement_and_velocity_relative_rms=.01,
                   raw_interface_force='reported diagnostic, not an acoustic or full-force convergence gate'),
        environment=dict(python=platform.python_version(),numpy=np.__version__,scipy=scipy.__version__),
        maximum_energy_balance_relative_error=max(c['maximum_balance_relative_error'] for c in all_cases),
        maximum_support_work_balance_relative_error=max(c['support_work_balance_relative_error'] for c in all_cases),
        maximum_interface_force_residual_n=max(c['maximum_interface_force_residual'] for c in all_cases),
        scalar_resolution=scalar,p2_resolution=p2,p2_support=reports['p2']['support'],
        scalar_fine=reports['resolution-fine']['cases'],p2_fine=reports['p2-finest']['cases'],
        matched_support_controls=reports['scalar']['cases'],forced_support_ablation=reports['scalar']['forced_support_ablation'],
        source_sha256={str(p.relative_to(tools.parent)).replace('\\','/'):digest(p) for p in sources},
        input_sha256={p.name:digest(p) for p in sorted(args.input.glob('*.csv'))},
        p2_asset_sha256=digest(args.p2_asset),report_sha256={f:digest(args.root/f/'report.json') for f in folders},
        factory_package_sha256=digest(package))
    assert summary['factory_package_sha256']=='ecca3e6381a130e8db088315f406296589f30a5a2ee75475acec176a1b9a55cb'
    assert scalar[-1]['mechanical_output_gate_passed'] and p2[-1]['mechanical_output_gate_passed']
    args.build.mkdir(parents=True,exist_ok=True)
    archive=args.build/'source-audit.zip'
    with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as out:
        for p in sources:out.write(p,str(p.relative_to(tools.parent)))
        for p in sorted(args.input.glob('*.csv')):out.write(p,'input/'+p.name)
        out.write(args.p2_asset,'support/'+args.p2_asset.name)
    summary['source_archive_sha256']=digest(archive)
    (args.root/'resumen.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
    print(json.dumps(dict(scalar_resolution=scalar,p2_resolution=p2,
        maximum_energy_balance_relative_error=summary['maximum_energy_balance_relative_error'],
        source_archive_sha256=summary['source_archive_sha256']),indent=2))


if __name__=='__main__':main()
