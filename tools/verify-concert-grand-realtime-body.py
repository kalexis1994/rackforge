"""Authenticate the P7 measurements and retained diagnostic without rerendering."""
import hashlib
import json
from pathlib import Path


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    root = Path(__file__).resolve().parent.parent
    folder = root/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P7-cuerpo-realtime-2026-10-12'
    source = root/'local/concert-grand-physics/P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    teacher_sha = digest(source)
    paths = [folder/(name+'.json') for name in ['destilacion', 'modal', 'espacial', 'estatico', 'reciproco', 'vector', 'avx2', 'ilp']]
    report_hashes = {path.name: digest(path) for path in paths}
    native_controls = 0
    for path in paths:
        row = json.loads(path.read_text())
        if row['teacher_sha256'] != teacher_sha or not row['source_and_teacher_unchanged']:
            raise ValueError('teacher integrity failed')
        for name, sha in row.get('source_sha256', row.get('sources_sha256', {})).items():
            if digest(root/'tools'/name) != sha:
                raise ValueError('measured source changed: '+name)
        if row['full_piano_realtime'] or row['global_physics_approved']:
            raise ValueError('component misrepresented as a complete piano')
        for key in ['parent_report_sha256', 'modal_parent_sha256']:
            if key in row and row[key] not in report_hashes.values():
                raise ValueError('broken report provenance')
        for case in row['cases']:
            rms = case.get('mobility_rms_relative', case.get('complex_mobility_rms_relative'))
            worst = case.get('mobility_worst_relative', case.get('complex_mobility_worst_relative'))
            fidelity = rms <= .05 and worst <= .10
            if fidelity != case['transfer_gate_passed']:
                raise ValueError('fidelity gate drift')
            budgets = []
            for rate in case['rates']:
                budget = rate['benchmark']
                passed = (budget['p99_ms']/budget['deadline_ms'] <= .20
                          and budget['max_ms'] <= budget['deadline_ms']
                          and budget['callback_allocations'] == 0)
                if passed != budget['observed_body_budget_passed']:
                    raise ValueError('CPU reserve gate drift')
                budgets.append(passed)
                checks = rate['verification']
                if not any(checks.get(key) for key in ['native_python_state_and_output_passed',
                    'native_python_modal_states_outputs_and_work_passed',
                    'native_python_final_states_outputs_and_work_passed']):
                    raise ValueError('missing native/Python verification')
                energy = checks.get('python_reconstructed_step_energy_excess_relative',
                                    checks.get('maximum_step_energy_excess_relative'))
                if energy is None or energy > 1e-10:
                    raise ValueError('component step energy failed')
                native_controls += 1
            if all(budgets) != case['body_budget_passed_both_rates']:
                raise ValueError('joint rate budget drift')
            if 'both_fidelity_and_body_budget_passed' in case and case['both_fidelity_and_body_budget_passed'] != (fidelity and all(budgets)):
                raise ValueError('joint gate drift')
    candidate_path = folder/'candidato.json'
    candidate = json.loads(candidate_path.read_text())
    if digest(root/'tools/export-concert-grand-diagnostic-body.py') != candidate['exporter_sha256']:
        raise ValueError('candidate exporter changed')
    if (candidate['measured_report_sha256'] != report_hashes['reciproco.json']
            or candidate['teacher_sha256'] != teacher_sha or not candidate['diagnostic_only']
            or candidate['strict_body_20percent_budget_passed'] or candidate['full_piano_realtime']
            or candidate['factory_updated'] or candidate['global_physics_approved']):
        raise ValueError('diagnostic scope or preserved failed gate changed')
    if digest(candidate['mapping_path']) != candidate['mapping_sha256']:
        raise ValueError('retained map changed')
    measured = next(c for c in json.loads(paths[4].read_text())['cases'] if c['latent_ports'] == 20)
    for rate, original in zip(candidate['rates'], measured['rates']):
        path = Path(rate['coefficient_path'])
        if (digest(path) != rate['coefficient_sha256'] or rate['coefficient_sha256'] != original['coefficient_sha256']
                or path.stat().st_size != rate['coefficient_bytes']
                or rate['previously_measured_benchmark'] != original['benchmark']):
            raise ValueError('diagnostic coefficients/timings changed')
        bench = original['benchmark']
        if bench['p99_ms'] >= bench['deadline_ms'] or bench['max_ms'] >= bench['deadline_ms']:
            raise ValueError('standalone diagnostic deadline failed')
    result = dict(stage='independent P7 source/data/gate verification', verifier_sha256=digest(__file__),
        source_teacher_report_hashes_passed=True, reports_sha256=report_hashes,
        candidate_report_sha256=digest(candidate_path), retained_coefficients_hash_identical=True,
        native_python_controls_recorded=native_controls, fidelity_and_cpu_gates_recomputed=True,
        failed_20percent_body_reserve_preserved=True, full_piano_realtime=False,
        scope='Authentication and scalar gate arithmetic only; no independent modal replay or new benchmark, no air/contact/pedal/pressure/polyphonic/WASM certificate.')
    (folder/'verificacion.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
