"""Measure a loaded mobility diagnostic with exact held-force Rust steps.

Fidelity failures stay failures. This timing excludes microphone pressure,
strings, hammer, pedals and port closure; it never approves a whole piano.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import time
import numpy as np
from concert_grand_realtime_body import Body, modal_response
from concert_grand_modal_runtime import ModalRuntime
from concert_grand_reciprocal_runtime import ReciprocalRuntime
from concert_grand_wave_cache import digest

spec = importlib.util.spec_from_file_location('spatial_checks', Path(__file__).with_name('distill-concert-grand-spatial-body.py'))
prior = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prior)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--body-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists() or args.local.exists():
        raise ValueError('new runtime diagnostic paths required')
    root = Path(__file__).resolve().parent.parent
    parent = json.loads(args.body_report.read_text())
    if digest(parent['model_path']) != parent['model_sha256']:
        raise ValueError('measured loaded model changed')
    for name, sha in parent['source_sha256'].items():
        if digest(root/'tools'/name) != sha:
            raise ValueError('measured parent sources changed')
    names = [Path(__file__).name, 'concert_grand_realtime_body.py', 'concert_grand_modal_runtime.py',
             'concert_grand_reciprocal_runtime.py', 'concert-grand-reciprocal-body.rs',
             'concert-grand-spatial-body.rs', 'distill-concert-grand-spatial-body.py']
    hashes = {name:digest(root/'tools'/name) for name in names}
    started = time.perf_counter()
    args.local.mkdir(parents=True)
    wrapper = (root/'tools/concert-grand-spatial-body.rs').read_text()
    marker = 'include!("concert-grand-modal-body.rs");'
    if wrapper.count(marker) != 1:
        raise ValueError('native wrapper marker changed')
    generated = args.local/'native-loaded.rs'
    generated.write_text(wrapper.replace(marker, (root/'tools/concert-grand-reciprocal-body.rs').read_text()))
    generated_sha = digest(generated)
    executable = args.local/'loaded-mobility-bench.exe'
    subprocess.run(['rustc','--edition=2021','-O','-C','target-cpu=native',str(generated),'-o',str(executable)], check=True)
    generated.unlink()
    with np.load(parent['model_path']) as data:
        body = Body.mechanical(data['omega'], data['damping'], data['ports'])
        mapping = data['mapping']
        modal = ModalRuntime.from_body(body)
        canonical = ReciprocalRuntime(modal)
        frequency = np.geomspace(23.,5970.,111)
        target = modal_response(data['omega'],data['damping'],data['ports'],frequency)
        actual = canonical.modal.response(frequency, list(range(data['ports'].shape[1])))
        equivalent = float(np.linalg.norm(actual-target)/np.linalg.norm(target))
        if equivalent > 1e-9:
            raise ValueError('exact modal realization changed loaded transfer')
    map_path = args.local/'ports.bin'
    with map_path.open('xb') as stream:
        stream.write(struct.pack('<8sII',b'RFPRT001',*mapping.shape))
        stream.write(np.asarray(mapping,dtype='<f8').tobytes())
    rates = []
    for rate in [44100,48000]:
        bundle = args.local/f'body-{rate}.bin'
        step = canonical.write(bundle,rate)
        checks = prior.verify(prior.run(executable,bundle,map_path,'verify'), canonical.modal, canonical.modal, step,mapping)
        bench = prior.run(executable,bundle,map_path,'bench')
        fraction = bench['p99_ms']/bench['deadline_ms']
        bench.update(p99_deadline_fraction=fraction,
                     observed_body_budget_passed=fraction<=.20 and bench['max_ms']<=bench['deadline_ms'] and bench['callback_allocations']==0)
        rates.append(dict(coefficient_path=str(bundle.resolve()), coefficient_sha256=digest(bundle),
            coefficient_bytes=bundle.stat().st_size, verification=checks, benchmark=bench))
    report = dict(stage='loaded mobility diagnostic native runtime measured', seconds=time.perf_counter()-started,
        body_report_sha256=digest(args.body_report), source_sha256=hashes,
        expanded_native_source_sha256=generated_sha, native_executable_sha256=digest(executable),
        rustc=subprocess.check_output(['rustc','--version'],text=True).strip(),
        compiler_flags=['--edition=2021','-O','-C','target-cpu=native'],
        modes=len(body.a)//2, native_resonators=len(canonical.modal.roots), latent_ports=mapping.shape[1],
        mapping_sha256=digest(map_path), exact_modal_transfer_relative=equivalent,
        symmetric_residue_defect_relative=canonical.reciprocity_defect_relative, rates=rates,
        parent_mobility_gate_passed=bool(parent['mobility_gate_passed']),
        body_20percent_budget_passed_both_rates=all(r['benchmark']['observed_body_budget_passed'] for r in rates),
        diagnostic_only=True, pressure_included=False, full_piano_realtime=False,
        global_physics_approved=False, factory_updated=False,
        source_unchanged=all(digest(root/'tools'/n)==sha for n,sha in hashes.items()),
        verification_scope='Native final state/checksum/work parity; per-step energy on reconstructed Python trajectory. Host-specific, no WASM or live port closure.')
    args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print('loaded native diagnostic', [(r['benchmark']['p99_ms'],r['benchmark']['p99_deadline_fraction']) for r in rates],flush=True)


if __name__ == '__main__':
    main()
