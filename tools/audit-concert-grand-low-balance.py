"""Scale-independent low-band vs body-band energy for the rejected SI phrase."""
import argparse,json
from pathlib import Path
from scipy.io import wavfile
from scipy.fft import rfft,rfftfreq
import numpy as np
from importlib.util import spec_from_file_location,module_from_spec
s=spec_from_file_location('balance_hash',Path(__file__).with_name('audit-concert-grand-phrase-colour.py'));m=module_from_spec(s);s.loader.exec_module(m)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--shared',type=Path,required=True);p.add_argument('--reference',type=Path,required=True);p.add_argument('--report',type=Path,required=True);a=p.parse_args()
    if a.report.exists():raise ValueError('preserve results')
    rows={};sources={}
    for name,path,start in [('C4-pilot',a.shared/'P6-nota-1s'/'C4-124-Piloto-fisico-1s.wav',.1),
        ('C5-pilot',a.shared/'P6-frase'/'notas'/'72-124.wav',.1),('C4-reference',a.reference/'C4v16.wav',0),('C5-reference',a.reference/'C5v16.wav',0)]:
        rate,x=wavfile.read(path);x=x.astype(float);rows[name]={};sources[str(path)]=m.digest(path)
        for label,lo,hi in [('attack',0,.12),('sustain',.15,.8)]:
            y=x[round((start+lo)*rate):round((start+hi)*rate)];S=abs(rfft(y*np.hanning(len(y))[:,None],axis=0))**2;f=rfftfreq(len(y),1/rate)
            low=float(S[(f>=40)&(f<250)].sum());body=float(S[(f>=250)&(f<1200)].sum())
            rows[name][label]=dict(low_to_body_db=float(10*np.log10(max(low,1e-40)/body)))
    result=dict(windows_s=dict(attack=[0,.12],sustain=[.15,.8]),bands_hz=dict(low=[40,250],body=[250,1200]),notes=rows,inputs_sha256=sources,source_sha256=m.digest(Path(__file__)),
        scope='Recording-start windows for reference; 100ms lead removed for pilot. Energy ratios only, not absolute calibrated SPL or a tonal correction.')
    a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result,indent=2))

if __name__=='__main__':main()
