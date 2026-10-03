"""Temporal attack diagnostics, separate from harmonic balance.

Energy includes both capsules; mono cancellation must not look like silence.
Level-relative features never change the exported audio. These are diagnostic
guards, not a perceptually validated piano-quality score.
"""
import numpy as np

WINDOWS = ((0, .005), (.005, .02), (.02, .05), (.05, .1), (.1, .2))


def db(power):
    return 10*np.log10(np.maximum(power, 1e-30))


def measure(x, note, rate=44100):
    x = np.asarray(x, dtype=np.float64)
    assert x.ndim == 2 and x.shape[1] == 2 and np.isfinite(x).all()
    step = round(rate*.001)
    count = min(len(x), rate)//step*step
    bins = np.mean(x[:count].reshape(-1, step, 2)**2, axis=(1, 2))
    assert bins.max() > 0
    onset = int(np.flatnonzero(bins > bins.max()*1e-4)[0]*step)
    energy = np.mean(x[onset:]**2, axis=1)
    assert len(energy) >= round(.25*rate)
    levels = [float(db(np.mean(energy[round(lo*rate):round(hi*rate)])))
              for lo, hi in WINDOWS]
    # Two fundamental periods reduce phase/period ripple in the bass. The
    # fixed 5ms rise is reported everywhere but only used as a guard when that
    # window contains at least two cycles. This is measurement resolution,
    # not a note-dependent DSP adjustment.
    f0 = 440*2**((note-69)/12)
    width = round(rate*max(.005, 2/f0))
    stop = round(.2*rate)
    prefix = np.r_[0., np.cumsum(energy[:stop+width])]
    envelope = (prefix[width:]-prefix[:-width])/width
    stride = round(rate*.005)
    indices = np.arange(0, stop-width+1, stride)
    centers = (indices+width/2)/rate
    power = envelope[indices]
    sustain = np.mean(energy[round(.05*rate):round(.1*rate)])
    peak = int(np.argmax(envelope[:stop-width+1]))
    return dict(onset_ms=1000*onset/rate, windows_s=WINDOWS, levels_db=levels,
                rise_5_20_db=levels[1]-levels[0], rise_resolved=f0*.005 >= 2,
                smoothing_ms=1000*width/rate, envelope_times_ms=(1000*centers).tolist(),
                envelope_relative_db=(db(power)-db(sustain)).tolist(),
                envelope_db=db(power).tolist(),
                peak_time_ms=1000*(peak+width/2)/rate,
                peak_level_db=float(db(envelope[peak])))


def compare(target, prediction):
    assert target['envelope_times_ms'] == prediction['envelope_times_ms']
    t = np.asarray(target['envelope_relative_db'])
    p = np.asarray(prediction['envelope_relative_db'])
    absolute = np.asarray(target['envelope_db'])
    mask = absolute > absolute.max()-40
    values = dict(envelope_shape_error_db=float(np.mean(abs(p[mask]-t[mask]))),
                  attack_level_error_db=float(np.mean(abs(np.asarray(prediction['levels_db'])[:3]
                                                         -np.asarray(target['levels_db'])[:3]))),
                  peak_time_error_ms=abs(prediction['peak_time_ms']-target['peak_time_ms']),
                  peak_level_error_db=abs(prediction['peak_level_db']-target['peak_level_db']))
    values['rise_error_db'] = (abs(prediction['rise_5_20_db']-target['rise_5_20_db'])
                               if target['rise_resolved'] else None)
    return values


def guards(rows, baseline):
    assert rows.keys() == baseline.keys() and rows
    mean = lambda data, key: float(np.mean([v[key] for v in data.values() if v[key] is not None]))
    shape = np.array([v['envelope_shape_error_db'] for v in rows.values()])
    before = np.array([v['envelope_shape_error_db'] for v in baseline.values()])
    # FFT-convolved identity can differ by roundoff. This tolerance is many
    # orders below the declared engineering guards, not a perceptual allowance.
    epsilon=1e-9
    checks = dict(mean_shape_not_worse=shape.mean() <= before.mean()+epsilon,
                  p95_shape_within_half_db=np.percentile(shape, 95) <= np.percentile(before, 95)+.5+epsilon)
    resolved = any(v['rise_error_db'] is not None for v in rows.values())
    if resolved:
        checks['mean_resolved_rise_not_worse'] = mean(rows, 'rise_error_db') <= mean(baseline, 'rise_error_db')+epsilon
    return dict(passed=all(checks.values()), checks={k:bool(v) for k,v in checks.items()},
                actual_mean_shape_db=float(before.mean()), identified_mean_shape_db=float(shape.mean()),
                actual_mean_resolved_rise_db=mean(baseline, 'rise_error_db') if resolved else None,
                identified_mean_resolved_rise_db=mean(rows, 'rise_error_db') if resolved else None,
                policy='Do not regress mean temporal shape or resolved rise; p95 shape allowance 0.5dB. '
                       'Engineering guard, not a perceptual threshold. Listening can reject a passing candidate.')
