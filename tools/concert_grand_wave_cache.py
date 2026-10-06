"""One authenticated, local correlation cache; regenerate angular factors.

The immutable numerical identity includes the board, approved report and
physical construction sources. Contact/observer experiments can share R
without saving another factor. No repair or reduced coordinates on reload.
"""
import hashlib
import json
from pathlib import Path

import numpy as np
from concert_grand_streaming_air import StreamingRationalAir


def digest(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def provider_signature(provider):
    def array_signature(value):
        a = np.ascontiguousarray(value)
        return dict(shape=list(a.shape), dtype=a.dtype.str,
                    sha256=hashlib.sha256(memoryview(a).cast('B')).hexdigest())
    surface = provider.surface
    return dict(kind=type(provider).__name__, rate=provider.rate, taps=provider.taps,
                modes=provider.modes, outputs=provider.outputs,
                surface={name:float(getattr(surface,name)) for name in [
                    'hx','hy','length','width','density','speed']},
                arrays={name:array_signature(getattr(provider,name)) for name in [
                    'shapes','positions','directions','weights']})


def save_wave_cache(path, wave, identity):
    path = Path(path)
    if path.exists():
        raise ValueError('new cache directory required; never overwrite')
    if not isinstance(wave, StreamingRationalAir) or np.any(wave.tail_mass):
        raise ValueError('unmodified streaming wave required')
    path.mkdir(parents=True)
    np.save(path/'R.npy', wave.R, allow_pickle=False)
    metadata = dict(identity=identity, shape=list(wave.R.shape), rate=wave.rate,
                    outputs=wave.outputs, reciprocity_relative=float(wave.reciprocity_error),
                    dc_relative=float(wave.dc_error), R_sha256=digest(path/'R.npy'),
                    provider_signature=provider_signature(wave.provider))
    (path/'manifest.json').write_text(json.dumps(metadata, indent=2, allow_nan=False), encoding='utf-8')
    return metadata


def load_wave_cache(path, provider, identity):
    return _load(path, provider, identity, True)


def _load(path, provider, identity, require_signature):
    path = Path(path)
    metadata = json.loads((path/'manifest.json').read_text())
    if metadata['identity'] != identity:
        raise ValueError('changed numerical cache identity')
    signature = metadata.get('provider_signature')
    if require_signature and signature is None:
        raise ValueError('legacy cache requires explicit measured provider attestation')
    if signature is not None and signature != provider_signature(provider):
        raise ValueError('changed realized surface/basis/angular provider')
    if digest(path/'R.npy') != metadata['R_sha256']:
        raise ValueError('changed correlation file')
    if (metadata['rate'] != provider.rate or metadata['outputs'] != provider.outputs
            or metadata['shape'] != [provider.taps, provider.modes, provider.modes]):
        raise ValueError('different provider dimensions/rate')
    R = np.load(path/'R.npy', mmap_mode='r', allow_pickle=False)
    if R.dtype != np.float64 or list(R.shape) != metadata['shape']:
        raise ValueError('float64 correlation dimensions required')
    wave = StreamingRationalAir.__new__(StreamingRationalAir)
    wave.provider = provider
    wave.budget = provider.budget(real_gram=True)
    wave.rate = provider.rate
    wave.a = 2*wave.rate
    wave.order = provider.taps-1
    wave.modes = provider.modes
    wave.outputs = provider.outputs
    wave.states = wave.order*wave.modes
    wave.R = R
    wave.D = R[0]/2
    wave.tail_mass = np.zeros((wave.modes, wave.modes))
    wave.reciprocity_error = metadata['reciprocity_relative']
    wave.dc_error = metadata['dc_relative']
    if wave.reciprocity_error > 1e-11 or wave.dc_error > 1e-11:
        raise ValueError('failed original reciprocal/DC construction')
    return wave, metadata


def attest_legacy_cache(path, provider, identity, measured_report):
    """Attach a provider proof to this session's freshly verified legacy R.

    Explicit migration only, after construction controls completed on the
    frozen physical sources. Changes metadata, never R. This proves cache
    construction only, even if the later contact certificate failed to save.
    """
    path = Path(path)
    report = json.loads(Path(measured_report).read_text())
    measured_stages = {'independent storage/radiation certificate',
                       'independent certificate measured','contact prefix audit completed'}
    if (report.get('stage') not in measured_stages
            or Path(report.get('cache','')).resolve() != path.resolve()
            or report.get('cache_mass_control_relative',1.) >= 1e-10
            or len(report.get('cache_transfer_controls',[])) != 3
            or max(row['frozen_error_absolute'] for row in report['cache_transfer_controls']) >= 1e-10
            or identity.get('full_report_sha256') not in report.get('parent_sha256',{}).values()
            or any(report.get('source_sha256',{}).get(name) != sha
                   for name,sha in identity.get('physical_source_sha256',{}).items())):
        raise ValueError('completed matching frozen construction controls required')
    wave, metadata = _load(path,provider,identity,False)
    if metadata['R_sha256'] != report.get('cache_R_sha256'):
        raise ValueError('measurement belongs to another correlation file')
    if 'provider_signature' in metadata:
        return metadata
    old_manifest = digest(path/'manifest.json')
    metadata['provider_signature'] = provider_signature(provider)
    metadata['provider_attestation'] = dict(previous_manifest_sha256=old_manifest,
                                          measured_report_sha256=digest(measured_report),
                                          construction_only=True,measurement_stage=report['stage'])
    (path/'manifest.json').write_text(json.dumps(metadata,indent=2,allow_nan=False),encoding='utf-8')
    return metadata
