"""Composite Simpson from authenticated nested trapezoids on uniform bands.

S_h=(4*T_h-T_2h)/3 is the exact composite Simpson weight identity, not a
fit to source values. Use it only where the saved frequency grid is uniform.
Below 6 kHz the diagnostic contains extra endpoints and keeps trapezoids.
"""
import numpy as np


def refined_band_quadrature(frequency,steps,integrals):
    f = np.asarray(frequency,dtype=float)
    steps = np.asarray(steps,dtype=float)
    if (f.ndim!=1 or len(f)<3 or not np.isfinite(f).all() or (np.diff(f)<=0).any()
            or steps.shape!=(3,) or not np.isfinite(steps).all() or (steps<=0).any()
            or not np.array_equal(steps[:-1],2*steps[1:])):
        raise ValueError('three nested halved steps and finite ordered grid required')
    high = (f>=6000)&(f<=24000)
    if not np.array_equal(f[high],np.arange(6000.,24000.+steps[-1]/2,steps[-1])):
        raise ValueError('uniform saved 6-24 kHz grid required')
    rows = [integrals[str(float(step))] for step in steps]
    kinds = set(rows[0])
    if any(set(row)!=kinds for row in rows) or not kinds:
        raise ValueError('matching observed quantities required')
    bands = {'0-6000','6000-12000','12000-24000','0-24000'}
    for row in rows:
        for values in row.values():
            if (set(values)!=bands or not np.isfinite(list(values.values())).all()
                    or min(values.values())<0):
                raise ValueError('matching finite nonnegative source bands required')
    results = []
    for coarse,fine in zip(rows[:-1],rows[1:]):
        item = {}
        for kind in sorted(kinds):
            # Uneven extra points below 6 kHz prohibit this identity there.
            values = {'0-6000':fine[kind]['0-6000']}
            for band in ['6000-12000','12000-24000']:
                values[band] = (4*fine[kind][band]-coarse[kind][band])/3
                if values[band]<0:
                    raise ValueError('negative Simpson estimate; no clipping')
            values['0-24000'] = sum(values.values())
            if values['0-24000']<=0:
                raise ValueError('nonzero observed band norm required')
            item[kind] = values
        results.append(item)
    changes = {kind:{band:abs(results[0][kind][band]-results[1][kind][band])/
        max(results[1][kind][band],1e-40) for band in sorted(bands)} for kind in sorted(kinds)}
    return dict(method='trapezoid below 6 kHz, composite Simpson on uniform high bands',
        integrals_by_step_hz={str(float(step)):result for step,result in zip(steps[1:],results)},
        finest_relative=changes,
        above_6k_fraction={kind:(results[-1][kind]['6000-12000']+results[-1][kind]['12000-24000'])/
            results[-1][kind]['0-24000'] for kind in sorted(kinds)})
