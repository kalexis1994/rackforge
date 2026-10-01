"""Compare the implemented modal observer to frequency-dependent Rayleigh pressure.

This audits the same assumed reduced plate, not an independently measured piano.
The physical bank coefficients are exported by export_physical_transfer.
"""
import argparse
import json
from pathlib import Path
import numpy as np


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--coefficients',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    c=np.genfromtxt(a.coefficients,delimiter=',',names=True)
    nx,ny=160,96
    xx,yy=np.meshgrid((np.arange(nx)+.5)/nx,((np.arange(ny)+.5)/ny-.5),indexing='ij')
    x,y=xx.ravel(),yy.ravel()
    along=c['a'][:,None]+c['b'][:,None]*np.cos(np.pi*c['q'][:,None]*x)+c['c'][:,None]*np.sin(np.pi*c['q'][:,None]*x)
    shapes=along*np.cos(np.pi*c['qy'][:,None]*y+c['theta_y'][:,None])
    geometry=[]
    for capsule in (.7-c['half_spacing'][0],.7+c['half_spacing'][0]):
        r=np.sqrt((x*1.4-capsule)**2+y*y+c['height'][0]**2)
        weight=((1-c['pattern'][0])+c['pattern'][0]*c['height'][0]/r)/r/len(x)
        geometry.append((r,weight))
    rows=[]
    for note in (48,60,66,72,78,84,90,96,102):
        pos=(note-21)/87
        phi=(c['a']+c['b']*np.cos(np.pi*c['q']*pos)+c['c']*np.sin(np.pi*c['q']*pos))*np.cos(c['theta_y'])
        for h in range(1,9):
            frequency=440*2**((note-69)/12)*h
            if frequency>16000:break
            w=2*np.pi*frequency/44100;z=np.exp(-1j*w)
            velocity=c['drive']*(1+z)/(1-c['a1']*z-c['a2']*z*c['half_step']*(1+z)/(1-z))
            old=[];proper=[]
            for side,(r,weight) in enumerate(geometry):
                old.append(np.sum(phi*velocity*(c['out_l' if side==0 else 'out_r']+c['out_l1' if side==0 else 'out_r1']*z)))
                pressure=1j*1.2*frequency*1.4*(shapes@(weight*np.exp(-1j*2*np.pi*frequency*r/343)))
                proper.append(np.sum(phi*velocity*pressure))
            levels=lambda q:(20*np.log10(np.maximum(abs(np.asarray(q)),1e-20))).tolist()
            rms=lambda q:10*np.log10(np.mean(abs(np.asarray(q))**2)+1e-40)
            rows.append(dict(note=note,partial=h,frequency=frequency,implemented_channels_db=levels(old),
                rayleigh_channels_db=levels(proper),change_db=float(rms(proper)-rms(old))))
    a.output.write_text(json.dumps(dict(policy=__doc__,quadrature=[nx,ny],rows=rows),indent=2))
    for r in rows:
        if r['partial'] in (1,2,4,8):print(r,flush=True)


if __name__=='__main__':main()
