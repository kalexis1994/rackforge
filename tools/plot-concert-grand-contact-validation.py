"""Export the whole-keyboard regressions and the still-unresolved H1 decay."""
import argparse
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();r=json.loads(a.report.read_text());z=r['rows']
    notes=sorted({v['note'] for v in z.values()});vels=(28,60,100,124)
    values=np.array([[z[f'{n}:{v}']['error_change_db'] for v in vels] for n in notes])
    names=('C','C#','D','D#','E','F','F#','G','G#','A','A#','B')
    label=lambda n:f'{names[n%12]}{n//12-1}'
    fig,(ax,bx)=plt.subplots(1,2,figsize=(13,10),gridspec_kw={'width_ratios':[1,1.8]})
    im=ax.imshow(values,aspect='auto',cmap='RdYlGn_r',vmin=-2,vmax=2)
    ax.set_xticks(range(4),labels=vels);ax.set_yticks(range(len(notes)),labels=[label(n) for n in notes])
    ax.set_xlabel('Velocidad MIDI');ax.set_title('Cambio de error espectral inicial\nVerde: mejora / rojo: regresión')
    fig.colorbar(im,ax=ax,label='Cambio de error en dB (escala saturada a ±2)')
    selected=(60,72,84,90,96);x=np.arange(len(selected));width=.25
    for j,(key,title,color) in enumerate((('reference','Steinway','#20252e'),('baseline','14B','#a8afba'),('model','Fieltro experimental','#2f82b5'))):
        data=[]
        for n in selected:
            stages=z[f'{n}:124'][key]['stages'];data.append(stages[4]['partials_db'][0]-stages[2]['partials_db'][0])
        bx.bar(x+(j-1)*width,data,width,label=title,color=color)
    bx.axhline(0,color='black',linewidth=.7);bx.set_xticks(x,labels=[label(n) for n in selected])
    bx.set_ylabel('Cambio de nivel de H1 respecto al inicio, dB');bx.set_title('La corrección del contacto no corrige las pérdidas\nH1: 50–200 ms frente a 0,5–1,5 s; velocidad 124')
    bx.legend();bx.grid(axis='y',alpha=.2)
    fig.suptitle('Contacto experimental: 120 casos, 30 notas originales, ganancia congelada',fontsize=14)
    fig.tight_layout();fig.savefig(a.output,dpi=160);plt.close(fig)


if __name__=='__main__':main()
