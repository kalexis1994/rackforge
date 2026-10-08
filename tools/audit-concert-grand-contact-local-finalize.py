"""Authenticate completed measurements after the scalar-only summarizer failed.

Keep producer and checkpoint frozen. No dynamics, duplicated vectors or
measurements; this small report binds the checkpoint and closes its controls.
"""
import argparse
import importlib.util
import json
import time
from pathlib import Path
from concert_grand_wave_cache import digest
from concert_grand_local_hann_controls import local_controls

spec = importlib.util.spec_from_file_location('local_finalize_parent',
    Path(__file__).with_name('audit-concert-grand-contact-band.py'))
parent = importlib.util.module_from_spec(spec)
spec.loader.exec_module(parent)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--checkpoint', type=Path, required=True)
    p.add_argument('--report', type=Path, required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new report required')
    start = time.perf_counter()
    old = json.loads(a.checkpoint.read_text(encoding='utf-8'))
    if old['stage'] != 'cell refinement at 24000 Hz' or old['all_gates_passed'] is not False:
        raise ValueError('expected complete measurement checkpoint required')
    parent.prior.verify(old)
    board = parent.prior.LOCAL/'P6-proyeccion-espacial-2026-10-05/Tabla-proyectada-pasiva.npz'
    cache = parent.prior.LOCAL/'P6-carga-causal-2026-10-06'
    seed = parent.prior.LOCAL/'P6-contacto96-seed-2026-10-07/Contacto-96-3072k.npz'
    proof = old['seed_provenance']
    unchanged = (digest(board) == old['board_sha256']
        and digest(cache/'R.npy') == old['cache_R_sha256']
        and digest(cache/'manifest.json') == old['cache_manifest_sha256']
        and digest(seed) == proof['payload_sha256']
        and digest(seed.with_suffix('.json')) == proof['manifest_sha256'])
    r = dict(stage='canonical fine contact local Hann and mesh diagnostic completed',
        all_gates_passed=False, preflight_only=True,
        source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
            Path(__file__).name, 'concert_grand_local_hann_controls.py']},
        parent_sha256={str(a.checkpoint):digest(a.checkpoint)},
        criteria=old['criteria'], original_diagnostic_gates=old['original_diagnostic_gates'],
        local_controls=local_controls(old, unchanged),
        measurement_seconds=old['seconds'], peak_working_set_bytes=old['peak_working_set_bytes'],
        measurement_checkpoint=str(a.checkpoint),
        scope=old['scope'],
        incident='All seven measured probes saved before scalar-only maximum_nested rejected '
                 'lists. Producer/checkpoint frozen; controls closed here without reintegration '
                 'or duplicated vectors. Original two-rate failures remain unchanged.')
    r['local_controls_passed'] = all(r['local_controls'].values())
    r['finalization_seconds'] = time.perf_counter()-start
    parent.prior.verify(r)
    a.report.parent.mkdir(parents=True, exist_ok=True)
    a.report.write_text(json.dumps(r, indent=2, allow_nan=False), encoding='utf-8')
    print(json.dumps(r['local_controls']))


if __name__ == '__main__':
    main()
