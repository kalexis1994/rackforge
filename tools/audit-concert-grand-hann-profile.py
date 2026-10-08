"""Bind the two-rate local-V/original-A profile to completed measurements.

This closes a separate observer control, not old band/temporal/mesh gates.
Keep experimental local-A failure and all original reports intact.
"""
import argparse
import importlib.util
import json
from pathlib import Path
from concert_grand_wave_cache import digest
from concert_grand_hann_observation import profile_controls

spec = importlib.util.spec_from_file_location('hann_profile_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--coarse', type=Path, required=True)
    p.add_argument('--fine', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    coarse, fine = [json.loads(p.read_text(encoding='utf-8')) for p in [a.coarse, a.fine]]
    for r in [coarse, fine]:
        parent.prior.verify(r)
    if (coarse['stage'] != 'coarse local Hann and saved 16-kHz mesh diagnostic completed'
            or fine['stage'] != 'cell refinement at 24000 Hz'
            or coarse['board_sha256'] != fine['board_sha256']
            or coarse['cache_R_sha256'] != fine['cache_R_sha256']
            or coarse['cache_manifest_sha256'] != fine['cache_manifest_sha256']
            or not coarse['local_controls']['contact_reproduction']
            or not coarse['local_controls']['source_and_cache_unchanged']
            or not all(fine['canonical_endpoint_exact'])):
        raise ValueError('completed matched physical measurements required')
    rows = profile_controls(coarse, fine)
    r = dict(stage='two-rate Hann local-velocity original-acceleration profile completed',
        all_gates_passed=False, preflight_only=True, criteria=coarse['criteria'],
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name, 'concert_grand_hann_observation.py']},
        parent_sha256={str(p):digest(p) for p in [a.coarse, a.fine]},
        cases=rows, profile_oracle_passed=all(x['velocity_local_passed'] and x['acceleration_original_passed'] for x in rows),
        original_diagnostic_gates=coarse['original_diagnostic_gates'],
        scope='Nine original oracle frequencies per rate, local V and established '
              'analytic A of the same live Hermite source/window. No new dynamics '
              'or replacement of temporal, spectral-band, spatial/operator gates.',
        decision='Keep original analytic A; experimental local A failed 1e-10 '
                 'at coarse 24 kHz. Preserve that failure and its source report.')
    parent.prior.verify(r)
    a.report.parent.mkdir(parents=True, exist_ok=True)
    a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')
    print(json.dumps(dict(passed=r['profile_oracle_passed'], cases=rows)))


if __name__ == '__main__':
    main()
