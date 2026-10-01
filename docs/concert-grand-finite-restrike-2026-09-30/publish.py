from pathlib import Path
import hashlib
import json
import shutil
import subprocess

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt

r = Path('local/concert-grand-coherent-unison')
f = r/'frozen-attack-14b'
out = Path('C:/SHARED/Modelo-14B')
doc = Path('docs/concert-grand-finite-restrike-2026-09-30')
doc.mkdir(exist_ok=True)
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
model = json.loads((f/'model.json').read_text(encoding='utf-8'))
results = []
clips = []
for stem, clip_range, clip_name in [
    ('chopin-nocturno-op9-2', (0,9.5), 'Modelo-14B-nocturno-inicio'),
    ('liszt-la-campanella', (143,153), 'Modelo-14B-campanella-2m23s'),
]:
    wav = out/f'Modelo-14B-{stem}.wav'
    info = json.loads(wav.with_suffix('.json').read_text(encoding='utf-8'))
    assert info['model_provenance_sha256'] == sha(f/'model.json')
    assert info['binary_sha256'] == sha(f/'renderer.exe') == model['binary_sha256']
    assert info['wav_sha256'] == sha(wav)
    old_info = json.loads((Path('C:/SHARED/Modelo-14')/f'Modelo-14-{stem}.json').read_text())
    assert info['midi_sha256'] == old_info['midi_sha256']
    assert info['events_sha256'] == old_info['events_sha256']
    assert not info['unmatched_note_counts']
    peak = 0
    with sf.SoundFile(wav) as sound:
        assert sound.frames == info['total_frames'] and sound.channels == 2 and sound.samplerate == 44100 and sound.subtype == 'PCM_24'
        for block in sound.blocks(blocksize=441000):
            assert np.isfinite(block).all()
            peak = max(peak,float(abs(block).max()))
    assert peak <= .850001 and abs(peak-info['output_peak']) < 1e-6
    old_raw = np.memmap(r/'sustain14-midi'/stem/'performance.f32',dtype='<f4',mode='r').reshape(-1,2)
    raw_path = r/'attack14b-midi'/stem/'performance.f32'
    raw = np.memmap(raw_path,dtype='<f4',mode='r').reshape(-1,2)
    gain = 10**(info['gain_db']/20)
    start,end = [round(t*44100) for t in clip_range]
    clip_path = out/(clip_name+'.wav')
    sf.write(clip_path,raw[start:end].astype(float)*gain,44100,subtype='PCM_24')
    clip_info = dict(source_wav=str(wav),source_raw_sha256=sha(raw_path),original_piece_seconds=clip_range,
                     output_wav=str(clip_path),wav_sha256=sha(clip_path),gain_db=info['gain_db'],
                     policy='Exact contiguous excerpt at full-piece gain; no fades, EQ, compression, or reset at excerpt start.')
    clip_path.with_suffix('.json').write_text(json.dumps(clip_info,indent=2),encoding='utf-8')
    clips.append(clip_info)
    shutil.copy2(wav.with_suffix('.json'), doc/wav.with_suffix('.json').name)
    results.append(dict(stem=stem,seconds=info['total_frames']/44100,peak=peak,gain_db=info['gain_db'],
                        additional_common_trim_db=info['additional_common_trim_db'],
                        midi_and_events_identical=True,notes=info['note_attacks'],wav_sha256=sha(wav)))
    if stem.startswith('chopin'):
        assert np.array_equal(old_raw[:121792],raw[:121792]), 'Fresh attacks changed before first restrike'

assert results[0]['gain_db'] == results[1]['gain_db']
# Paired high-frequency onset measures on the actual final native audio.
events = (r/'sustain14-midi/liszt-la-campanella/events.txt').read_text().splitlines()
measures = []
for stem,lo,hi in [('chopin-nocturno-op9-2',4,4.01),('liszt-la-campanella',146.7,148.4)]:
    onsets = []
    for line in (r/'sustain14-midi'/stem/'events.txt').read_text().splitlines():
        fields = list(map(int,line.split()))
        if len(fields)==4 and fields[1]&0xf0 == 0x90 and fields[3] and lo <= fields[0]/44100 <= hi:
            onsets.append(fields)
    powers = []
    for sub in ['sustain14-midi','attack14b-midi']:
        raw = np.memmap(r/sub/stem/'performance.f32',dtype='<f4',mode='r').reshape(-1,2)
        mono = raw[:round((hi+1)*44100)].mean(axis=1).astype(float)
        hf = sosfilt(butter(4,8000,fs=44100,btype='highpass',output='sos'),mono)
        powers.append([float(np.mean(hf[at:at+88]**2)) for at,_,_,_ in onsets])
    deltas = [float(10*np.log10(max(b,1e-30)/max(a,1e-30))) for a,b in zip(*powers)]
    measures.append(dict(stem=stem,seconds=[lo,hi],onsets=[dict(seconds=at/44100,note=n,velocity=v,hf_0_2ms_change_db=d)
                         for (at,_,n,v),d in zip(onsets,deltas)],
                         aggregate_hf_0_2ms_change_db=float(10*np.log10(sum(powers[1])/sum(powers[0])))))

verification = dict(binary_sha256=sha(f/'renderer.exe'),model_sha256=sha(f/'model.json'),
                    unit_tests=dict(passed=108,failed=0,ignored=74),
                    legacy_fingerprints=['0x243e622a8b59910d','0x706b728c4d0a6b06'],
                    original14_off_switch_control_bit_identical=True,
                    nocturne_prefix_before_first_restrike_bit_identical=True,
                    fresh_parity=json.loads((r/'attack14b-verification/fresh-parity.json').read_text()),
                    stress=json.loads((r/'attack14b-verification/stress.json').read_text()),
                    performances=results,clips=clips,attack_measures=measures,
                    limitations='Diagnostic HF changes do not establish perceived pick removal. Linear finite modal reconstruction; no contact initialized from old moving strings. 4ms is a global audition calibration, not a measured physical contact duration. No desktop ASIO test.')
(doc/'verification.json').write_text(json.dumps(verification,indent=2),encoding='utf-8')
shutil.copy2(f/'model.json',doc/'model.json')
for source,name in [
    ('attack-nocturne-004-diagnosis/report.json','nocturne-source-ablation.json'),
    ('attack-147-diagnosis/report.json','campanella-source-ablation.json'),
    ('attack-nocturne-ramp-probes/report.json','nocturne-ramp-probes.json'),
]:
    shutil.copy2(r/source,doc/name)

baseline=Path('C:/Users/kalex/.codex/worktrees/concert-grand-refit/rackforge/plugins/concert-grand/src')
patch=subprocess.run(['git','diff','--no-index','--',str(baseline),str((r/'src').resolve())],capture_output=True)
assert patch.returncode==1
(doc/'research.patch').write_bytes(patch.stdout)
provenance=dict(model_sha256=sha(f/'model.json'),binary_sha256=sha(f/'renderer.exe'),
                source_sha256={p.name:sha(p) for p in (r/'src').glob('*.rs')},
                cumulative_patch_base=str(baseline),research_patch_sha256=sha(doc/'research.patch'),
                attack_source_tool_sha256=sha(Path('tools/audit-concert-grand-attack-sources.py')),
                midi_tool='C:/Users/kalex/.codex/worktrees/concert-grand-refit/rackforge/tools/render-concert-grand-midi.py',
                midi_tool_sha256=sha(Path('C:/Users/kalex/.codex/worktrees/concert-grand-refit/rackforge/tools/render-concert-grand-midi.py')))
(doc/'provenance.json').write_text(json.dumps(provenance,indent=2),encoding='utf-8')
readme='''# Modelo 14B: ataque finito al volver a tocar una cuerda

Prototipo para escuchar. Continúa el modelo 14; no modifica el plugin instalado.

## Diagnóstico de los pasajes señalados

- Nocturno, 4,005 s: G5, MIDI 79, velocidad 78. La misma nota sonó 2,344 s antes; una repetición puede encontrar una cuerda viva sin ser rápida.
- La Campanella, bajada alrededor de 2:27: contiene notas del registro alto que ya sonaron y otras repetidas a intervalos cortos. Muchas cuerdas agudas no tienen apagador.

Se renderizaron contrafactuales desde el inicio de los MIDI, conservando pedal, eventos y estado previo a cada fragmento. Quitar KNOCK_LEVEL/THUMP_BASE apenas cambia los picos por encima de 8 kHz; quitar CONTACT_BOARD_GAIN tampoco los explica. Repartir la excitación del golpe sí los modifica. Esto identifica un problema en la reconstrucción de nuevos ataques sobre voces vivas; no prueba que toda sensación de pick venga de ese mecanismo ni separa todos los ruidos del piano real.

## Corrección propuesta

La ruta anterior sustituye el estado complejo calculado del golpe por su magnitud con signo en la cuadratura de velocidad. Con las dos rampas compiladas a cero, lo entrega en una muestra. Desaparece la fase que distribuía el ataque entre parciales y cuerdas. El nuevo modo conserva ambas cuadraturas, el unísono y la contribución negativa inicial que suaviza el crecimiento. Solo la contribución nueva entra con una envolvente smoothstep; el movimiento que ya sonaba sigue evolucionando.

La variante seleccionada para audición usa RESTRIKE_PHASE_PRESERVE = 1 y MERGE_RAMP_S = 0,004 s, globalmente. Se conserva la configuración de tabla, pérdidas, fieltro, nivel mecánico y unísono del 14. No se añade EQ ni compresión. Se probaron 1,5 ms y 4 ms; el segundo reduce más el pico inicial de G5 en el pasaje señalado. **Los 4 ms son una calibración de audición, no una duración de contacto medida.**

Es una reconstrucción finita de estados modales. Todavía no simula el martillo entrando en contacto con las cuerdas inicializadas con su movimiento anterior. La rotación de la excitación en espera utiliza las pérdidas intrínsecas; no identifica un intercambio completo de energía martillo/cuerdas/tabla durante ese intervalo.

## Verificación

- 108 pruebas regulares y las dos huellas de audio heredadas pasan.
- La prueba analítica comprueba fase, magnitud y continuidad de una nueva contribución sobre un oscilador en movimiento, incluyendo fases negativas y distintas frecuencias/pérdidas.
- Con el modo nuevo desactivado, los dos fragmentos coinciden bit a bit con el 14.
- 16 notas aisladas sostenidas, C4/F4/G4/C5/G5/C6/G6/C7 a 60 y 124, coinciden bit a bit con el 14. No hay adelgazamiento de esas notas aisladas.
- En Nocturno, todo el audio anterior a la primera repetición que cambia coincide bit a bit.
- Dos secuencias de 529 eventos a 44,1/48 kHz y bloques de 256 frames producen salida finita y colas que decaen. No es una prueba de ASIO.
- Los MIDI completos conservan sus hashes de eventos y sus notas/tempos/controladores, con 10 s de cola.

verification.json contiene cambios de energía inicial por encima de 8 kHz en los ataques señalados. No es una puntuación perceptiva ni prueba que el pick haya desaparecido; hace falta escuchar el candidato. Al cambiar la fase, también cambia cómo se suma el nuevo golpe con el anterior: una diferencia de forma de onda no equivale por sí sola a pérdida de cuerpo.

## Audios

En C:\\SHARED\\Modelo-14B, estéreo 44,1 kHz/24 bit:

- Modelo-14B-nocturno-inicio.wav: primeros 9,5 s. La nota señalada sigue en 00:04.
- Modelo-14B-campanella-2m23s.wav: fragmento original de 2:23 a 2:33. El punto 2:27 está a los 4 s del fragmento.
- Modelo-14B-chopin-nocturno-op9-2.wav: pieza completa, 234,56 s con cola.
- Modelo-14B-liszt-la-campanella.wav: pieza completa, 285,16 s con cola.

Una ganancia común para las dos piezas y sus fragmentos, con la pequeña reducción adicional registrada en sus JSON para dejar el pico máximo en 0,85. No se normaliza cada pieza ni cada ataque por separado. Los archivos anteriores permanecen en C:\\SHARED\\Modelo-14.

## Reproducción

Binario/configuración congelados en local/concert-grand-coherent-unison/frozen-attack-14b. provenance.json registra hashes de fuentes, scripts y binario. research.patch conserva la investigación acumulada contra la carpeta de fuentes histórica especificada. El ajuste a las muestras y los coeficientes del modelo 14 no se vuelven a entrenar en esta revisión.
'''
(doc/'README.md').write_text(readme,encoding='utf-8')
for p in doc.iterdir():
    if p.is_file():shutil.copy2(p,out/p.name)
shutil.copy2(Path('tools/audit-concert-grand-attack-sources.py'),doc/'audit-attack-sources.py')
shutil.copy2(Path(__file__),doc/'publish.py')
print(json.dumps(dict(performances=results,attack_measures=measures),indent=2))
