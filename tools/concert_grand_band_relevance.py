"""Absolute error contributions derived from unchanged trapezoid norms."""
import math


def band_error_contributions(norms, relative):
    bands = ['0-6000', '6000-12000', '12000-24000']
    total = '0-24000'
    if set(norms) != set(bands+[total]) or set(relative) != set(norms):
        raise ValueError('matched partition and total required')
    if not all(math.isfinite(v) and v >= 0 for v in [*norms.values(), *relative.values()]):
        raise ValueError('finite nonnegative norms and errors required')
    if norms[total] <= 0 or not math.isclose(sum(norms[b] for b in bands), norms[total], rel_tol=2e-12):
        raise ValueError('positive additive total required')
    rows = {b:dict(reference_squared_norm=norms[b],
                  error_squared_norm=relative[b]**2*norms[b],
                  band_relative_error=relative[b],
                  error_relative_to_total=relative[b]*math.sqrt(norms[b]/norms[total]))
            for b in bands}
    recovered = math.sqrt(sum(row['error_squared_norm'] for row in rows.values())/norms[total])
    if not math.isclose(recovered, relative[total], rel_tol=2e-10, abs_tol=1e-16):
        raise ValueError('band error partition does not reproduce stored total')
    return dict(bands=rows, reconstructed_total_relative_error=recovered,
                stored_total_relative_error=relative[total])
