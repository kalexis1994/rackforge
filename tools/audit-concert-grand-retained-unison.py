"""Audit long unison tails, including treble partials up to 10 kHz.

Demodulate before downsampling: decimating the audio first would hide the
upper partials of C6/C7. A common baseline mask and fixed gain are used.
Recovered envelope dips are diagnostics, not independent audible defects.
"""
import argparse, json
from pathlib import Path
import numpy as np
from scipy.signal import butter, sosfiltfilt, resample_poly, find_peaks


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--audition', type=Path, required=True)
    p.add_argument('--renders', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a=p.parse_args(); j=json.loads(a.audition.read_text())
    fs=44100; er=fs/220; gain=10**(j['gain_db']/20)
    baseline=next(iter(j['profiles'])); result={}; freq={}; masks={}
    for name in j['profiles']:
        result[name]={}
        for note in j['notes']:
            raw=a.renders/name/f'long-{note}'/'performance.f32'
            x=np.fromfile(raw,dtype='<f4').reshape(-1,2)[:12*fs,0].astype(float)*gain
            assert np.isfinite(x).all()
            if name==baseline:
                clip=x[round(.05*fs):round(.45*fs)]
                spectrum=abs(np.fft.rfft(clip*np.hanning(len(clip)),n=262144))**2
                hz=np.fft.rfftfreq(262144,1/fs); f0=440*2**((note-69)/12)
                freq[note]=[]
                for h in range(1,17):
                    if h*f0>10000:break
                    indices=np.flatnonzero(abs(hz-h*f0)<.35*f0)
                    f=float(hz[indices[np.argmax(spectrum[indices])]])
                    if f<=10000:freq[note].append((h,f))
            rows=[]
            for h,f in freq[note]:
                # The carrier is removed at the original sample rate.
                z=resample_poly(x*np.exp(-2j*np.pi*f*np.arange(len(x))/fs),1,220)
                z=sosfiltfilt(butter(4,18,fs=er,output='sos'),z)
                db=20*np.log10(np.maximum(abs(z),1e-12)); t=np.arange(len(db))/er
                ceiling=float(db[(t>=.1)&(t<.5)].max())
                minima,props=find_peaks(-db,prominence=3,distance=round(.12*er))
                events=[dict(time_s=float(t[m]),depth_db=float(props['prominences'][i]))
                        for i,m in enumerate(minima) if .1<t[m]<11.5 and db[m]>max(ceiling-40,-90)]
                rows.append(dict(harmonic=h,frequency_hz=f,initial_db=ceiling,notches=events))
            if name==baseline:
                strongest=max(r['initial_db'] for r in rows)
                masks[note]=[r['initial_db']>max(strongest-35,-85) for r in rows]
            selected=[r for r,keep in zip(rows,masks[note]) if keep]
            result[name][str(note)]=dict(partials=selected,
                notches_by_interval=[sum(lo<=e['time_s']<hi for r in selected for e in r['notches'])
                    for lo,hi in ((.1,.5),(.5,1.5),(1.5,3.5),(3.5,7.5),(7.5,11.5))],
                levels_db=[float(10*np.log10(max(np.mean(x[round(lo*fs):round(hi*fs)]**2),1e-24)))
                    for lo,hi in ((.05,.2),(.5,1.5),(1.5,3.5),(3.5,7.5),(7.5,11.5))])
        print(name,[sum(r['notches_by_interval'][i] for r in result[name].values()) for i in range(5)],flush=True)
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(dict(policy=__doc__,audition=str(a.audition.resolve()),
        binary_sha256=j['binary_sha256'],profiles=j['profiles'],notes=j['notes'],velocity=j['long_velocity'],
        gain_db=j['gain_db'],results=result),indent=2))


if __name__=='__main__':main()
