"""Compare bridge-force decay on identical original MIDI strikes, without gain fitting."""
import argparse
import hashlib
import json
from pathlib import Path
import numpy as np
from scipy.io import wavfile


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before',type=Path,required=True)
    parser.add_argument('--after',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args()
    if args.report.exists():raise ValueError('fresh report required')
    before=json.loads(args.before.read_text(encoding='utf-8'))
    after=json.loads(args.after.read_text(encoding='utf-8'))
    original=after['excerpt']['original_events'];end=after['excerpt']['cut_s']
    assert original==[e for e in before['excerpt']['original_events'] if e['time_s']<end]
    assert before['midi_sha256']==after['midi_sha256']
    variants=[d['variants'][0] for d in (before,after)]
    forces=[]
    for variant in variants:
        by_key={}
        for voice in variant['voices']:
            if voice['key'] not in after['excerpt']['keys']:continue
            path=voice['audio_force_path']
            assert digest(path)==voice['audio_force_sha256']
            by_key[voice['key']]=np.memmap(path,dtype='<f8',mode='r')
        forces.append(by_key)
    rows=[]
    for key in after['excerpt']['keys']:
        ons=[e for e in original if e['kind']=='on' and e['key']==key]
        offs=[e['time_s'] for e in original if e['kind']=='off' and e['key']==key]
        for i,e in enumerate(ons):
            t=e['time_s']
            following=ons[i+1]['time_s'] if i+1<len(ons) else end
            if following<t+.08 or t+.08>end:continue
            off=next((x for x in offs if x>=t),end)
            pedal=next((x['value'] for x in reversed(after['excerpt']['events'])
                if x['time_s']<=t+.05 and x['kind']=='cc' and x['key']==64),0)
            measurements=[]
            for collection in forces:
                f=collection[key]
                def rms(a,b):
                    x=f[round((t+a)*48000):round((t+b)*48000)]
                    return float(np.sqrt(np.mean(x*x)))
                early=rms(0,.02);late=rms(.04,.08)
                measurements.append(dict(attack_rms_n=early,tail_rms_n=late,tail_to_attack=late/max(early,1e-30)))
            rows.append(dict(key=key,time_s=t,velocity=e['value'],held_ms=(off-t)*1000,
                pedal_at_50ms=pedal,before=measurements[0],after=measurements[1]))
    groups={}
    for name,group in [('all',rows),('pedal_below_40',[r for r in rows if r['pedal_at_50ms']<40]),
        ('undamped_register',[r for r in rows if r['key']>=after['undamped_from_midi']]),
        ('held_at_least_90ms',[r for r in rows if r['held_ms']>=90])]:
        groups[name]=dict(strikes=len(group),**{side:dict(
            median_tail_to_attack=float(np.median([r[side]['tail_to_attack'] for r in group])) if group else None,
            tail_below_one_percent=sum(r[side]['tail_to_attack']<.01 for r in group)) for side in ('before','after')})
    waves=[]
    for variant in variants:
        assert digest(variant['wav_path'])==variant['wav_sha256']
        rate,audio=wavfile.read(variant['wav_path']);assert rate==48000
        waves.append(audio)
    same_gain=before['common_listening_gain_per_pa']==after['common_listening_gain_per_pa']
    prefix_equal=same_gain and np.array_equal(waves[0][:round(8.1*48000)],waves[1][:round(8.1*48000)])
    report=dict(before_report=str(args.before.resolve()),before_sha256=digest(args.before),
        after_report=str(args.after.resolve()),after_sha256=digest(args.after),
        measurement_source_sha256=digest(__file__),same_midi=True,no_gain_or_time_fit=True,
        identical_listening_gain=same_gain,first_8s_plus_lead_bit_identical=bool(prefix_equal),
        diagnostic='Per-key bridge force: RMS 40..80 ms / RMS 0..20 ms. No intervening strike. Not a perceptual score.',
        groups=groups,strikes=rows)
    args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print(json.dumps(dict(groups=groups,first_8s_plus_lead_bit_identical=bool(prefix_equal)),indent=2))


if __name__=='__main__':main()
