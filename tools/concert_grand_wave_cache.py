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
                    dc_relative=float(wave.dc_error), R_sha256=digest(path/'R.npy'))
    (path/'manifest.json').write_text(json.dumps(metadata, indent=2, allow_nan=False), encoding='utf-8')
    return metadata


def load_wave_cache(path, provider, identity):
    path = Path(path)
    metadata = json.loads((path/'manifest.json').read_text())
    if metadata['identity'] != identity:
        raise ValueError('changed numerical cache identity')
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
