"""Finalize controls from a complete saved local-Hann measurement checkpoint."""
import math


def maximum_nested(value):
    if isinstance(value, dict):
        return max(maximum_nested(v) for v in value.values())
    if isinstance(value, (list, tuple)):
        return max(maximum_nested(v) for v in value)
    number = float(value)
    if not math.isfinite(number):
        raise ValueError('finite control required')
    return number


def local_controls(report, unchanged):
    c = report['criteria']
    probes = report['load_probes']
    if ([p['frequency_hz'] for p in probes] != [6000., 8000., 10000., 12000., 16000., 20000., 24000.]
            or report['rate'] != 3072000 or report['duration_s'] != .003
            or len(report['canonical_endpoint_exact']) != 4):
        raise ValueError('complete canonical measurement required')
    return dict(
        contact_reproduction=maximum_nested(report['contact_reproduction_relative']) < c['reproduction_relative'],
        endpoint_exact=all(report['canonical_endpoint_exact']),
        fine_local_oracle=maximum_nested(report['local_oracle_relative']) < c['analytic_oracle_relative'],
        local_order=maximum_nested(report['local_order8_12_relative']) < c['analytic_oracle_relative'],
        local_blocks=maximum_nested(report['local_block_relative']) < c['analytic_oracle_relative'],
        load_reproduction=max(maximum_nested(p['parent_reproduction_relative']) for p in probes) < c['reproduction_relative'],
        finest_load_quadrature=max(p['load_finest_quadrature_relative'] for p in probes) < c['load_cell_quadrature_relative'],
        fine_load_cells=max(p['load_fine_refinement_relative'] for p in probes) < c['load_cell_refinement_relative'],
        source_and_cache_unchanged=bool(unchanged))
