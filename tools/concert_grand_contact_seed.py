"""One authenticated living contact endpoint, without copying R or traces."""
import json
from pathlib import Path
import numpy as np
from concert_grand_wave_cache import digest


def save_seed(path,state,bindings):
    path=Path(path);manifest=path.with_suffix('.json')
    if path.exists() or manifest.exists():raise FileExistsError('existing seed must not be overwritten')
    if not isinstance(bindings,dict) or not bindings:raise ValueError('nonempty provenance bindings required')
    arrays=tuple(np.asarray(x) for x in state)
    if len(arrays)!=4 or any(x.dtype!=np.float64 or not x.size or not np.isfinite(x).all() for x in arrays):
        raise ValueError('four finite nonempty float64 states required')
    # Validate serializability before creating either file; no pickle/object arrays.
    encoded=json.dumps(bindings,sort_keys=True,allow_nan=False)
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.open('xb') as stream:np.savez(stream,**{f'state{i}':x for i,x in enumerate(arrays)})
    metadata=dict(schema='concert-grand-living-contact-seed-v1',bindings=json.loads(encoded),
                  payload_sha256=digest(path),shapes=[list(x.shape) for x in arrays])
    with manifest.open('x',encoding='utf-8') as stream:json.dump(metadata,stream,indent=2,allow_nan=False)
    return load_seed(path,bindings)


def load_seed(path,bindings):
    path=Path(path);manifest=path.with_suffix('.json');metadata=json.loads(manifest.read_text())
    if (metadata.get('schema')!='concert-grand-living-contact-seed-v1'
            or metadata.get('bindings')!=bindings or digest(path)!=metadata.get('payload_sha256')):
        raise ValueError('seed provenance or payload mismatch')
    with np.load(path,allow_pickle=False) as data:
        if set(data.files)!={f'state{i}' for i in range(4)}:raise ValueError('four seed states required')
        state=tuple(data[f'state{i}'].copy() for i in range(4))
    if (len(metadata['shapes'])!=4 or any(x.dtype!=np.float64 or list(x.shape)!=shape
            or not x.size or not np.isfinite(x).all() for x,shape in zip(state,metadata['shapes']))):
        raise ValueError('seed dimensions or values mismatch')
    return state,dict(payload_sha256=metadata['payload_sha256'],manifest_sha256=digest(manifest),
                      payload_bytes=path.stat().st_size,manifest_bytes=manifest.stat().st_size)


def overlapping_intervals(actual,reference):
    """Independent observer bases may differ; require overlap of their bounds."""
    for interval in (actual,reference):
        values=[float(interval[k]) for k in ('lower_j','estimate_j','upper_j')]
        if not np.isfinite(values).all() or not 0<=values[0]<=values[1]<=values[2]:
            raise ValueError('ordered finite nonnegative interval required')
    return max(actual['lower_j'],reference['lower_j'])<=min(actual['upper_j'],reference['upper_j'])
