"""Compare short releases/repetitions with immutable native renderers.

Keep the MIDI preroll, pedal and common output gain. Spectral differences are
diagnostics, not proof that a listener's reported click has disappeared.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def render(binary, events, frames, folder):
    folder.mkdir(parents=True, exist_ok=True)
    event_file = folder/'events.txt'
    event_file.write_text(events, encoding='utf-8')
    signature = dict(binary_sha256=sha(binary), events_sha256=sha(event_file), frames=frames,
                     rate=44100, preset='concert', tuning={}, params={})
    manifest, raw = folder/'manifest.json', folder/'performance.f32'
    if not (manifest.exists() and json.loads(manifest.read_text()) == signature
            and raw.exists() and raw.stat().st_size == frames*8):
        env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str(event_file.resolve()), CG_TOTAL_FRAMES=str(frames),
                   CG_RENDER_DIR=str(folder.resolve()), CG_PRESET='concert', CG_RATE='44100',
                   RUST_MIN_STACK='33554432')
        with (folder/'render.log').open('w', encoding='utf-8') as log:
            subprocess.run([str(binary.resolve()), 'tests::render_reference_wavs', '--exact',
                            '--ignored', '--nocapture', '--test-threads=1'], env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        manifest.write_text(json.dumps(signature, indent=2), encoding='utf-8')
    return raw


def load(path):
    return np.memmap(path, dtype='<f4', mode='r').reshape(-1, 2)


def write(path, samples):
    assert np.isfinite(samples).all() and abs(samples).max() < 1
    sf.write(path, samples, 44100, subtype='PCM_24')


def compare(a, b):
    bands = {}
    for lo, hi in [(20, 250), (250, 1000), (1000, 4000), (4000, 8000), (8000, 16000)]:
        sos = butter(4, [lo, hi], fs=44100, btype='bandpass', output='sos')
        powers = [np.mean(sosfilt(sos, x, axis=0)**2) for x in (a, b)]
        bands[f'{lo}-{hi}'] = float(10*np.log10(max(powers[1], 1e-30)/max(powers[0], 1e-30)))
    return dict(band_power_b_minus_a_db=bands,
                peak_a=float(abs(a).max()), peak_b=float(abs(b).max()),
                difference_rms_relative_a_db=float(10*np.log10(
                    max(np.mean((b-a)**2), 1e-30)/max(np.mean(a*a), 1e-30))))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ['baseline-binary', 'candidate-binary', 'baseline-audio', 'events', 'work', 'output']:
        p.add_argument('--'+name, type=Path, required=True)
    p.add_argument('--clip-start', type=float, default=106)
    p.add_argument('--clip-end', type=float, default=115)
    a = p.parse_args()
    a.output.mkdir(parents=True, exist_ok=True)
    assert 0 <= a.clip_start < a.clip_end
    frames = round((a.clip_end+1)*44100)
    events = [list(map(int,l.split())) for l in a.events.read_text().splitlines()]
    prefix = '\n'.join(' '.join(map(str,e)) for e in events if e[0] < frames)+'\n'
    candidate = render(a.candidate_binary, prefix, frames, a.work/'candidate-midi')
    start, end = round(a.clip_start*44100), round(a.clip_end*44100)
    clips = [load(path)[start:end].astype(np.float64) for path in (a.baseline_audio, candidate)]
    assert all(len(x) == end-start for x in clips)
    for name,x in zip(['Campanella-A-actual.wav', 'Campanella-B-misma-cuerda.wav'], clips):
        write(a.output/name, x)
    write(a.output/'Campanella-A-B.wav', np.concatenate([clips[0], np.zeros((22050,2)), clips[1]]))

    # Each group repeats one short unpedalled note three times; the earlier
    # groups' soundboard/room tails remain, just as in an actual performance.
    probe=[]
    notes=[60,71,78,83,85,90,97]
    for j,note in enumerate(notes):
        for i in range(3):
            at=round((j*2+.3+i*.09)*44100)
            probe.extend([(at,144,note,70),(at+round(.028*44100),128,note,64)])
    text='\n'.join(' '.join(map(str,e)) for e in sorted(probe))+'\n'
    isolated=[]; paths=[]
    for label,binary in [('A',a.baseline_binary),('B',a.candidate_binary)]:
        path=render(binary,text,len(notes)*2*44100,a.work/('notes-'+label))
        x=load(path).astype(np.float64);isolated.append(x);paths.append(path)
        write(a.output/f'Notas-cortas-{label}.wav',x)
    first_end=round((.3+.028)*44100)
    assert np.array_equal(isolated[0][:first_end],isolated[1][:first_end]), 'Fresh attack changed'
    report=dict(baseline_binary_sha256=sha(a.baseline_binary),
                candidate_binary_sha256=sha(a.candidate_binary),
                events_sha256=sha(a.events), baseline_audio_sha256=sha(a.baseline_audio),
                candidate_audio_sha256=sha(candidate), clip_seconds=[a.clip_start,a.clip_end],
                gain_db=0, sample_rate=44100, musical_passage=compare(*clips),
                probe_notes=notes, probe_velocity=70, probe_duration_ms=28,
                probe_repeat_ms=90, probe_audio_sha256=[sha(x) for x in paths],
                probe_comparison=compare(*isolated), fresh_attack_identical=True,
                policy='Full MIDI preroll, unchanged pedals, common unity export gain; B changes released-string reuse, no EQ or noise reduction. Audibility awaits listening.')
    (a.output/'mediciones.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report,indent=2),flush=True)


if __name__ == '__main__':
    main()
