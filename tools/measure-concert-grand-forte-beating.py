"""Compare resolved partial-envelope notches without folding down the audio first."""
import argparse
import json
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfiltfilt, resample_poly, find_peaks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--audit', type=Path, required=True)
    parser.add_argument('--renders', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    audit = json.loads(args.audit.read_text())
    rate = 44100
    profiles = audit['overrides']
    pairs = [tuple(pair) for pair in audit['pairs']]
    reference = sf.read(args.audit.parent / 'Steinway-sampler-intensidades.wav', always_2d=True)[0]
    # This audit export contains four seconds per note plus .25s silence.
    chunk = round(4.25 * rate)
    gain = 10 ** ((audit['gain_db'] + audit['common_trim_db']) / 20)
    signals = {label: {} for label in (*profiles, 'Steinway-sampler')}
    for i, pair in enumerate(pairs):
        signals['Steinway-sampler'][pair] = reference[i*chunk:i*chunk+4*rate]
        for label in profiles:
            raw = args.renders / label / f'{pair[0]}-{pair[1]}' / 'performance.f32'
            signals[label][pair] = np.fromfile(raw, dtype='<f4').reshape(-1, 2)[:4*rate].astype(float) * gain
    report = dict(policy=__doc__ + ' Three-dB recovered notches on a common baseline harmonic mask; '
                       'individual carrier search per instrument. Diagnostic counts, not a fidelity score.',
                  binary_sha256=audit['binary_sha256'], profiles=profiles, rows={}, summary={})
    envelope_rate = rate/220
    lowpass = butter(4, 18, fs=envelope_rate, output='sos')
    for pair in pairs:
        note, velocity = pair
        f0 = 440 * 2 ** ((note-69)/12)
        channels = {}
        for label, notes in signals.items():
            x = notes[pair]
            clip = x[round(.05*rate):round(.3*rate)]
            fft = np.fft.rfft(clip*np.hanning(len(clip))[:,None], n=262144, axis=0)
            spectrum = np.mean(abs(fft)**2, axis=1)
            hz = np.fft.rfftfreq(262144, 1/rate)
            partials = []
            for h in range(1, 17):
                if h*f0 > 10000:
                    break
                indices = np.flatnonzero(abs(hz-h*f0)<.3*f0)
                frequency = float(hz[indices[np.argmax(spectrum[indices])]])
                carrier = np.exp(-2j*np.pi*frequency*np.arange(len(x))/rate)
                demodulated = resample_poly(x*carrier[:,None], 1, 220, axis=0)
                envelope = np.sqrt(np.mean(abs(sosfiltfilt(lowpass, demodulated, axis=0))**2, axis=1))
                db = 20*np.log10(np.maximum(envelope, 1e-12))
                t = np.arange(len(db))/envelope_rate
                initial = float(db[(t>=.1)&(t<.5)].max())
                minima, properties = find_peaks(-db, prominence=3, distance=round(.08*envelope_rate))
                notches = [dict(time_s=float(t[m]), depth_db=float(properties['prominences'][i]))
                           for i,m in enumerate(minima) if .1<t[m]<3.5 and db[m]>initial-35]
                partials.append(dict(harmonic=h, frequency_hz=frequency, initial_db=initial, notches=notches))
            channels[label] = partials
        baseline = channels['Actual']
        ceiling = max(row['initial_db'] for row in baseline)
        mask = [row['initial_db'] > ceiling-30 for row in baseline]
        row = {}
        for label, partials in channels.items():
            chosen = [partial for partial,keep in zip(partials,mask) if keep]
            counts = [sum(lo<=event['time_s']<hi for partial in chosen for event in partial['notches'])
                      for lo,hi in ((.1,.5),(.5,1.5),(1.5,3.5))]
            row[label] = dict(partials=chosen, notch_counts=counts)
        report['rows'][f'{note}:{velocity}'] = row
    for label in signals:
        report['summary'][label] = {
            str(v): [sum(row[label]['notch_counts'][j] for key,row in report['rows'].items()
                         if int(key.split(':')[1])==v) for j in range(3)] for v in (60,124)}
    args.output.write_text(json.dumps(report, indent=2))
    print(json.dumps(report['summary'], indent=2))


if __name__ == '__main__':
    main()
