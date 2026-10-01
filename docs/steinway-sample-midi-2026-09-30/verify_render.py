import hashlib
import json
import re
import shutil
from pathlib import Path

import mido
import numpy as np
import soundfile as sf

base = Path('C:/SHARED/SteinwayD-MIDI')
work = Path('local/steinway-midi/render-stage')
docs = Path('docs/steinway-sample-midi-2026-09-30')

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

group = {}
regions = []
for line in (work/'SteinwayD-StageORTF.sfz').read_text().splitlines():
    fields = dict(re.findall(r'(\w+)=([^\s]+)', line))
    if line.startswith('<group>'):
        group = fields
    elif line.startswith('<region>'):
        regions.append(group | fields)
assert len(regions) == 658
main = [r for r in regions if r.get('trigger') == 'attack']
release = [r for r in regions if r.get('trigger') == 'release']
for note in range(21,109):
    for vel in range(1,128):
        matches = [r for r in main if int(r['lokey']) <= note <= int(r['hikey']) and int(r['lovel']) <= vel <= int(r['hivel'])]
        assert len(matches) == 1, (note, vel, matches)
        assert abs(note-int(matches[0]['pitch_keycenter'])) <= 1
        assert matches[0]['loop_mode'] == ('one_shot' if note >= 90 else 'no_loop')
        matches = [r for r in release if int(r['lokey']) <= note <= int(r['hikey']) and int(r['lovel']) <= vel <= int(r['hivel'])]
        assert len(matches) == (1 if note <= 89 else 0)

pieces = []
for stem, expected_notes, expected_cues in [('chopin-nocturno-op9-2',1498,1),('liszt-la-campanella',4249,160)]:
    wav = base/f'SteinwayD-StageORTF-{stem}.wav'
    info = json.loads(wav.with_suffix('.json').read_text())
    assert digest(wav) == info['wav_sha256']
    assert digest(Path(info['midi'])) == info['midi_sha256']
    notes = [msg for msg in mido.MidiFile(info['midi']) if msg.type == 'note_on' and msg.velocity > 0]
    assert len(notes) == expected_notes
    assert info['pedal_noise_cues'] == expected_cues
    assert info['max_active_voices'] < 512
    peak = 0
    with sf.SoundFile(wav) as reader:
        assert reader.frames == info['total_frames']
        assert reader.samplerate == 44100 and reader.channels == 2 and reader.subtype == 'PCM_24'
        for chunk in reader.blocks(blocksize=441000,dtype='float64'):
            assert np.isfinite(chunk).all()
            peak = max(peak,float(abs(chunk).max()))
        duration = reader.frames / reader.samplerate
    assert abs(peak-info['output_peak']) < 2**-22
    assert peak <= .850001
    exact = sum(msg.note % 3 == 0 for msg in notes)
    pieces.append(dict(file=str(wav),sha256=digest(wav),duration_seconds=duration,notes=expected_notes,
                       untransposed_note_events=exact,transposed_note_events=expected_notes-exact,
                       peak=peak,gain_db=info['gain_db'],max_active_voices=info['max_active_voices'],
                       pedal_noise_cues=expected_cues,message_counts=info['message_counts']))
assert pieces[0]['gain_db'] == pieces[1]['gain_db']
verification = dict(passed=True,mapping=dict(region_count=658,main_regions=len(main),release_regions=len(release),
                    keys=88,velocities=127,checks='One main region per key/velocity; closest root <=1 semitone; all damped keys covered by release; F6 damped, F#6 undamped.'),pieces=pieces)
(base/'verification.json').write_text(json.dumps(verification,indent=2),encoding='utf-8')
source = Path('local/steinway-midi/source-reference')
provenance = dict(library='https://bitklavier.com/the-bitklavier-grand/',
    license='https://creativecommons.org/licenses/by/4.0/',
    engine='https://github.com/sfztools/sfizz/releases/download/1.2.3/sfizz-1.2.3-win64.zip',
    source_reference_base='https://raw.githubusercontent.com/Princeton-CDH/bitKlavier/main/bk_JUCE/bitKlavier/Source/',
    source_reference_sha256={p.name:digest(p) for p in source.glob('*') if p.is_file()},
    engine_zip_sha256=digest(Path('local/steinway-midi/sfizz/sfizz-1.2.3-win64.zip')),
    verification_script_sha256=digest(Path(__file__)))
(base/'provenance.json').write_text(json.dumps(provenance,indent=2),encoding='utf-8')
shutil.copy2(__file__,docs/'verify_render.py')
for path in base.iterdir():
    if path.suffix in ('.json','.sfz'):
        shutil.copy2(path,docs/path.name)
print(json.dumps(verification,indent=2))
