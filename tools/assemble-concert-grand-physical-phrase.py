"""First musical audition: separated, independently simulated physical notes.

No pitch shift, resampling, gain fitting, overlapping bodies or copied source
reports. This deliberately does not claim shared-board polyphony or action.
"""
import argparse,hashlib,json
from pathlib import Path
import numpy as np
from scipy.io import wavfile


def sha(path):
    with Path(path).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--c4-report',type=Path,required=True)
    p.add_argument('--note-reports',type=Path,nargs=3,required=True)
    p.add_argument('--audio',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True)
    args=p.parse_args()
    if args.audio.exists() or args.report.exists():raise ValueError('preserve measured files')
    clips={};sources={};kernel_hash=None
    for path in [args.c4_report]+args.note_reports:
        r=json.loads(path.read_text());n=r.get('note',60)
        if not r.get('all_gates_passed') or not r['gates'] or not all(r['gates'].values()):raise ValueError('validated completed note required')
        gain=r.get('gain',r.get('audition_gain_pa_to_full_scale'))
        if gain!=.25 or r['duration_s']!=1.:raise ValueError('fixed common gain/duration required')
        wav=Path(r['audio_path'] if 'audio_path' in r else r['paths']['audio'])
        if sha(wav)!=r['audio_sha256']:raise ValueError('note WAV changed')
        kh=next(v for k,v in r['inputs_sha256'].items() if Path(k).name=='Microfonos-128k.npz')
        if kernel_hash is not None and kh!=kernel_hash:raise ValueError('same microphones required')
        kernel_hash=kh;rate,a=wavfile.read(wav)
        if rate!=48000 or a.dtype!=np.float32 or a.ndim!=2 or a.shape[1]!=2 or not np.isfinite(a).all():raise ValueError('matching stereo float32 source')
        if not np.all(a[:4800]==0) or not np.all(a[-14400:]==0):raise ValueError('expected audition silence')
        clip=a[4800:-14400]
        if n in clips or len(clip)>57600:raise ValueError('unique notes shorter than phrase spacing')
        clips[n]=clip;sources[str(n)]=dict(report_path=str(path),report_sha256=sha(path),audio_path=str(wav),audio_sha256=sha(wav),clip_frames=len(clip))
    if set(clips)!={60,64,67,72}:raise ValueError('C4/E4/G4/C5 phrase bank required')
    sequence=[60,64,67,72,67,64,60];lead=4800;trail=14400;spacing=57600
    total=lead+(len(sequence)-1)*spacing+len(clips[sequence[-1]])+trail
    audio=np.zeros((total,2),dtype=np.float32);events=[]
    for i,n in enumerate(sequence):
        start=lead+i*spacing;clip=clips[n];audio[start:start+len(clip)]=clip
        events.append(dict(note=n,velocity=124,source_onset_s=i*1.2,source_hold_s=1.,wav_onset_s=start/48000,clip_frames=len(clip)))
    peak=float(np.max(abs(audio)))
    if not np.isfinite(audio).all() or peak>=.98:raise ValueError('nonfinite/clipping phrase')
    args.audio.parent.mkdir(parents=True,exist_ok=True);wavfile.write(args.audio,48000,audio)
    result=dict(events=events,note_sources=sources,microphone_kernel_sha256=kernel_hash,
        audio_path=str(args.audio),audio_sha256=sha(args.audio),audio_rate_hz=48000,audio_channels=2,
        audio_peak=peak,audio_seconds=len(audio)/48000,common_gain_pa_to_full_scale=.25,
        source_sha256=sha(Path(__file__)),
        transformations='Only remove existing listening silence and place each note at its stated onset. No pitch shift, per-note gain, additional fade, EQ or overlap.',
        scope='Cold one-string note bank assembled into a monophonic diagnostic arpeggio. Note endings retain the 8ms source-file edit; not simulated note-off, repeated-note interaction, shared-board polyphony or a plugin performance.',
        factory_changed=False)
    args.report.parent.mkdir(parents=True,exist_ok=True);args.report.write_text(json.dumps(result,indent=2,allow_nan=False),encoding='utf-8')
    print(json.dumps(dict(audio=str(args.audio),seconds=result['audio_seconds'],peak=peak)),flush=True)


if __name__=='__main__':main()
