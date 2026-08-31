"""The broadband attack noise of a piano, taken as the floor BETWEEN its
partials, and referred to the tonal energy of the same note.

Run it with no arguments to read the YDP reference; the model is measured by
importing `floor_of` and pointing it at a render.

During an attack the spectrum is a comb of tonal partials standing on a
broadband bed. The partials are the string; the bed is the mechanism, the
board's own noise and the string's nonlinear hash. Masking the bins that belong
to a partial -- their frequencies are known, f_n = n f0 sqrt(1 + B n^2), and B
is fitted per note off the recording itself -- and taking the median of what is
left gives the bed's shape without ever assuming what it should be.
"""
import importlib.util, io, json, math, os, struct
import numpy as np
ROOT=r"C:\Users\kalex\OneDrive\Documents\rackforge"
SF=r"C:\Users\kalex\OneDrive\Documents\rackforge-plugin-rf-dls\target\rf-soundfonts-fuel-test\assets\ydp-grand-piano.sf2"
spec=importlib.util.spec_from_file_location("t", os.path.join(ROOT,"tools","extract-piano-targets.py"))
EX=importlib.util.module_from_spec(spec); spec.loader.exec_module(EX)

def fit_B(x, rate, f0n, count=16):
    seg=x[int(0.08*rate):int(0.08*rate)+rate*2]
    if len(seg)<rate: return 0.0
    S=np.abs(np.fft.rfft(seg*np.hanning(len(seg)))); fr=np.fft.rfftfreq(len(seg),1/rate)
    ns,fs=[],[]
    for n in range(1,count+1):
        sel=(fr>=f0n*n*0.97)&(fr<f0n*n*1.05)
        if sel.sum()<3: break
        band=S[sel]; f=fr[sel]; k=int(np.argmax(band))
        ns.append(n); fs.append(f[k])
    if len(ns)<8: return 0.0
    ns=np.array(ns,float); fs=np.array(fs)
    y=(fs/ns)**2; A=np.vstack([np.ones_like(ns),ns**2]).T
    (c0,c1),*_=np.linalg.lstsq(A,y,rcond=None)
    return max(c1/c0,0.0) if c0>0 else 0.0

BANDS=[(30,80),(80,160),(160,320),(320,640),(640,1250),(1250,2500),(2500,5000),(5000,10000)]

def floor_of(x, rate, f0, B):
    """Median level of the non-partial bins, per band, over the attack."""
    seg=x[:int(0.08*rate)]
    if len(seg)<1024: return None
    w=np.hanning(len(seg))
    S=np.abs(np.fft.rfft(seg*w))**2
    fr=np.fft.rfftfreq(len(seg),1/rate)
    mask=np.ones(len(fr),bool)
    n=1
    while True:
        f=f0*n*math.sqrt(1+B*n*n)
        if f>fr[-1]: break
        # a partial owns a window three bins wide either side, plus 1.5%
        half=max(3*(fr[1]-fr[0]), f*0.015)
        mask &= ~((fr>f-half)&(fr<f+half))
        n+=1
    out=[]
    for lo,hi in BANDS:
        sel=(fr>=lo)&(fr<hi)&mask
        if sel.sum()<8: out.append(np.nan); continue
        # median density times the band's width: an energy, comparable to a sum
        out.append(10*math.log10(np.median(S[sel])*sel.sum()+1e-30))
    # Referred to the note's own TONAL energy -- everything the mask took out.
    # Without this the floor can only be compared after normalising it against
    # one of its own bands, and then an ablation that empties that band lifts
    # every other one by contrast. That trap has been walked into three times
    # in this model's history; this is the way out of it.
    tonal = 10*math.log10(S[~mask].sum()+1e-30)
    return np.array(out) - tonal

if __name__=="__main__":
    pool,headers=EX.parse(SF)
    best=EX.loudest_per_note(headers)
    rows=[]
    print("piso de ruido del instrumento real durante el ataque, referido a su tono")
    print(f"{'nota':>5} " + " ".join(f"{a}-{b}".rjust(9) for a,b in BANDS))
    for pitch,(name,start,end,rate,_p) in best.items():
        if pitch<36 or pitch>96 or (pitch-36)%6: continue
        x=pool[start:end].astype(np.float64)/32768.0
        f0=440*2**((pitch-69)/12)
        B=fit_B(x,rate,f0)
        fl=floor_of(x,rate,f0,B)
        if fl is None or np.isnan(fl).all(): continue
        rows.append(fl)
        print(f"{pitch:>5} " + " ".join(f"{v:>+9.1f}" for v in fl))
    mean=np.nanmean(np.array(rows),axis=0)
    print(f"{'medio':>5} " + " ".join(f"{v:>+9.1f}" for v in mean))
    print()
    print("dB bajo el tono de la misma nota. Un render del modelo se mide")
    print("importando floor_of y apuntandolo al wav; hoy queda 17 a 26 dB")
    print("por debajo de estos numeros en todas las bandas.")
