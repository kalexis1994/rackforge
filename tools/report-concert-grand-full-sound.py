"""Summarize full-sound diagnostics, with causal release controls and plots."""
import argparse
import csv
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
import soundfile as sf

RATE = 44100
NAMES = ('C','C#','D','D#','E','F','F#','G','G#','A','A#','B')


def label(n):
    return f'{NAMES[n%12]}{n//12-1}'


def load(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def control_events(note, velocity, mode):
    events = [(0,144,note,velocity)]
    if mode in ('release','repeat','pedal'):
        events.append((round(.8*RATE),128,note,64))
    if mode == 'repeat':
        events.extend(((RATE,144,note,velocity),(round(1.8*RATE),128,note,64)))
    if mode == 'pedal':
        events.extend(((round(.2*RATE),176,64,127),(round(2.5*RATE),176,64,0)))
    return sorted(events,key=lambda e:e[0])


def model_control(a, frozen, events, directory, evaluator):
    directory.mkdir(parents=True,exist_ok=True)
    raw = directory/'performance.f32'
    signature = dict(binary_sha256=frozen['binary_sha256'],model=frozen,
        events=events,seconds=5,rate=RATE)
    marker = directory/'manifest.json'
    signature = json.loads(json.dumps(signature))
    if not (raw.exists() and raw.stat().st_size == RATE*5*8 and marker.exists()
            and json.loads(marker.read_text()) == signature):
        event = directory/'events.txt'
        event.write_text('\n'.join(' '.join(map(str,e)) for e in events)+'\n')
        tuning = directory/'tuning.txt'
        tuning.write_text('\n'.join(f'{k} = {v}' for k,v in sorted(frozen['tuning'].items()))+'\n')
        env = {k:v for k,v in os.environ.items() if not k.upper().startswith('CG_')}
        env.update(CG_EVENT_FILE=str(event.resolve()),CG_TOTAL_FRAMES=str(RATE*5),CG_RATE=str(RATE),
            CG_TUNING=str(tuning.resolve()),CG_RENDER_DIR=str(directory.resolve()),CG_PRESET='concert',
            CG_PARAMS=','.join(f'{k}={v}' for k,v in frozen['params'].items()),RUST_MIN_STACK='33554432')
        with (directory/'render.log').open('w') as log:
            subprocess.run([str(a.binary.resolve()),'--exact','tests::render_reference_wavs','--ignored','--quiet'],
                env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        assert raw.stat().st_size == RATE*5*8
        marker.write_text(json.dumps(signature,indent=2),encoding='utf-8')
    x = evaluator.read_model(raw)
    assert np.isfinite(x).all()
    return x


def source_control(synth, events):
    synth.send_cc(synth.s,0,120,0)
    for cc in (64,66,67):
        synth.send_cc(synth.s,0,cc,0)
    for _ in range(16):
        synth.render(1024)
    assert synth.get_num_active_voices(synth.s) == 0
    x = np.empty((RATE*5,2),dtype=np.float64)
    next_event = 0
    for start in range(0,len(x),1024):
        frames = min(1024,len(x)-start)
        while next_event < len(events) and events[next_event][0] < start+frames:
            at,status,data,value = events[next_event]
            delay = at-start
            if status == 144:
                synth.send_note_on(synth.s,delay,data,value)
            elif status == 128:
                synth.send_note_off(synth.s,delay,data,value)
            else:
                synth.send_cc(synth.s,delay,data,value)
                if data == 64:
                    cue = 20 if value == 127 else 21
                    synth.send_cc(synth.s,delay,cue,127)
                    synth.send_cc(synth.s,delay,cue,0)
            next_event += 1
        x[start:start+frames] = synth.render(frames)
    assert np.isfinite(x).all() and next_event == len(events)
    return x


def control_metrics(x, baseline, anchor, evaluator):
    rows = []
    for lo,hi in ((0,.05),(.05,.2),(.2,.5),(.5,1),(1,2)):
        z = x[round((anchor+lo)*RATE):round((anchor+hi)*RATE)]
        b = baseline[round((anchor+lo)*RATE):round((anchor+hi)*RATE)]
        hz,p = evaluator.psd(z)
        _,q = evaluator.psd(b)
        rows.append(dict(after_event=[lo,hi],level_db=float(evaluator.db(evaluator.power(z))),
            delta_from_held_db=float(evaluator.db(evaluator.power(z))-evaluator.db(evaluator.power(b))),
            above8k_delta_from_held_db=float(evaluator.db(p[hz>=8000].sum())-evaluator.db(q[hz>=8000].sum()))))
    return rows


def stats(values):
    z = np.asarray([v for v in values if v is not None],dtype=float)
    if not len(z):
        return None
    return dict(count=len(z),median=float(np.median(z)),mean=float(z.mean()),
        mean_absolute=float(abs(z).mean()),p90_absolute=float(np.quantile(abs(z),.9)),
        min=float(z.min()),max=float(z.max()))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('output','work','model','binary','sfz','dll'):
        p.add_argument('--'+name,type=Path,required=True)
    a = p.parse_args()
    e = load(Path(__file__).with_name('evaluate-concert-grand-full-sound.py'),'full_evaluator')
    sampler = load(Path(__file__).with_name('render-bitklavier-steinway-midi.py'),'full_sampler')
    report = json.loads((a.output/'evaluation.json').read_text())
    frozen = json.loads(a.model.read_text())
    assert e.digest(a.binary) == frozen['binary_sha256'] == report['binary_sha256']
    rows = report['rows']
    exact = [r for r in rows.values() if r['transposition_semitones'] == 0]
    assert len(exact) == 480
    # Source replay check: separate gain/routing from changes to the recording.
    source_replay = []
    for r in exact:
        if not r['calibration_pair']:
            continue
        x,rate = sf.read(r['source_file'],frames=RATE*16,always_2d=True)
        assert rate == RATE
        x *= 10**(r['source_gain_db']/20)
        x = np.pad(x,((0,max(0,RATE*16-len(x))),(0,0)))
        f = e.features(x,r['note'],r['reference']['centers_hz'])
        source_replay.append(dict(note=r['note'],velocity=r['velocity'],
            onset_delta_s=r['reference']['onset_s']-f['onset_s'],
            level_delta_50_200ms_db=r['reference']['stages'][2]['total_db']-f['stages'][2]['total_db'],
            level_delta_0_20ms_db=r['reference']['stages'][0]['total_db']-f['stages'][0]['total_db']))
    summary = dict(gain_db=report['gain_db'],stages=[],registers={},dynamics={},
        source_replay_check=source_replay,checks=report['checks'])
    for index,(lo,hi) in enumerate(e.STAGES):
        used = [r for r in exact if r['differences'][index]['recorded_support']
                and r['differences'][index]['reference_above_floor']]
        summary['stages'].append(dict(stage=[lo,hi],level_delta_db=stats([r['differences'][index]['level_delta_db'] for r in used]),
            subfundamental_delta_db=stats([r['differences'][index]['subfundamental_delta_db'] for r in used]),
            spectral_mae_db=stats([r['spectrum'][1]['stage_mae_db'][index] for r in used]),
            both_channel_comparison=True))
    for name,low,high in [('bajos',21,47),('medios',48,71),('agudos',72,108)]:
        subset = [r for r in exact if low<=r['note']<=high]
        summary['registers'][name] = {}
        for category,predicate in [('suaves',lambda v:v<=36),('medias',lambda v:36<v<100),('fuertes',lambda v:v>=100)]:
            z = [r for r in subset if predicate(r['velocity'])]
            h1 = []
            for r in z:
                fa,fb = r['reference']['stages'],r['model']['stages']
                if r['differences'][4]['recorded_support'] and r['differences'][4]['partial_reliable'][0]:
                    h1.append((fb[4]['partials_db'][0]-fb[2]['partials_db'][0])-(fa[4]['partials_db'][0]-fa[2]['partials_db'][0]))
            summary['registers'][name][category] = dict(
                initial_level=stats([r['differences'][2]['level_delta_db'] for r in z]),
                body_level=stats([r['differences'][4]['level_delta_db'] for r in z if r['differences'][4]['recorded_support']]),
                H1_relative_decay_error_db=stats(h1),
                body_250_600=stats([r['differences'][4]['band_delta_db'][2] for r in z if r['differences'][4]['recorded_support']]))
    for n in e.ROOTS:
        weak,strong = rows[f'{n}:28'],rows[f'{n}:124']
        ref = strong['reference']['stages'][2]['total_db']-weak['reference']['stages'][2]['total_db']
        model = strong['model']['stages'][2]['total_db']-weak['model']['stages'][2]['total_db']
        summary['dynamics'][label(n)] = dict(reference_28_to_124_db=ref,model_28_to_124_db=model,error_db=model-ref)
    summary['dynamics_error_stats'] = stats([r['error_db'] for r in summary['dynamics'].values()])
    # Causal controls: identical pre-release trajectory, changed MIDI only.
    synth = sampler.Engine(a.dll,a.sfz)
    controls = {}
    audio = {}
    prefix_checks = []
    gain = 10**(report['gain_db']/20)
    for note in (60,66,72,84,90,96):
        for velocity in (60,124):
            group = {}
            for mode in ('held','release','pedal','repeat'):
                events = control_events(note,velocity,mode)
                model = model_control(a,frozen,events,a.work/'controls'/f'{note}-{velocity}-{mode}',e)*gain
                reference = source_control(synth,events)
                group[mode] = dict(model=model,reference=reference)
            for which in ('model','reference'):
                prefix = np.array_equal(group['held'][which][:round(.8*RATE)],group['release'][which][:round(.8*RATE)])
                assert prefix,(note,velocity,which,'prefix changed before release')
                prefix_checks.append(dict(note=note,velocity=velocity,engine=which,identical_before_off=prefix))
            key = f'{note}:{velocity}'
            controls[key] = {}
            for mode,anchor in [('release',.8),('pedal',2.5),('repeat',1)]:
                controls[key][mode] = dict(event_s=anchor,
                    model=control_metrics(group[mode]['model'],group['held']['model'],anchor,e),
                    reference_sampler=control_metrics(group[mode]['reference'],group['held']['reference'],anchor,e))
            if note in (60,72,84) and velocity == 124:
                audio[key] = group
            if velocity == 124:
                print('CONTROLLED',label(note),flush=True)
    synth.close()
    summary['controls'] = controls
    summary['control_prefix_checks'] = prefix_checks
    summary['release_limit'] = 'Sampler note-off uses a chosen 30ms envelope plus recorded noises. Neither its speed nor the library note-off timestamps identify real acoustic piano damping.'
    # Compact control audios, same gain and common additional playback trim.
    peak = max(float(abs(z).max()) for g in audio.values() for mode in ('release','pedal','repeat') for z in g[mode].values())
    trim = min(1,.85/peak)
    for which,name in [('reference','Steinway'),('model','Modelo-14B')]:
        pieces = []
        for n in (60,72,84):
            for mode in ('release','pedal','repeat'):
                z = audio[f'{n}:124'][mode][which].copy()*trim
                z[-662:] *= np.linspace(1,0,662)[:,None]
                pieces.extend((z,np.zeros((round(.4*RATE),2))))
        sf.write(a.output/f'{name}-liberacion-pedal-repeticion.wav',np.concatenate(pieces),RATE,subtype='PCM_24')
    summary['control_audio_common_trim_db'] = float(20*np.log10(trim))
    # Scientific static plots; full unrounded values remain in JSON/CSV.
    plt.rcParams.update({'font.size':9,'axes.spines.top':False,'axes.spines.right':False})
    fig,axes = plt.subplots(1,3,figsize=(14,10),layout='constrained')
    for ax,index,title in zip(axes,(0,2,4),('Ataque: 0–20 ms','Inicio tonal: 50–200 ms','Cuerpo: 0,5–1,5 s')):
        matrix = np.array([[rows[f'{n}:{v}']['differences'][index]['level_delta_db'] for v in e.VELOCITIES] for n in e.ROOTS])
        im = ax.imshow(matrix,aspect='auto',cmap='RdBu_r',vmin=-20,vmax=20)
        ax.set_title(title)
        ax.set_xticks(range(16),e.VELOCITIES,rotation=90)
        ax.set_yticks(range(30),[label(n) for n in e.ROOTS])
        ax.set_xlabel('Velocidad MIDI de la capa')
    fig.colorbar(im,ax=axes,label='Modelo − Steinway, dB. Azul: menos energía; rojo: más.',shrink=.75)
    fig.suptitle('Modelo 14B · una ganancia común · 480 pares · Stage ORTF')
    fig.savefig(a.output/'mapa-nivel.png',dpi=150)
    plt.close(fig)
    fig,axes = plt.subplots(2,3,figsize=(13,7),layout='constrained')
    for ax,n in zip(axes.flat,(60,66,72,84,90,96)):
        r = rows[f'{n}:124']
        for key,name,color in [('reference','Steinway','#3d7b62'),('model','14B','#bc5738')]:
            y = np.array(r[key]['envelope_10ms_db'])
            ax.plot(np.arange(len(y))*.01,y,label=name,color=color,lw=1)
        ax.axvline(12,color='#888',lw=.8,ls='--')
        ax.set(title=label(n)+' · v124',xlim=(0,16),ylim=(-100,-10),xlabel='Segundos desde inicio acústico',ylabel='dBFS')
        ax.grid(alpha=.18)
    axes.flat[0].legend()
    fig.suptitle('Envolventes completas · grabación reproducida y modelo · note-off alrededor de 12 s')
    fig.savefig(a.output/'envolventes-completas.png',dpi=150)
    plt.close(fig)
    fig,axes = plt.subplots(1,2,figsize=(12,4),layout='constrained')
    for n in (60,66,72,84,96):
        ref = [rows[f'{n}:{v}']['reference']['stages'][2]['total_db'] for v in e.VELOCITIES]
        model = [rows[f'{n}:{v}']['model']['stages'][2]['total_db'] for v in e.VELOCITIES]
        axes[0].plot(e.VELOCITIES,ref,label=label(n))
        axes[1].plot(e.VELOCITIES,model,label=label(n))
    for ax,title in zip(axes,('Steinway · capas de la biblioteca','Modelo 14B · ganancia común')):
        ax.set(title=title,xlabel='Velocidad MIDI',ylabel='Nivel 50–200 ms, dBFS')
        ax.grid(alpha=.18)
        ax.legend(fontsize=8)
    fig.savefig(a.output/'curvas-intensidad.png',dpi=150)
    plt.close(fig)
    (a.output/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
    print(json.dumps(dict(gain_db=summary['gain_db'],registers=summary['registers'],
        dynamics_error_stats=summary['dynamics_error_stats'],source_replay_check=source_replay),indent=2))


if __name__ == '__main__':
    main()
