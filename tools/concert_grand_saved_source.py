"""Validate full, unscaled spectral vectors before reusing their directions."""
import numpy as np


def saved_source(report, label, frequency):
    binding = report['source_vector_binding']
    if (binding['coordinates'] != 606 or binding['normalized'] is not False
            or binding['board_sha256'] != report['board_sha256']
            or binding['absolute_time_window_s'] != [0., .003]):
        raise ValueError('matched full board basis, unscaled contact window required')
    rows = report['source_vectors'][label]
    if [r['frequency_hz'] for r in rows] != [6000., 8000., 10000., 12000., 16000., 20000., 24000.]:
        raise ValueError('complete ordered source probes required')
    selected = [r for r in rows if r['frequency_hz'] == frequency]
    if len(selected) != 1:
        raise ValueError('one measured source per requested frequency required')
    pairs = np.asarray(selected[0]['transform'], dtype=float)
    if pairs.shape != (606, 2) or not np.isfinite(pairs).all():
        raise ValueError('606 finite complex source coordinates required')
    source = pairs[:, 0]+1j*pairs[:, 1]
    if np.linalg.norm(source) <= 0:
        raise ValueError('nonzero source direction required')
    return source
