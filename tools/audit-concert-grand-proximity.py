"""Compare a frozen pre-fix renderer with the corrected proximity shelf."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

import numpy as np
import soundfile as sf


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline-binary', 'candidate-binary', 'work', 'output', 'baseline-midi-dir'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    rate = 44100
    args.work.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    binaries = {'A': args.baseline_binary, 'B': args.candidate_binary}
    hashes = {label: sha(binary) for label, binary in binaries.items()}
    assert hashes['A'] != hashes['B']

    def render(label, name, events, frames, params=''):
        folder = args.work / f'{name}-{label}'
        folder.mkdir(exist_ok=True)
        event_path = folder / 'events.txt'
        event_path.write_text(events, encoding='utf-8')
        env = {k: v for k, v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str(event_path.resolve()), CG_TOTAL_FRAMES=str(frames),
                   CG_RENDER_DIR=str(folder.resolve()), CG_PRESET='concert', CG_RATE=str(rate),
                   CG_PARAMS=params, RUST_MIN_STACK='33554432')
        print(f'Rendering {name}-{label}', flush=True)
        with (folder / 'render.log').open('w', encoding='utf-8') as log:
            subprocess.run([str(binaries[label].resolve()), '--exact', 'tests::render_reference_wavs',
                            '--ignored', '--nocapture', '--test-threads=1'], env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        path = folder / 'performance.f32'
        assert path.stat().st_size == frames * 8
        audio = np.memmap(path, dtype='<f4', mode='r').reshape(-1, 2)
        assert np.isfinite(audio).all()
        return path, audio

    def export(path, audio):
        assert np.isfinite(audio).all() and float(abs(audio).max()) < 1
        with sf.SoundFile(path, mode='w', samplerate=rate, channels=2, subtype='PCM_24') as stream:
            for start in range(0, len(audio), rate * 10):
                stream.write(audio[start:start + rate * 10])
        info = sf.info(path)
        assert info.frames == len(audio) and info.subtype == 'PCM_24'
        return {'wav': str(path.resolve()), 'wav_sha256': sha(path),
                'frames': len(audio), 'peak': float(abs(audio).max())}

    def measure(a, b):
        result = {'rms_change_db': float(10 * np.log10(np.sum(b.astype('f8') ** 2)
                                                       / np.sum(a.astype('f8') ** 2))),
                  'max_difference': float(abs(b - a).max()), 'bands_db': {}}
        # Average independent 1 s Hann spectra from both capsules.
        power = []
        for audio in (a, b):
            spectrum = np.zeros(rate // 2 + 1)
            for start in range(0, len(audio) - rate + 1, rate):
                segment = audio[start:start + rate].astype('f8') * np.hanning(rate)[:, None]
                spectrum += np.sum(abs(np.fft.rfft(segment, axis=0)) ** 2, axis=1)
            power.append(spectrum)
        frequencies = np.fft.rfftfreq(rate, 1 / rate)
        for low, high in ((20, 120), (120, 250), (250, 1000), (1000, 4000), (4000, 12000)):
            band = (frequencies >= low) & (frequencies < high)
            result['bands_db'][f'{low}-{high}'] = float(10 * np.log10(power[1][band].sum()
                                                                    / power[0][band].sum()))
        return result

    note_events = []
    for i, (note, velocity) in enumerate((n, v) for n in (30, 48, 60, 65, 72, 84) for v in (60, 110)):
        start = (i * 3 + 1) * rate
        note_events += [(start, 144, note, velocity), (start + rate, 128, note, 64)]
    note_frames = 38 * rate
    text = ''.join(' '.join(map(str, event)) + '\n' for event in sorted(note_events))
    notes = {}
    for label in binaries:
        _, notes[label] = render(label, 'notas', text, note_frames)
        export(args.output / f'Notas-{label}.wav', notes[label])
    gap = np.zeros((rate // 2, 2), dtype='f4')
    combined = [np.zeros((rate, 2), dtype='f4')]
    for i in range(12):
        for label in ('A', 'B'):
            start = (i * 3 + 1) * rate
            combined.extend((notes[label][start:start + 2 * rate], gap))
    export(args.output / 'Notas-A-B.wav', np.concatenate(combined))
    report = {'binary_sha256': hashes, 'notes': measure(notes['A'], notes['B']),
              'pieces': {}, 'policy': 'A: released 0.172.0. B: proximity coefficient restored. '
              'Factory Concert Grand, stereo 44.1 kHz/24 bit, original velocities and pedal. '
              'Identical gain; no export normalization, EQ, fades or compression.'}
    # Render an omni control through both complete pipelines. If there is
    # another sound change hiding in the fix, this will no longer be exact.
    omni = [render(label, 'omni-control', text, note_frames, '26=0')[1] for label in ('A', 'B')]
    assert np.array_equal(*omni), 'The correction changed something besides the proximity shelf'
    report['omni_control_max_difference'] = float(abs(omni[1] - omni[0]).max())

    def piece(item):
        stem, name, previous_name = item
        previous = json.loads((args.baseline_midi_dir / f'{previous_name}.json').read_text(encoding='utf-8'))
        assert previous['binary_sha256'] == hashes['A']
        previous_wav = args.baseline_midi_dir / f'{previous_name}.wav'
        assert sha(previous_wav) == previous['wav_sha256']
        midi = Path(previous['midi'])
        source = root / 'local/concert-grand-coherent-unison/attack14b-midi' / stem / 'events.txt'
        assert sha(midi) == previous['midi_sha256'] and sha(source) == previous['events_sha256']
        frames = previous['total_frames']
        raw_a = root / 'local/concert-grand-restrike-contact/release-midi' / stem / 'performance.f32'
        assert sha(raw_a) == previous['raw_sha256']
        a = np.memmap(raw_a, dtype='<f4', mode='r').reshape(-1, 2)
        raw_b, b = render('B', name, source.read_text(encoding='utf-8'), frames)
        path_a = args.output / f'{name}-A-actual.wav'
        shutil.copyfile(previous_wav, path_a)
        output_b = export(args.output / f'{name}-B-proximidad.wav', b)
        result = {'before': str(path_a.resolve()), 'after': output_b, 'measurement': measure(a, b),
                  'midi_sha256': previous['midi_sha256'], 'events_sha256': previous['events_sha256'],
                  'raw_before_sha256': sha(raw_a), 'raw_after_sha256': sha(raw_b),
                  'note_attacks': previous['note_attacks'], 'frames': frames}
        print(f'Finished {name}: {result["measurement"]}', flush=True)
        return name, result

    with ThreadPoolExecutor(max_workers=2) as pool:
        for name, result in pool.map(piece, [
            ('chopin-nocturno-op9-2', 'Nocturne', 'Nocturne-op9-2-RF-Concert-Grand-0.172.0'),
            ('liszt-la-campanella', 'Campanella', 'La-Campanella-RF-Concert-Grand-0.172.0')]):
            report['pieces'][name] = result
    (args.output / 'mediciones.json').write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8')
    (args.output / 'LEEME.txt').write_text(
        'A = plugin publicado 0.172.0, proximidad inactiva. B = corrección experimental.\n'
        'Notas-A-B.wav: por cada nota/velocidad se oye A y luego B.\n'
        'Orden: F#2, C3, C4, F4, C5, C6; cada nota a velocidades 60 y 110.\n'
        'Nocturne/Campanella: mismas piezas y pedales, mismo nivel, sin normalizar.\n'
        'El plugin en C:/SHARED permanece sin reemplazar. No se hizo commit.\n', encoding='utf-8')
    print(json.dumps(report['notes'], indent=2), flush=True)


if __name__ == '__main__':
    main()
