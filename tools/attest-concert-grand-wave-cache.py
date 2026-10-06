"""Bind a freshly measured legacy R to its realized provider, without copying R."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path
from types import SimpleNamespace

import numpy as np
from concert_grand_projected_support import ModalSupportMap, projected_matrices
from concert_grand_real_surface import RealArithmeticSurface
from concert_grand_streaming_air import SurfaceFactorBlocks
from concert_grand_pade_air import FourthFactorBlocks
from concert_grand_wave_cache import digest, attest_legacy_cache, load_wave_cache


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['full-report','prefix-report','board','cache','source-commit','report']:
        p.add_argument('--'+name, required=True, type=str if name=='source-commit' else Path)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('new attestation report required')
    parent = json.loads(a.full_report.read_text())
    measured = json.loads(a.prefix_report.read_text())
    if (not parent['harmonic_gates_passed'] or digest(a.board)!=parent['board_sha256']
            or digest(a.full_report) not in measured['parent_sha256'].values()):
        raise ValueError('unchanged approved full board/load required')
    # The cache helper was hardened after the running audit's source freeze.
    # Verify every actual audit source against that Git snapshot, including
    # the air constants module not listed by the original full-load auditor.
    for name, sha in measured['source_sha256'].items():
        data = subprocess.check_output(['git','show',a.source_commit+':tools/'+name])
        if hashlib.sha256(data).hexdigest()!=sha:
            raise ValueError('measurement sources differ from Git snapshot: '+name)
    physical = set(parent['source_sha256']) | {'concert_grand_surface_impedance.py'}
    for name in physical:
        frozen = subprocess.check_output(['git','show',a.source_commit+':tools/'+name])
        if Path(__file__).with_name(name).read_bytes()!=frozen:
            raise ValueError('physical source changed since measured construction: '+name)
    with np.load(a.board) as d:
        mechanical = {name:d[name] for name in [
            'frequency','low_count','tail_mass','tail_stiffness','tail_damping','low_damping']}
        geometry = dict(panel=json.loads(str(d['panel'])),boundary=str(d['boundary']),
                        nx=int(d['nx']),ny=int(d['ny']),vectors=d['ritz_vectors'])
    modal = ModalSupportMap.from_matrices(*projected_matrices(mechanical))
    geometry['vectors'] = geometry['vectors'] @ modal.basis
    surface = RealArithmeticSurface(geometry,parent['cells'])
    shapes = surface.forward(np.eye(surface.modes))
    plain = SimpleNamespace(shapes=shapes,modes=surface.modes,nx=surface.nx,ny=surface.ny,
                            hx=surface.hx,hy=surface.hy,length=surface.length,width=surface.width,
                            density=surface.density,speed=surface.speed)
    provider = FourthFactorBlocks(SurfaceFactorBlocks(plain,parent['rate'],*parent['angular'],32,fold_y=True))
    identity = dict(format=1,board_sha256=digest(a.board),full_report_sha256=digest(a.full_report),
                    physical_source_sha256=parent['source_sha256'],cells=parent['cells'],rate=parent['rate'],
                    angular=parent['angular'],derivative='fourth',fold_y=True)
    proof = attest_legacy_cache(a.cache,provider,identity,a.prefix_report)
    # Verify the hardened read path immediately, with the same realized basis.
    loaded, checked = load_wave_cache(a.cache,provider,identity)
    del loaded
    r = dict(source_commit=a.source_commit,parent_sha256={str(a.prefix_report):digest(a.prefix_report),
             str(a.full_report):digest(a.full_report)},board_sha256=digest(a.board),
             R_sha256=checked['R_sha256'],manifest_sha256=digest(a.cache/'manifest.json'),
             attestation=proof.get('provider_attestation'),provider_signature_checked=True,
             correlation_bytes_unchanged=True,
             source_sha256={name:digest(Path(__file__).with_name(name)) for name in [
                 Path(__file__).name,'concert_grand_wave_cache.py']},
             scope='Explicit provider binding of this session legacy cache only; no numerical or timbral change.')
    a.report.parent.mkdir(parents=True,exist_ok=True)
    a.report.write_text(json.dumps(r,indent=2,allow_nan=False),encoding='utf-8')
    print('cache provider attested',r['manifest_sha256'],flush=True)


if __name__=='__main__':
    main()
