"""Refine high-band contact integrals from immutable nested trapezoid results.

Exact Simpson weight identity on the uniform high bands; no new dynamics,
frequency samples, source scaling or threshold. Retains every other failed
control from the full-contact report, including a possible oracle failure.
"""
import argparse
import importlib.util
import json
from pathlib import Path
from concert_grand_wave_cache import digest
from concert_grand_band_quadrature import refined_band_quadrature

spec = importlib.util.spec_from_file_location('contact_quadrature_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--parent',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    old = json.loads(a.parent.read_text(encoding='utf-8'))
    if (old['stage']!='full-contact high-band source and directional load diagnostic completed'
            or old['all_gates_passed'] is not False or old['rates']!=[1536000,3072000]
            or len(old['cases'])!=2 or old['duration_s']!=.003
            or old['frequency_steps_hz']!=[125.,62.5,31.25]):
        raise ValueError('completed matched contact diagnostic required')
    parent.prior.verify(old)
    r = dict(stage='contact high-band quadrature refinement completed',all_gates_passed=False,
        preflight_only=True,criteria=old['criteria'],
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name,'concert_grand_band_quadrature.py']},
        parent_sha256={str(a.parent):digest(a.parent)},cases=[],
        scope='Exact composite Simpson weights from the same saved uniform high-band samples; '
              'low bands keep trapezoids. No new physical run or correction of other failed controls.')
    for row in old['cases']:
        r['cases'].append(dict(rate=row['rate'],quadrature={label:refined_band_quadrature(
            old['frequency_grid_hz'],old['frequency_steps_hz'],values['integrals_by_step_hz'])
            for label,values in row['source_and_pressure'].items()}))
    worst = max(parent.reference.maximum_nested(item['finest_relative'])
        for row in r['cases'] for item in row['quadrature'].values())
    r['frequency_integration_relative_maximum'] = worst
    r['frequency_integration_passed'] = bool(worst<old['criteria']['frequency_integral_relative'])
    r['original_diagnostic_gates'] = old['diagnostic_gates']
    r['diagnostic_gates_with_refined_quadrature'] = {**old['diagnostic_gates'],
        'frequency_integration':r['frequency_integration_passed']}
    r['diagnostic_gates_with_refined_quadrature_passed'] = all(r['diagnostic_gates_with_refined_quadrature'].values())
    parent.prior.verify(r)
    a.report.parent.mkdir(parents=True,exist_ok=True)
    a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print(json.dumps(dict(maximum=worst,gates=r['diagnostic_gates_with_refined_quadrature'])))


if __name__=='__main__':
    main()
