"""Authenticate collocation stage clocks against ordinary and frozen builds."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-report', type=Path, required=True)
    parser.add_argument('--reference-ref', required=True)
    parser.add_argument('--local', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    if args.local.exists() or args.report.exists():
        raise ValueError('fresh paths required')
    report_path = args.reference_report.resolve().relative_to(root).as_posix()
    if subprocess.check_output(['git','show',args.reference_ref+':'+report_path],cwd=root)!=args.reference_report.read_bytes():
        raise ValueError('reference report changed')
    old = json.loads(args.reference_report.read_text())
    for name, sha in old['source_sha256'].items():
        if hashlib.sha256(subprocess.check_output(['git','show',args.reference_ref+':tools/'+name],cwd=root)).hexdigest()!=sha:
            raise ValueError('frozen source mismatch: '+name)
    controls = [next(c for c in old['cases'] if c['label']==f'prepared-{rate}-O{order}')
                for rate,order in [(128000,24),(256000,16),(512000,8)]]
    for c in controls:
        for prefix in ['result','coefficient']:
            if digest(c[prefix+'_path'])!=c[prefix+'_sha256']:
                raise ValueError('reference data changed')
    frozen = old['executable_path']
    if digest(frozen)!=old['executable_sha256']:
        raise ValueError('frozen executable changed')
    names = set(old['source_sha256'])|{'concert-grand-phase-profile.rs',Path(__file__).name}
    sources = {name:digest(root/'tools'/name) for name in sorted(names)}
    args.local.mkdir(parents=True)
    plain = args.local/'plain.exe'; profiled = args.local/'profile.exe'
    for exe, flags in [(plain,[]),(profiled,['--cfg','concert_phase_profile'])]:
        subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native','--cfg','concert_phase_library',
                        *flags,str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(exe)],check=True)
    record = dict(source_sha256=sources,reference_ref=args.reference_ref,
                  reference_report_path=str(args.reference_report.resolve()),reference_report_sha256=digest(args.reference_report),
                  reference_sources_authenticated_from_git=True,reference_reintegrated=False,
                  plain_executable_path=str(plain.resolve()),plain_executable_sha256=digest(plain),
                  profile_executable_path=str(profiled.resolve()),profile_executable_sha256=digest(profiled),
                  frozen_executable_path=frozen,frozen_executable_sha256=digest(frozen),
                  fidelity_checked_duration_s=.006,benchmark_duration_s=.008,
                  longer_duration_fidelity_approved=False,full_piano_realtime=False,cases=[],benchmarks=[])
    keys = ['q','v','work_relative_max','numerical_lambda_abs_max','maximum_iterations',
            'midpoint_residual_relative_max','initial_wire_energy_j','final_wire_energy_j']
    bench_control = None

    def run(label, exe, control, buffers=0):
        nonlocal bench_control
        rate=control['rate_hz']; order=control['order']; microframes=rate//8000
        mode = ['bench','1',str(order),str(buffers),f'--buffer-microframes={microframes}'] if buffers else ['verify','1',str(order)]
        command=[str(exe),control['coefficient_path'],*mode,'--collocation']
        process=subprocess.run(command,capture_output=True,text=True,check=True)
        data=json.loads(process.stdout); output=args.local/(label+'.json')
        output.write_text(process.stdout,encoding='utf-8')
        if buffers:
            if bench_control is None:
                bench_control={k:data[k] for k in keys}
            same = all(data[k]==bench_control[k] for k in keys) if rate==512000 else None
        else:
            frozen_data=json.loads(Path(control['result_path']).read_text())
            same=all(data[k]==frozen_data[k] for k in keys)
        entry={k:v for k,v in data.items() if k not in ['q','v']}
        entry.update(label=label,rate_hz=rate,order=order,command=command,executable_sha256=digest(exe),
                     coefficient_path=control['coefficient_path'],coefficient_sha256=control['coefficient_sha256'],
                     result_path=str(output.resolve()),result_sha256=digest(output),arrays_and_work_equal=same)
        record['benchmarks' if buffers else 'cases'].append(entry)
        record['sources_unchanged']=all(digest(root/'tools'/name)==sha for name,sha in sources.items())
        args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
        print(label,data['seconds'],data['mean_ms'],same,flush=True)
        if not data['completed'] or data['allocations']!=0 or same is False:
            raise ValueError('profile changed state or processing failed')

    for c in controls:
        run(f'plain-{c["rate_hz"]}',plain,c)
        run(f'profile-{c["rate_hz"]}',profiled,c)
    c=controls[-1]
    for label,exe in [('frozen-benchmark',frozen),('plain-benchmark',plain),('profile-benchmark',profiled)]:
        run(label,exe,c,64)
    run('profile-256k',profiled,controls[1],64)
    run('profile-128k',profiled,controls[0],64)
    for label,exe in [('plain-repeat',plain),('profile-repeat',profiled),('frozen-repeat',frozen)]:
        run(label,exe,c,64)


if __name__ == '__main__':
    main()
