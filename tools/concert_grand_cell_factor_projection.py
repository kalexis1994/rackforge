"""Observation-only linear projection before the spatial histograms.

Keep the authenticated provider immutable and regenerate its COMPLETE factor
for the independent R[0] norm check. A private geometry view builds L(shapes Q)
with the same cells, weights, direction ordering, reversal and time derivative.
It cannot provide a physical load. The original L(shapes) Q remains the oracle.
"""
from copy import copy
import time
import numpy as np
from concert_grand_streaming_air import SurfaceFactorBlocks
from concert_grand_pade_air import FourthFactorBlocks


def projected_cell_provider(provider,Q):
    Q = np.asarray(Q,dtype=float)
    base = provider.provider if type(provider) is FourthFactorBlocks else provider
    if type(base) is not SurfaceFactorBlocks:
        raise TypeError('cell projection requires the known linear surface provider')
    if Q.ndim!=2 or Q.shape[0]!=base.modes or Q.shape[1]<1 or not np.isfinite(Q).all():
        raise ValueError('finite matching projection required')
    view = copy(base)
    view.shapes = base.shapes@Q
    view.shapes.flags.writeable = False
    view.modes = Q.shape[1]
    # Constructor checks and folded geometry remain those of the FULL shapes.
    # Never symmetrize/rebuild directions or modify surface/cache identity.
    return FourthFactorBlocks(view) if type(provider) is FourthFactorBlocks else view


def observation_factor_blocks(provider,Q,method,probes,stats):
    if method not in ('factor','cells'):
        raise ValueError('factor projection must be factor or cells')
    count = (len(provider.directions)+provider.block-1)//provider.block
    if any(not isinstance(i,int) or not 0<=i<count for i in probes):
        raise ValueError('projection probe block out of range')
    stats.update(method=method,projected_shapes_bytes=0,projection_preparation_s=0.,
                 full_factor_generation_s=0.,projection_s=0.,probe_results=[],blocks=0)
    t0=time.perf_counter()
    view=projected_cell_provider(provider,Q) if method=='cells' else None
    if view is not None:stats['projected_shapes_bytes']=view.shapes.nbytes
    stats['projection_preparation_s']=time.perf_counter()-t0
    full=iter(provider.blocks());small=iter(view.blocks()) if view is not None else None
    for block in range(count):
        t0=time.perf_counter();H=next(full)
        stats['full_factor_generation_s']+=time.perf_counter()-t0
        t0=time.perf_counter()
        hq=(H.reshape(-1,provider.modes)@Q).reshape(*H.shape[:2],Q.shape[1]) if small is None else next(small)
        projection_seconds=time.perf_counter()-t0
        stats['projection_s']+=projection_seconds
        if hq.shape!=(*H.shape[:2],Q.shape[1]):raise ValueError('projected factor layout changed')
        if block in probes:
            t0=time.perf_counter()
            oracle=(H.reshape(-1,provider.modes)@Q).reshape(hq.shape)
            seconds=time.perf_counter()-t0
            error=float(np.linalg.norm(hq-oracle)/max(np.linalg.norm(oracle),1e-300))
            stats['probe_results'].append(dict(block=block,relative=error,direct_projection_s=seconds,
                                              selected_projection_s=projection_seconds))
            if error>=1e-10:raise ValueError('cell-projected factor differs from full-factor oracle')
        stats['blocks']+=1
        yield H,hq
    # Detect truncation/extra channels instead of silently accepting zip truncation.
    for iterator in [full,small]:
        if iterator is not None and next(iterator,None) is not None:
            raise ValueError('factor block count changed')
