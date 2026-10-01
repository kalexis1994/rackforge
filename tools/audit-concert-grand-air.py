"""Audition travelled-distance air loss against the frozen proximity candidate."""
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
        for block in iter(lambda: stream.read(1048576), b''): digest.update(block)
    return digest.hexdigest()


def bands(before, after, rate=44100):
    powers = []
    for audio in (before, after):
        power = np.zeros(rate // 2 + 1)
        for start in range(0, len(audio) - rate + 1, rate):
            block = audio[start:start + rate].astype('f8') * np.hanning(rate)[:, None]
            power += np.sum(abs(np.fft.rfft(block, axis=0)) ** 2, axis=1)
        powers.append(power)
    frequencies = np.fft.rfftfreq(rate, 1 / rate)
    result = {}
    for low, high in ((20, 120), (120, 250), (250, 1000), (1000, 4000), (4000, 8000), (8000, 16000)):
        mask = (frequencies >= low) & (frequencies < high)
        result[f'{low}-{high}'] = float(10 * np.log10(powers[1][mask].sum() / powers[0][mask].sum()))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('baseline-binary', 'candidate-binary', 'work', 'output', 'proximity-audit'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    prior = json.loads((args.proximity_audit / 'mediciones.json').read_text(encoding='utf-8'))
    hashes = {'A': sha(args.baseline_binary), 'B': sha(args.candidate_binary)}
    assert hashes['A'] == prior['binary_sha256']['B']
    args.work.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    rate = 44100

    def render(binary, name, events, frames, bypass=False):
        folder = args.work / name
        folder.mkdir(exist_ok=True)
        event_path = folder / 'events.txt'
        event_path.write_text(events, encoding='utf-8')
        env = {k: v for k, v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str(event_path.resolve()), CG_TOTAL_FRAMES=str(frames),
                   CG_RENDER_DIR=str(folder.resolve()), CG_PRESET='concert', CG_RATE=str(rate),
                   RUST_MIN_STACK='33554432')
        if bypass:
            tuning = folder / 'tuning.txt'
            tuning.write_text('AIR_PROPAGATION = 0\n', encoding='utf-8')
            env['CG_TUNING'] = str(tuning.resolve())
        print('Rendering ' + name, flush=True)
        with (folder / 'render.log').open('w', encoding='utf-8') as log:
            subprocess.run([str(binary.resolve()), '--exact', 'tests::render_reference_wavs',
                            '--ignored', '--nocapture', '--test-threads=1'], env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
        raw = folder / 'performance.f32'
        assert raw.stat().st_size == frames * 8
        audio = np.memmap(raw, dtype='<f4', mode='r').reshape(-1, 2)
        assert np.isfinite(audio).all() and 0 < float(abs(audio).max()) < 1
        return raw, audio

    def export(name, audio):
        wav = args.output / f'{name}.wav'
        with sf.SoundFile(wav, mode='w', samplerate=rate, channels=2, subtype='PCM_24') as stream:
            for start in range(0, len(audio), rate * 10): stream.write(audio[start:start + rate * 10])
        assert sf.info(wav).frames == len(audio)
        return {'wav': str(wav.resolve()), 'sha256': sha(wav), 'peak': float(abs(audio).max())}

    events = []
    for i, note in enumerate((30, 48, 60, 65, 72, 84)):
        start = (1 + i * 3) * rate
        events += [(start, 144, note, 90), (start + rate, 128, note, 64)]
    text = ''.join(' '.join(map(str, event)) + '\n' for event in sorted(events))
    frames = 20 * rate
    _, before = render(args.baseline_binary, 'notas-A', text, frames)
    _, bypass = render(args.candidate_binary, 'notas-bypass', text, frames, True)
    assert np.array_equal(before, bypass), 'Air bypass changed the previous model'
    _, after = render(args.candidate_binary, 'notas-B', text, frames)
    sequence = [np.zeros((rate, 2), dtype='f4')]
    for i in range(6):
        for audio in (before, after):
            start = (1 + i * 3) * rate
            sequence += [audio[start:start + 2 * rate], np.zeros((rate // 2, 2), dtype='f4')]
    report = {'binary_sha256': hashes, 'bypass_max_difference': float(abs(before - bypass).max()),
              'notes': export('Notas-A-B', np.concatenate(sequence)),
              'notes_band_change_db': bands(before, after), 'pieces': {},
              'environment': {'temperature_c': 20, 'relative_humidity_percent': 50, 'pressure_kpa': 101.325},
              'policy': 'A has the corrected proximity shelf. B adds ISO air propagation and corrects '
                        'stored-duration FDN gain. Same factory profile, velocities, MIDI/pedal, '
                        '44.1 kHz stereo PCM24, 256 frames, 200 ms warmup, 10 s tail. '
                        'No normalization, EQ or export compression.'}

    def piece(name):
        previous = prior['pieces'][name]
        source = root / 'local/concert-grand-proximity/audition' / f'{name}-B'
        raw_a = source / 'performance.f32'
        events = source / 'events.txt'
        assert sha(raw_a) == previous['raw_after_sha256']
        assert sha(events) == previous['events_sha256']
        before = np.memmap(raw_a, dtype='<f4', mode='r').reshape(-1, 2)
        raw_b, after = render(args.candidate_binary, f'{name}-B', events.read_text(encoding='utf-8'), previous['frames'])
        source_wav = Path(previous['after']['wav'])
        assert sha(source_wav) == previous['after']['wav_sha256']
        wav_a = args.output / f'{name}-A-sin-absorcion-fisica.wav'
        shutil.copyfile(source_wav, wav_a)
        result = {'before': str(wav_a.resolve()), 'after': export(f'{name}-B-aire-fisico', after),
                  'band_change_db': bands(before, after), 'raw_after_sha256': sha(raw_b),
                  'midi_sha256': previous['midi_sha256'], 'events_sha256': previous['events_sha256'],
                  'frames': previous['frames'], 'note_attacks': previous['note_attacks']}
        print(f'Finished {name}: {result["band_change_db"]}', flush=True)
        return name, result

    with ThreadPoolExecutor(max_workers=2) as pool:
        for name, result in pool.map(piece, ('Nocturne', 'Campanella')):
            report['pieces'][name] = result
    (args.output / 'mediciones.json').write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding='utf-8')
    (args.output / 'LEEME.txt').write_text(
        'A = candidato con proximidad corregida. B = el mismo, con propagación por aire ISO.\n'
        'Notas-A-B: A y después B para F#2, C3, C4, F4, C5 y C6 a velocidad 90.\n'
        'Piezas completas con los mismos pedales; sin normalización ni procesamiento añadido.\n'
        '20 °C, humedad 50%, presión 101.325 kPa. El rfplugin publicado no se reemplazó.\n', encoding='utf-8')


if __name__ == '__main__': main()
