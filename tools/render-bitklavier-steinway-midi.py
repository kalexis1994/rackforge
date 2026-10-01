"""Render the bitKlavier Grand with sfizz's native SFZ engine, offline.

Closest recorded pitch, at most one semitone away, 16 discrete recorded layers.
Velocity gain follows the published bitKlavier loader/player approach. This is
an independent SFZ mapping, not an export made by the bitKlavier application.
"""
import argparse
from collections import Counter
import ctypes as c
import hashlib
import json
import math
from pathlib import Path
import shutil

import mido
import numpy as np
import soundfile as sf

RATE = 44100
BLOCK = 1024
ROOTS = tuple(range(21,109,3))
NAMES = ('C','C#','D','D#','E','F','F#','G','G#','A','A#','B')


def note_name(n):
    return f'{NAMES[n%12]}{n//12-1}'


def digest(path):
    with path.open('rb') as f:
        h=hashlib.file_digest(f,'sha256')
    return h.hexdigest()


def build_bank(bank, dest):
    sources={}; layers=[]
    lines=['// bitKlavier Grand, Stage ORTF; CC BY 4.0. Credits in README.md.',
           '// Independent mapping. No added reverb, filters, compressor or loops.',
           '<control> set_cc64=0 set_cc66=0 set_cc67=0 set_cc20=0 set_cc21=0',
           '<global> sample_quality=10 volume_oncc67=-3 bend_up=200 bend_down=-200']
    for root in ROOTS:
        below=None
        for k in range(1,17):
            path=bank/'notes'/f'{note_name(root)}v{k}.wav'
            assert path.exists(), path
            with sf.SoundFile(path) as f:
                assert f.samplerate==RATE and f.channels==2 and f.subtype=='PCM_16'
                x=f.read(round(.4*RATE),dtype='float64')
                frames=f.frames
            level=20*math.log10(max(float(np.mean(np.sqrt(np.mean(x*x,axis=0)))),1e-12))
            dyn=30/16 if k==1 else level-below
            if dyn>50:dyn=30
            dyn+=4/16
            lo,hi=8*(k-1),8*k-1
            values={v:dyn*(v-hi)/(hi-lo)+(k-16)*4/16 for v in range(max(1,lo),hi+1)}
            base=max(values.values())
            curve=' '.join(f'amp_velcurve_{v}={10**((db-base)/20):.10f}' for v,db in values.items())
            region=dict(root=root,layer=k,lokey=max(21,root-1),hikey=min(108,root+1),
                        lovel=max(1,lo),hivel=hi,file=str(path.resolve()),
                        rms_0_400ms_db=level,velocity_gain_db=values,frames=frames)
            layers.append(region);sources[str(path.resolve())]=digest(path)
            zones=[(region['lokey'],region['hikey'])]
            if region['lokey']<=89<region['hikey']:
                zones=[(region['lokey'],89),(90,region['hikey'])]
            for low_key,high_key in zones:
                mode='one_shot' if low_key>=90 else 'no_loop'
                lines += [f'<group> trigger=attack loop_mode={mode} ampeg_attack=0.003 ampeg_decay=0 ampeg_sustain=100 ampeg_release=0.03 amp_veltrack=100',
                          f'<region> lokey={low_key} hikey={high_key} pitch_keycenter={root} lovel={max(1,lo)} hivel={hi} volume={base:.9f} {curve} sample={path.resolve().as_posix()}']
            below=level
    # Original release resonance: native Direct uses .2 * -6dB and exp(-held/2).
    lines.append('<group> trigger=release loop_mode=one_shot amp_veltrack=50 volume=-20 rt_decay=4.342944819 ampeg_attack=0.003 ampeg_release=0.003')
    for root in ROOTS:
        if root>90:continue
        for k in range(1,4):
            path=bank/'release_resonance'/f'harm{note_name(root)}v{k}.wav'
            assert path.exists(),path
            sources[str(path.resolve())]=digest(path)
            lo,hi=math.floor(128*(k-1)/3),math.floor(128*k/3)-1
            lines.append(f'<region> lokey={max(21,root-1)} hikey={min(89,root+1)} pitch_keycenter={root} lovel={max(1,lo)} hivel={hi} sample={path.resolve().as_posix()}')
    # Key-return recordings are per key, not transposed. .02 * -6dB = -40dB.
    lines.append('<group> trigger=release_key loop_mode=one_shot amp_veltrack=50 volume=-40 rt_decay=0 ampeg_attack=0.003 ampeg_release=0.003')
    for note in range(21,109):
        path=bank/'hammer_release'/f'rel{note-20}.wav'
        sources[str(path.resolve())]=digest(path)
        lines.append(f'<region> key={note} sample={path.resolve().as_posix()}')
    # Original end-of-travel pedal noises use hysteresis, independently of
    # the damper threshold. Mid-pedal sensor crossings are not full strokes.
    lines.append('<group> loop_mode=one_shot amp_veltrack=0 volume=-20 ampeg_attack=0.003 ampeg_release=0.003')
    for cc,direction in [(20,'D'),(21,'U')]:
        path=bank/'pedal'/f'pedal{direction}1.wav'
        sources[str(path.resolve())]=digest(path)
        lines.append(f'<region> lokey=-1 hikey=-1 on_locc{cc}=127 on_hicc{cc}=127 pitch_keytrack=0 sample={path.resolve().as_posix()}')
    dest.write_text('\n'.join(lines)+'\n',encoding='utf-8')
    return dict(layers=layers,sources_sha256=sources,region_count=496+72+88+2,
                policy=dict(velocity='16 equal MIDI bins, within-layer RMS based gain as in bitKlavier, +4dB extended range; no crossfade of different recorded attacks',
                    pitch='nearest recorded root +/-1 semitone; original roots not retuned',
                    sustain='CC64 >=64; native SFZ sustain. Intermediate sensor values delivered but no calibrated physical half-pedal model.',
                    sostenuto='Native SFZ CC66.',soft='CC67 continuous volume reduction up to 3dB, not an actual una-corda recording.',
                    release='30ms sampler release for damped keys; original resonance samples -20dB, 4.34294dB/s elapsed-time attenuation.',
                    hammer='Original key-return samples per key, -40dB, release_key; velocity follows the sampler trigger.',
                    pedal_noise='Recorded D1/U1 samples -20dB at alternating full-travel endpoints (>=115 / <=10), independent of the >=64 damper threshold; extra CC20/21 are internal cues, original MIDI unmodified.',
                    upper_keys='F#6 and above play recorded decay to its end; no artificial loop.',
                    envelope='3ms onset envelope; sampler reproduction, not untouched standalone WAV or a new acoustic performance.'))


class Engine:
    def __init__(self,dll,bank):
        self.lib=c.CDLL(str(dll.resolve()))
        def bind(name,restype,args):
            fn=getattr(self.lib,'sfizz_'+name);fn.restype=restype;fn.argtypes=args
            setattr(self,name,fn)
        ptr=c.c_void_p;integer=c.c_int
        bind('create_synth',ptr,[]);bind('free',None,[ptr])
        bind('set_sample_rate',None,[ptr,c.c_float]);bind('set_samples_per_block',None,[ptr,integer])
        bind('set_num_voices',None,[ptr,integer]);bind('get_num_active_voices',integer,[ptr])
        bind('get_num_regions',integer,[ptr]);bind('set_sample_quality',None,[ptr,integer,integer])
        bind('set_volume',None,[ptr,c.c_float]);bind('enable_freewheeling',None,[ptr])
        bind('load_file',c.c_bool,[ptr,c.c_char_p]);bind('get_unknown_opcodes',ptr,[ptr])
        bind('free_memory',None,[ptr]);bind('render_block',None,[ptr,c.POINTER(c.POINTER(c.c_float)),integer,integer])
        for name in ['send_note_on','send_note_off','send_cc','send_poly_aftertouch']:
            bind(name,None,[ptr,integer,integer,integer])
        for name in ['send_pitch_wheel','send_program_change','send_channel_aftertouch']:
            bind(name,None,[ptr,integer,integer])
        bind('send_bpm_tempo',None,[ptr,integer,c.c_float])
        bind('send_time_signature',None,[ptr,integer,integer,integer])
        self.s=self.create_synth();assert self.s
        self.set_sample_rate(self.s,RATE);self.set_samples_per_block(self.s,BLOCK)
        self.set_num_voices(self.s,512);self.set_sample_quality(self.s,1,10)
        self.set_volume(self.s,0);self.enable_freewheeling(self.s)
        assert self.load_file(self.s,str(bank.resolve()).encode()), 'SFZ load failed'
        unknown=self.get_unknown_opcodes(self.s)
        if unknown:
            text=c.string_at(unknown).decode();self.free_memory(unknown)
            assert not text, f'Unknown SFZ opcodes: {text}'
        self.buf=np.zeros((2,BLOCK),dtype=np.float32)
        self.channels=(c.POINTER(c.c_float)*2)(*[a.ctypes.data_as(c.POINTER(c.c_float)) for a in self.buf])
    def render(self,frames):
        self.render_block(self.s,self.channels,2,frames)
        return self.buf[:,:frames].T
    def close(self):
        self.free(self.s)


def midi_events(path):
    mid=mido.MidiFile(path);seconds=0;events=[];counts=Counter();channels=set();held=Counter()
    for msg in mid:
        seconds+=msg.time
        if msg.is_meta:
            if msg.type in ('set_tempo','time_signature'):events.append((round(seconds*RATE),msg))
            continue
        assert msg.type in ['note_on','note_off','control_change','program_change','pitchwheel','aftertouch','polytouch'],msg
        channels.add(msg.channel);counts[msg.type]+=1
        if msg.type=='control_change':assert msg.control not in (20,21),'Internal pedal cues conflict with MIDI'
        if msg.type=='note_on' and msg.velocity:
            assert 21<=msg.note<=108
            held[msg.note]+=1
        elif msg.type=='note_off' or msg.type=='note_on':held[msg.note]-=1
        events.append((round(seconds*RATE),msg))
    assert channels=={0},f'Single piano channel required, found {channels}'
    assert not any(held.values()),held
    return events,seconds,dict(counts)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--bank',type=Path,required=True);p.add_argument('--dll',type=Path,required=True)
    p.add_argument('--work',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('midi',type=Path,nargs='+');a=p.parse_args()
    a.work.mkdir(parents=True,exist_ok=True);a.output.mkdir(parents=True,exist_ok=True)
    sfz=a.work/'SteinwayD-StageORTF.sfz';mapping=build_bank(a.bank,sfz)
    mapping.update(sfz_sha256=digest(sfz),dll_sha256=digest(a.dll),script_sha256=digest(Path(__file__)),
                   engine='sfizz 1.2.3 official Windows x64 release; native C API; quality 10, freewheeling, 512 voices, stereo 44100Hz.')
    (a.work/'mapping.json').write_text(json.dumps(mapping,indent=2),encoding='utf-8')
    print('Bank prepared:',mapping['region_count'],'regions',flush=True)
    jobs=[]
    for path in a.midi:
        events,seconds,counts=midi_events(path)
        total=round((seconds+10)*RATE);d=a.work/path.stem;d.mkdir(exist_ok=True)
        raw=d/'performance.f32';peak=0;max_voices=0;pedal_endpoint_down=False;cue_count=0;next_event=0
        synth=Engine(a.dll,sfz)
        assert synth.get_num_regions(synth.s)==mapping['region_count']
        for _ in range(9):synth.render(BLOCK) # empty warmup; events still start at frame zero
        with raw.open('wb') as stream:
            for start in range(0,total,BLOCK):
                frames=min(BLOCK,total-start)
                while next_event<len(events) and events[next_event][0]<start+frames:
                    at,msg=events[next_event];delay=at-start;assert delay>=0
                    if msg.type=='set_tempo':synth.send_bpm_tempo(synth.s,delay,60e6/msg.tempo)
                    elif msg.type=='time_signature':synth.send_time_signature(synth.s,delay,msg.numerator,msg.denominator)
                    elif msg.type=='note_on' and msg.velocity:synth.send_note_on(synth.s,delay,msg.note,msg.velocity)
                    elif msg.type in ('note_on','note_off'):synth.send_note_off(synth.s,delay,msg.note,msg.velocity)
                    elif msg.type=='control_change':
                        synth.send_cc(synth.s,delay,msg.control,msg.value)
                        if msg.control==64:
                            now=pedal_endpoint_down
                            if msg.value>=115:now=True
                            elif msg.value<=10:now=False
                            if now!=pedal_endpoint_down:
                                synth.send_cc(synth.s,delay,20 if now else 21,127)
                                synth.send_cc(synth.s,delay,20 if now else 21,0)
                                cue_count+=1
                            pedal_endpoint_down=now
                    elif msg.type=='pitchwheel':synth.send_pitch_wheel(synth.s,delay,msg.pitch+8192)
                    elif msg.type=='program_change':synth.send_program_change(synth.s,delay,msg.program)
                    elif msg.type=='aftertouch':synth.send_channel_aftertouch(synth.s,delay,msg.value)
                    elif msg.type=='polytouch':synth.send_poly_aftertouch(synth.s,delay,msg.note,msg.value)
                    next_event+=1
                audio=synth.render(frames)
                assert np.isfinite(audio).all(),(path,start)
                peak=max(peak,float(abs(audio).max()));stream.write(audio.astype('<f4').tobytes())
                max_voices=max(max_voices,synth.get_num_active_voices(synth.s))
                if start//(30*RATE)!=(start+frames)//(30*RATE):print(path.name,round(start/RATE),'s',flush=True)
        assert next_event==len(events) and max_voices<512 and peak>0
        synth.close()
        info=dict(midi=str(path.resolve()),midi_sha256=digest(path),seconds=seconds,total_frames=total,
                  message_counts=counts,events_sent=len(events),pedal_noise_cues=cue_count,max_active_voices=max_voices,
                  raw_peak=peak,raw_sha256=digest(raw),engine=mapping['engine'],mapping_sha256=digest(a.work/'mapping.json'))
        jobs.append((path,raw,info));print('RENDERED',path.name,'peak',peak,'voices',max_voices,flush=True)
    gain=min(1,.85/max(info['raw_peak'] for _,_,info in jobs))
    for path,raw,info in jobs:
        audio=np.memmap(raw,dtype='<f4',mode='r').reshape(-1,2)
        wav=a.output/f'SteinwayD-StageORTF-{path.stem}.wav'
        with sf.SoundFile(wav,'w',samplerate=RATE,channels=2,subtype='PCM_24') as writer:
            for start in range(0,len(audio),RATE*10):writer.write(audio[start:start+RATE*10]*gain)
        info.update(wav=str(wav),wav_sha256=digest(wav),gain_db=20*math.log10(gain),output_peak=info['raw_peak']*gain,
                    sample_rate=RATE,channels=2,subtype='PCM_24',policy=mapping['policy'])
        wav.with_suffix('.json').write_text(json.dumps(info,indent=2),encoding='utf-8')
        print('WROTE',wav,flush=True)
    shutil.copy2(sfz,a.output/sfz.name);shutil.copy2(a.work/'mapping.json',a.output/'mapping.json')


if __name__=='__main__':main()
