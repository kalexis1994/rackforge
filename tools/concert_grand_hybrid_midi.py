"""Offline MIDI window and continuous bridge-force reconstruction for listening."""
from math import gcd
from pathlib import Path
import numpy as np
from scipy.signal import resample_poly


def excerpt(plan,cut=8.,tail=1.):
    rate=plan['contact_rate_hz'];first=next(e['tick'] for e in plan['events'] if e['kind']=='on')
    end=first+round(cut*rate)
    prefix=[dict(e,time_s=0.) for e in plan['events'] if e['tick']<first]
    if any(e['kind'] not in ('cc','program') for e in prefix):raise ValueError('cold pre-roll required')
    original=[dict(e,time_s=(e['tick']-first)/rate) for e in plan['events'] if first<=e['tick']<end]
    keys=sorted({e['key'] for e in original if e['kind']=='on'})
    if len({e['channel'] for e in prefix+original})!=1:raise ValueError('one piano channel required')
    held={k:0 for k in keys}
    for e in original:
        if e['kind']=='on':held[e['key']]+=1
        elif e['kind']=='off':held[e['key']]=max(0,held.get(e['key'],0)-1)
    synthetic=[dict(time_s=cut,kind='off',key=k,value=0,channel=0,synthetic=True)
               for k in keys for _ in range(held[k])]
    synthetic.append(dict(time_s=cut,kind='cc',key=64,value=0,channel=0,synthetic=True))
    events=prefix+original+synthetic
    for e in events:
        if e['kind']=='program' and e['key']!=0:raise ValueError('only frozen piano program zero')
        if e['kind']=='cc' and not(e['key'] in (64,67) or e['key']==66 and e['value']==0):raise ValueError('unsupported controller')
    return dict(keys=keys,events=events,original_events=original,synthetic_events=synthetic,
        source_first_attack_tick=first,removed_cold_silence_s=first/rate,cut_s=cut,tail_s=tail,duration_s=cut+tail)


def voice_events(window,key):
    result=[]
    for e in window['events']:
        if e['kind'] in ('on','off') and e['key']==key:kind=0 if e['kind']=='on' else 1
        elif e['kind']=='cc' and e['key'] in (64,67):kind=e['key']
        else:continue
        result.append((e['time_s'],kind,e['value']))
    return result


def bridge_force(coast,segments,contact_rate,capture_rate,output_rate=48000):
    """Anti-alias contact segments, keeping filter tails across transitions.

    No FFT block is normalized and no filter is restarted on the audio stream.
    Contact and coast are complementary sources on the same global timeline.
    """
    force=np.asarray(coast,dtype=float).copy()
    divisor=gcd(contact_rate,capture_rate);up=capture_rate//divisor;down=contact_rate//divisor
    for segment in segments:
        contact=np.fromfile(segment['path'],dtype='<f8')
        if len(contact)!=segment['frames']:raise ValueError('contact trace length changed')
        # Extend each sparse contact segment to retain both FIR tails. The
        # padding is a multiple of the decimation denominator, preserving phase.
        pad=32*down
        filtered=resample_poly(np.pad(contact,(pad,pad)),up,down)
        begin=segment['start_capture_frame']-32*up;end=begin+len(filtered)
        left=max(0,begin);right=min(len(force),end)
        if right>left:force[left:right]+=filtered[left-begin:right-begin]
    divisor=gcd(capture_rate,output_rate)
    result=resample_poly(force,output_rate//divisor,capture_rate//divisor)
    if not np.isfinite(result).all():raise ValueError('nonfinite bridge force')
    return result
