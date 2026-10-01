"""Render both preserved MIDI performances with the frozen contact audition."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess

import numpy as np
import soundfile as sf


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('candidate', 'binary', 'baseline', 'events', 'work', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--gain-db', type=float)
    args = parser.parse_args()
    candidate = json.loads(args.candidate.read_text(encoding='utf-8'))
    model = candidate['model']
    binary_hash = sha(args.binary)
    assert binary_hash == model['binary_sha256'], 'Wrong frozen renderer'
    tuning = {**model['tuning'], **candidate['overrides']}
    args.work.mkdir(parents=True, exist_ok=True)
    args.output.mkdir(parents=True, exist_ok=True)
    stems = ('chopin-nocturno-op9-2', 'liszt-la-campanella')
    names = ('Fieltro-experimental-Nocturno-op9-2', 'Fieltro-experimental-La-Campanella')

    def render(stem):
        previous = json.loads((args.baseline / f'Modelo-14B-{stem}.json').read_text(encoding='utf-8'))
        midi = Path(previous['midi'])
        source_events = args.events / stem / 'events.txt'
        assert sha(midi) == previous['midi_sha256']
        assert sha(source_events) == previous['events_sha256']
        folder = args.work / stem
        folder.mkdir(exist_ok=True)
        events = folder / 'events.txt'
        events.write_bytes(source_events.read_bytes())
        config = folder / 'tuning.txt'
        config.write_text('\n'.join(f'{key} = {value}' for key, value in sorted(tuning.items())) + '\n', encoding='utf-8')
        frames = previous['total_frames']
        signature = dict(binary_sha256=binary_hash, candidate_sha256=sha(args.candidate),
                         events_sha256=sha(events), tuning=tuning, params=model['params'], total_frames=frames)
        raw = folder / 'performance.f32'
        marker = folder / 'render-manifest.json'
        log = folder / 'render.log'
        cached = marker.exists() and json.loads(marker.read_text()) == signature
        if not (cached and raw.exists() and raw.stat().st_size == frames * 8):
            env = {key: value for key, value in os.environ.items() if not key.startswith('CG_')}
            env.update(CG_EVENT_FILE=str(events.resolve()), CG_TOTAL_FRAMES=str(frames),
                       CG_RENDER_DIR=str(folder.resolve()), CG_TUNING=str(config.resolve()),
                       CG_PRESET='concert', CG_PARAMS=','.join(f'{key}={value}' for key, value in model['params'].items()),
                       CG_RATE='44100', RUST_MIN_STACK='33554432')
            print('Rendering ' + stem, flush=True)
            with log.open('w', encoding='utf-8') as stream:
                subprocess.run([str(args.binary.resolve()), '--exact', 'tests::render_reference_wavs',
                                '--ignored', '--nocapture', '--test-threads=1'], env=env,
                               stdout=stream, stderr=subprocess.STDOUT, check=True)
            assert 'complaints: []' in log.read_text(encoding='utf-8')
            marker.write_text(json.dumps(signature, indent=2), encoding='utf-8')
        assert raw.stat().st_size == frames * 8
        samples = np.memmap(raw, dtype='<f4', mode='r').reshape(-1, 2)
        assert np.isfinite(samples).all()
        peak = float(np.abs(samples).max())
        assert peak > 0
        assert sha(raw) != previous['raw_sha256'], 'Rendered old model instead of contact candidate'
        print(f'Finished {stem}: {frames / 44100:.2f}s, raw peak {peak:.6f}', flush=True)
        return previous, raw, peak

    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(render, stems))
    # Keep the gain of the previous full-piece exports; lower both together if needed.
    previous_gain_db = results[0][0]['gain_db']
    assert all(item[0]['gain_db'] == previous_gain_db for item in results)
    base_gain_db = previous_gain_db if args.gain_db is None else args.gain_db
    maximum = max(item[2] for item in results) * 10 ** (base_gain_db / 20)
    trim_db = 20 * math.log10(min(1, .85 / maximum))
    final_gain_db = base_gain_db + trim_db
    gain = 10 ** (final_gain_db / 20)
    for name, (previous, raw, peak) in zip(names, results):
        wav = args.output / (name + '.wav')
        samples = np.memmap(raw, dtype='<f4', mode='r').reshape(-1, 2)
        with sf.SoundFile(wav, mode='w', samplerate=44100, channels=2, subtype='PCM_24') as stream:
            for start in range(0, len(samples), 44100 * 10):
                stream.write(samples[start:start + 44100 * 10].astype(np.float64) * gain)
        info = sf.info(wav)
        assert info.frames == previous['total_frames'] and info.samplerate == 44100
        assert info.channels == 2 and info.subtype == 'PCM_24'
        output_peak = 0.
        with sf.SoundFile(wav) as stream:
            for block in stream.blocks(blocksize=441000, always_2d=True):
                assert np.isfinite(block).all()
                output_peak = max(output_peak, float(np.abs(block).max()))
        assert 0 < output_peak <= .850001
        metadata = {key: previous[key] for key in ('midi', 'midi_sha256', 'midi_seconds', 'total_frames',
                    'channel_messages', 'note_attacks', 'velocity_range', 'controller_values', 'events_sha256')}
        metadata.update(label=candidate['name'], experimental_only=True,
                        candidate=str(args.candidate.resolve()), candidate_sha256=sha(args.candidate),
                        binary_sha256=binary_hash, tuning=tuning, params=model['params'],
                        sample_rate=44100, channels=2,
                        gain_db=final_gain_db, base_gain_db=base_gain_db, additional_common_trim_db=trim_db,
                        raw_peak=peak, output_peak=output_peak, raw_sha256=sha(raw),
                        wav=str(wav.resolve()), wav_sha256=sha(wav),
                        policy='Candidate configuration preserved. All channel events and pedal values preserved; '
                               '256-frame blocks; 200 ms silent warmup; 10 s tail. One common gain for both pieces; '
                               'no added EQ, compression or fades.')
        wav.with_suffix('.json').write_text(json.dumps(metadata, indent=2, ensure_ascii=False), encoding='utf-8')
        print(f'Exported {wav}: peak {output_peak:.6f}, gain {final_gain_db:.6f} dB', flush=True)
    (args.output / 'LEEME.txt').write_text(
        f'Nocturno Op.9 No.2 y La Campanella: {candidate["name"]}.\n'
        'Configuración exacta y hashes en los JSON junto a los WAV.\n'
        'Se conservan las velocidades, tiempos y mensajes de pedal de ambos MIDI.\n'
        'Estéreo, 44,1 kHz, PCM de 24 bits, cola de 10 segundos.\n'
        f'Ganancia común: {final_gain_db:.6f} dB. Sin EQ ni compresión añadidas.\n', encoding='utf-8')


if __name__ == '__main__':
    main()
