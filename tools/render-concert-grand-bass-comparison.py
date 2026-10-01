"""Alternating bass auditions against the frozen Steinway Stage ORTF sampler."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('candidate','binary','work','output'):
        p.add_argument('--'+name,type=Path,required=True)
    p.add_argument('--notes',default='42',help='MIDI notes 41 (F2) and/or 42 (F#2)')
    a = p.parse_args()
    a.work.mkdir(parents=True,exist_ok=True)
    a.output.mkdir(parents=True,exist_ok=True)
    e = load(Path(__file__).with_name('evaluate-concert-grand-full-sound.py'),'bass_eval')
    sampler = load(Path(__file__).with_name('render-bitklavier-steinway-midi.py'),'bass_sampler')
    e.HOLD, e.SECONDS = 4, 6
    velocities = (40,60,84,124)
    names = {41:'F2',42:'Fs2-exacto'}
    selected = tuple(int(n) for n in a.notes.split(','))
    assert selected and len(set(selected)) == len(selected) and all(n in names for n in selected)
    notes = tuple((n,names[n]) for n in selected)
    candidate = json.loads(a.candidate.read_text(encoding='utf-8'))
    model = {**candidate['model'],'tuning':{**candidate['model']['tuning'],**candidate['overrides']}}
    assert e.digest(a.binary) == model['binary_sha256']
    mapping_path = Path('local/steinway-midi/render-stage/mapping.json')
    mapping = json.loads(mapping_path.read_text(encoding='utf-8'))
    sfz = Path('local/steinway-midi/render-stage/SteinwayD-StageORTF.sfz')
    dll = Path('local/steinway-midi/sfizz/library/bin/Release/sfizz.dll')
    synth = sampler.Engine(dll,sfz)
    assert synth.get_num_regions(synth.s) == mapping['region_count']
    # Preserve the fixed gain used for previous sample auditions. A common
    # export trim, if necessary, is applied to BOTH instruments and all levels.
    gain_db = 26.401176937389373
    gain = 10**(gain_db/20)
    sounds = {}
    sources = {}
    try:
        with ThreadPoolExecutor(max_workers=2) as pool:
            jobs = {(n,v):pool.submit(e.render_model,a.binary,model,a.work/f'{n}-{v}',n,v)
                    for n,_ in notes for v in velocities}
            for n,_ in notes:
                for v in velocities:
                    root = min(sampler.ROOTS,key=lambda r:abs(r-n))
                    layer = v//8+1
                    source = next(r for r in mapping['layers'] if r['root']==root and r['layer']==layer)
                    source_path = Path(source['file'])
                    assert e.digest(source_path) == mapping['sources_sha256'][str(source_path.resolve())]
                    sounds[n,v,'Nuestro'] = e.read_model(jobs[n,v].result())*gain
                    sounds[n,v,'Steinway'] = e.render_reference(synth,n,v)
                    sources[f'{n}:{v}'] = dict(root=root,layer=layer,
                        transposition_semitones=n-root,file=str(source_path),
                        source_sha256=e.digest(source_path))
                    print(f'Rendered MIDI {n}, velocity {v}',flush=True)
    finally:
        synth.close()
    maximum = max(float(abs(x).max()) for x in sounds.values())
    trim = min(1,.85/maximum)
    gap = np.zeros((round(.5*e.RATE),2))
    def segment(x):
        y = x.copy()*trim
        y[-441:] *= np.linspace(1,0,441)[:,None]
        return y
    files = {}
    schedule = []
    for n,stem in notes:
        alternating = []
        for i,v in enumerate(velocities):
            for j,label in enumerate(('Nuestro','Steinway')):
                clip = segment(sounds[n,v,label])
                alternating.extend((clip,gap))
                schedule.append(dict(file=stem+'-comparacion.wav',seconds=(i*2+j)*6.5,
                                     instrument=label,note=n,velocity=v,hold_seconds=4))
                file = a.output/f'{stem}-{label}-v{v}.wav'
                sf.write(file,clip,e.RATE,subtype='PCM_24')
                files[file.name] = dict(frames=len(clip),peak=float(abs(clip).max()),sha256=e.digest(file))
        combined = a.output/f'{stem}-comparacion.wav'
        sf.write(combined,np.concatenate(alternating),e.RATE,subtype='PCM_24')
        for label in ('Nuestro','Steinway'):
            sequence = np.concatenate([part for v in velocities for part in
                                       (segment(sounds[n,v,label]),gap)])
            sf.write(a.output/f'{stem}-{label}-intensidades.wav',sequence,e.RATE,subtype='PCM_24')
    for wav in a.output.glob('*.wav'):
        x,rate = sf.read(wav,always_2d=True)
        assert rate == e.RATE and x.shape[1] == 2 and np.isfinite(x).all()
        assert 0 < abs(x).max() <= .850001 and sf.info(wav).subtype == 'PCM_24'
        files[wav.name] = dict(frames=len(x),seconds=len(x)/rate,
                              peak=float(abs(x).max()),sha256=e.digest(wav))
    report = dict(candidate=str(a.candidate.resolve()),candidate_sha256=e.digest(a.candidate),
        binary_sha256=e.digest(a.binary),sfz_sha256=e.digest(sfz),dll_sha256=e.digest(dll),
        mapping_sha256=e.digest(mapping_path),sources=sources,files=files,schedule=schedule,
        velocities=list(velocities),hold_seconds=4,release_tail_seconds=2,gap_seconds=.5,
        native_gain_db=gain_db,common_export_trim_db=float(20*np.log10(trim)),
        policy='Fixed prior model gain, original sampler velocity mapping, no per-note normalization. '
               'Stage ORTF stereo; no added EQ, reverb or compressor. 10ms fade at excerpt end only. '
               + ('F2 uses recorded F#2 pitched down one semitone. ' if 41 in selected else '')
               + 'F#2 uses the original pitch. '
               'Steinway note-off is a controlled sampler release, not a measured acoustic release.')
    (a.output/'comparacion.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8')
    credits = Path('local/reference-audio/bitklavier-grand/README.md').read_text(encoding='utf-8')
    (a.output/'CREDITOS-Steinway.md').write_text(credits,encoding='utf-8')
    (a.output/'LEEME.txt').write_text(
        'COMPARACIÓN DE GRAVES — modelo actual con contacto duplex corregido\n\n'
        + ('F2-comparacion.wav: F2 de nuestro modelo y Steinway (F#2 transpuesto -1 semitono).\n' if 41 in selected else '')
        +
        'Fs2-exacto-comparacion.wav: F#2 de ambos, muestra Steinway sin transposición.\n'
        'En cada pareja: NUESTRO primero, STEINWAY segundo.\n'
        'Velocidades MIDI: 40, 60, 84, 124.\n'
        '0:00 Nuestro v40; 0:06,5 Steinway v40\n'
        '0:13 Nuestro v60; 0:19,5 Steinway v60\n'
        '0:26 Nuestro v84; 0:32,5 Steinway v84\n'
        '0:39 Nuestro v124; 0:45,5 Steinway v124\n'
        '4 s de tecla sostenida, sin pedal; 2 s de cola y 0,5 s de separación.\n'
        'También hay intensidades por separado y cada nota/velocidad individual.\n'
        f'Ganancia fija del modelo: {gain_db} dB. Ajuste global común: {20*np.log10(trim):.4f} dB.\n'
        'Sin normalizar cada nota, EQ ni compresión. PCM24 estéreo, 44,1 kHz.\n'
        'Steinway: bitKlavier Grand, micrófonos Stage ORTF, reproducido con sfizz.\n'
        'La liberación del sampler no es una nueva grabación de note-off real.\n'
        'Créditos, CC BY 4.0 y modificaciones en CREDITOS-Steinway.md y comparacion.json.\n',encoding='utf-8')
    print(f'DONE: {a.output}; common trim {20*np.log10(trim):.4f} dB',flush=True)


if __name__ == '__main__':
    main()
