"""Finite-window Hann profile: local V, established analytic A.

Both integrate the same actual C1 Hermite trajectory/window. Keep global
analytic A, whose original control passed; local A remains a failed recorded
experiment at the coarse rate. This changes no physical source or threshold.
"""
import numpy as np
from concert_grand_local_hann import local_hann_spectra
from concert_grand_uniform_spectra import uniform_window_spectra


def hann_observation_spectra(time, position, velocity, frequency):
    v, _ = local_hann_spectra(time, position, velocity, frequency)
    _, a = uniform_window_spectra(time, position, velocity, frequency)['hann']
    return v, a


def profile_controls(coarse, fine):
    expected = [0., 60., 6000., 8000., 10000., 12000., 16000., 20000., 24000.]
    if ([coarse['rate'], fine['rate']] != [1536000, 3072000]
            or any(r['duration_s'] != .003 or r['oracle_frequency_hz'] != expected for r in [coarse, fine])
            or coarse['criteria'] != fine['criteria']):
        raise ValueError('matched two-rate window, frequencies and criteria required')
    threshold = coarse['criteria']['analytic_oracle_relative']
    if threshold != 1e-10:
        raise ValueError('unchanged original oracle threshold required')
    rows = []
    for r in [coarse, fine]:
        v = np.asarray(r['local_oracle_relative']['velocity'], float)
        a = np.asarray(r['original_global_oracle_relative']['acceleration'], float)
        local_a = np.asarray(r['local_oracle_relative']['acceleration'], float)
        if any(x.shape != (9,) or not np.isfinite(x).all() or (x < 0).any() for x in [v, a, local_a]):
            raise ValueError('nine finite nonnegative measured errors required')
        rows.append(dict(rate=r['rate'], velocity_local_maximum=float(v.max()),
            acceleration_original_maximum=float(a.max()),
            velocity_local_passed=bool(v.max() < threshold),
            acceleration_original_passed=bool(a.max() < threshold),
            experimental_acceleration_local_maximum=float(local_a.max()),
            experimental_acceleration_local_passed=bool(local_a.max() < threshold)))
    return rows
