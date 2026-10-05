"""Read original source trajectories plus measured note-off tails, without EQ.

The result isolates release; it is not an accepted bright/full piano model.
Linear addition of independently computed note tails is not shared-body MIDI.
"""
import argparse,json
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import numpy as np
from scipy.signal import resample_poly
from scipy.io import wavfile
from concert_grand_cell_audio import pressure_fft
s=spec_from_file_location('release_source_join',Path(__file__).with_name('render-concert-grand-exponential-note.py'));join=module_from_spec(s);s.loader.exec_module(join)

def read(path):
    with np.load(path) as d:return {k:d[k] for k in ['board_position','board_velocity']}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--shared',type=Path,required=True);p.add_argument('--reports',type=Path,required=True)
    p.add_argument('--audio',type=Path,required=True);p.add_argument('--report',type=Path,required=True)
    a=p.parse_args()
    if a.audio.exists() or a.report.exists():raise ValueError('preserve measurements')
    sh=a.shared;docs=a.reports;kernelpath=sh/'P6-primera-nota'/'Microfonos-128k.npz'
    with np.load(kernelpath) as d:kernel=d['kernel']
    clips={};rows={}
    for note in [60,64,67,72]:
        report=docs/'medidas-P6-release'/f'nota-{note}.json';r=json.loads(report.read_text())
        if not r.get('all_gates_passed'):raise ValueError('release checks must finish')
        for path,digest in r['inputs_sha256'].items():
            if join.digest(Path(path))!=digest:raise ValueError('release input hash')
        tailpath=sh/'P6-release'/str(note)/'Release-128000.npz'
        if join.digest(tailpath)!=r['cases']['128000']['sha256']:raise ValueError('release state hash')
        tail=read(tailpath)
        if note==60:
            onsetpath=sh/'P6-primera-nota'/'C4-124-250ms.npz';onset=read(onsetpath)
            onB,onV=onset['board_position'][:513],onset['board_velocity'][:513]
            prefixpath=sh/'P6-coast-exponencial-250ms'/'Coast-128000.npz'
            extensionpath=sh/'P6-nota-1s'/'Extension-128000.npz'
            t,B,V,_=join.joined_trace(onB,onV,read(prefixpath),read(extensionpath),128000)
            parent=json.loads((docs/'medidas-P6-nota-1s'/'render.json').read_text());expected=parent['cases']['128000']['extension_sha256']
            if join.digest(extensionpath)!=expected:raise ValueError('original sustain hash')
            first=json.loads((docs/'medidas-P6-primera-nota'/'sostenido-250ms.json').read_text())
            coast=json.loads((docs/'medidas-P6-coast-exponencial'/'sostenido-250ms.json').read_text())
            if join.digest(onsetpath)!=first['raw_sha256'] or join.digest(prefixpath)!=coast['cases']['128000']['trajectory_sha256']:raise ValueError('original onset/prefix hash')
            sources=[onsetpath,prefixpath,extensionpath]
        else:
            folder=sh/'P6-frase'/'notas'/f'{note}-124';onsetpath=folder/'Contact-768000.npz';prefixpath=folder/'Coast-128000.npz'
            onset=read(onsetpath);prefix=read(prefixpath);parent=json.loads((docs/'medidas-P6-frase'/f'nota-{note}.json').read_text())
            for path,expected in [(onsetpath,parent['contacts']['768000']['sha256']),(prefixpath,parent['coasts']['128000']['sha256'])]:
                if join.digest(path)!=expected:raise ValueError('original trajectory hash')
            end={k:prefix[k][-1:] for k in prefix};t,B,V,_=join.joined_trace(onset['board_position'],onset['board_velocity'],prefix,end,128000)
            sources=[onsetpath,prefixpath]
        j=dict(position=join.relative(tail['board_position'][0],B[-1]),velocity=join.relative(tail['board_velocity'][0],V[-1]))
        if max(j.values())>1e-12:raise ValueError('note-off state discontinuity')
        B=np.vstack([B,tail['board_position'][1:]]);V=np.vstack([V,tail['board_velocity'][1:]]);t=np.arange(len(B))/128000
        pressure=pressure_fft(t,B,V,kernel)[0][:len(t)+len(kernel)-1]
        hold_rms=float(np.sqrt(np.mean(pressure[round(.15*128000):round(.8*128000)]**2)))
        end_rms=float(np.sqrt(np.mean(pressure[-round(.02*128000):]**2)))
        end_db=float(20*np.log10(max(end_rms,1e-40)/hold_rms))
        # Do not introduce a new hard cut of an audible tail.
        if end_db>-60:raise ValueError(f'{note}: extend release; endpoint {end_db:.2f}dB')
        clips[note]=resample_poly(pressure*.25,3,8,axis=0).astype(np.float32)
        rows[str(note)]=dict(source_paths_sha256={str(x):join.digest(x) for x in sources+[tailpath,report]},join_relative=j,
            hold_rms_pa=hold_rms,end_rms_pa=end_rms,tail_endpoint_relative_db=end_db,release_duration_s=r['duration_s'])
    sequence=[60,64,67,72,67,64,60];lead=4800;spacing=57600
    audio=np.zeros((lead+(len(sequence)-1)*spacing+len(clips[60])+14400,2),dtype=np.float32);events=[]
    for i,note in enumerate(sequence):
        start=lead+i*spacing;clip=clips[note];audio[start:start+len(clip)]+=clip
        events.append(dict(note=note,velocity=124,note_on_s=start/48000,note_off_s=start/48000+1))
    peak=float(np.max(abs(audio)))
    if peak>=.98 or not np.isfinite(audio).all():raise ValueError('clipping or nonfinite audio')
    a.audio.parent.mkdir(parents=True,exist_ok=True);wavfile.write(a.audio,48000,audio)
    result=dict(audio_path=str(a.audio),audio_sha256=join.digest(a.audio),audio_rate_hz=48000,audio_seconds=len(audio)/48000,audio_peak=peak,
        notes=rows,events=events,common_gain_pa_to_full_scale=.25,kernel_sha256=join.digest(kernelpath),source_sha256=join.digest(Path(__file__)),
        scope='Release-only control; original timbre untouched. Full continuous source with note-off, no edit fade, EQ or pitch shift. Sum of independent cold notes with overlapping release tails, not shared-body polyphony or factory change.')
    join.write(a.report,result);print(json.dumps(result,indent=2))

if __name__=='__main__':main()
