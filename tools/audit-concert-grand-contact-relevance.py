"""Measure absolute contact-error relevance without rerunning dynamics.

Preserves the original failed relative-band gates. No auditory threshold,
source normalization, new quadrature, or gain changes.
"""
import argparse
import importlib.util
import json
from pathlib import Path
from concert_grand_wave_cache import digest
from concert_grand_band_relevance import band_error_contributions

spec = importlib.util.spec_from_file_location('contact_relevance_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--parent', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    old = json.loads(a.parent.read_text(encoding='utf-8'))
    if (old['stage'] != 'full-contact high-band source and directional load diagnostic completed'
            or old['rates'] != [1536000, 3072000] or len(old['cases']) != 2):
        raise ValueError('completed matched contact diagnostic required')
    parent.prior.verify(old)
    finest = old['cases'][-1]['source_and_pressure']
    result = {label:{name:band_error_contributions(
        finest[label]['integrals_by_step_hz']['31.25'][name], values)
        for name, values in observations.items()}
        for label, observations in old['contact_temporal_relative'].items()}
    r = dict(stage='absolute contact error contribution diagnostic completed',
        all_gates_passed=False, preflight_only=True,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name, 'concert_grand_band_relevance.py']},
        parent_sha256={str(a.parent):digest(a.parent)},
        criteria=old['criteria'], original_diagnostic_gates=old['diagnostic_gates'],
        scope='Same raw trapezoid partition and errors; no failed gate replaced, '
              'no source scaling, perceptibility claim or physical integration.',
        contact_temporal_error_contributions=result)
    parent.prior.verify(r)
    a.report.parent.mkdir(parents=True, exist_ok=True)
    a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')
    print(json.dumps(result['hann']['pressure']))


if __name__ == '__main__':
    main()
