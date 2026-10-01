"""Plot raw-recording and musical-context diagnostics without refitting."""
import argparse
import csv
import json
from pathlib import Path

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np

NAMES=('C','C#','D','D#','E','F','F#','G','G#','A','A#','B')


def label(n):
    return f'{NAMES[n%12]}{n//12-1}'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    raw=json.loads((a.output/'raw-recordings.json').read_text())
    music=json.loads((a.output/'performance-comparison.json').read_text())
    roots=tuple(range(21,109,3));velocities=tuple(range(4,128,8));rows=raw['rows']
    plt.rcParams.update({'font.size':9,'axes.spines.top':False,'axes.spines.right':False})
    fig,axes=plt.subplots(1,3,figsize=(14,10),layout='constrained')
    for ax,index,title in zip(axes,(0,2,4),('Ataque: 0–20 ms','Inicio tonal: 50–200 ms','Cuerpo: 0,5–1,5 s')):
        matrix=np.array([[rows[f'{n}:{v}']['differences'][index]['level_delta_db'] for v in velocities] for n in roots])
        im=ax.imshow(matrix,aspect='auto',cmap='RdBu_r',vmin=-20,vmax=20)
        ax.set_title(title)
        ax.set_xticks(range(16),velocities,rotation=90)
        ax.set_yticks(range(30),[label(n) for n in roots])
        ax.set_xlabel('Velocidad MIDI representativa')
    fig.colorbar(im,ax=axes,label='Modelo − grabación, dB. RMS sin ponderación perceptiva.',shrink=.75)
    fig.suptitle('Grabaciones originales Stage ORTF · 480 pares · una ganancia común al 14B')
    fig.savefig(a.output/'mapa-grabaciones-originales.png',dpi=150)
    plt.close(fig)
    fig,axes=plt.subplots(2,2,figsize=(12,7),layout='constrained')
    for ax,n in zip(axes.flat,(60,72,84,96)):
        r=rows[f'{n}:124']; anchor=r['reference']['stages'][2]['partials_db'][0]
        for which,name,color in [('reference','Steinway','#3d7b62'),('model','14B','#bc5738')]:
            for index,stage,style in [(2,'inicio','-'),(4,'cuerpo','--')]:
                values=np.array(r[which]['stages'][index]['partials_db'])-anchor
                ax.plot(np.arange(len(values))+1,values,style,color=color,label=f'{name} · {stage}',lw=1.3)
        ax.set(title=f'{label(n)} · velocidad 124',xlabel='Parcial',ylabel='dB respecto a H1 original del inicio',ylim=(-70,10))
        ax.set_xticks(range(1,min(24,len(values))+1,2))
        ax.grid(alpha=.18)
    axes.flat[0].legend(fontsize=8)
    fig.suptitle('Distribución tonal: inicio 50–200 ms y cuerpo 0,5–1,5 s\nAmbas fuentes usan el mismo anclaje visual; no se igualan notas ni parciales.')
    fig.savefig(a.output/'parciales-inicio-cuerpo.png',dpi=150)
    plt.close(fig)
    fig,axes=plt.subplots(1,2,figsize=(12,4.8),layout='constrained')
    labels=['25–100','100–250','250–600','600–1500','1500–4000','4000–8000','8000–16000']
    for ax,(stem,piece) in zip(axes,music['pieces'].items()):
        x=np.arange(len(labels))
        anchor=piece['passage_metrics']['reference']['total_db']
        for offset,key,name,color in [(-.18,'reference','Steinway','#3d7b62'),(.18,'model','14B','#bc5738')]:
            y=np.array(piece['passage_metrics'][key]['bands_db'])-anchor
            ax.bar(x+offset,y,width=.36,label=name,color=color)
        title='Nocturno · 3,7–4,4 s' if stem.startswith('chopin') else 'La Campanella · 2:26,7–2:28,4'
        ax.set(title=title,ylabel='dB respecto al RMS total de la referencia',ylim=(-75,5))
        ax.set_xticks(x,labels,rotation=45,ha='right')
        ax.set_xlabel('Banda, Hz')
        ax.grid(axis='y',alpha=.2)
        ax.legend()
    fig.suptitle('Pasajes MIDI: una calibración para las dos piezas\nReferencia de sampler; notas superpuestas y distintos algoritmos de pedal.')
    fig.savefig(a.output/'pasajes-midi.png',dpi=150)
    plt.close(fig)
    with (a.output/'grabaciones-originales-por-nota.csv').open('w',newline='',encoding='utf-8') as stream:
        writer=csv.writer(stream)
        writer.writerow(['note','velocity','initial_50_200ms_level_delta_db','body_500_1500ms_level_delta_db',
                         'H1_relative_decay_error_db','early_subfundamental_relative_total_error_db',
                         'early_subfundamental_reliable','body_support'])
        for r in rows.values():
            fr,fm=r['reference']['stages'],r['model']['stages'];d=r['differences']
            h1=(fm[4]['partials_db'][0]-fm[2]['partials_db'][0])-(fr[4]['partials_db'][0]-fr[2]['partials_db'][0])
            writer.writerow([r['note'],r['velocity'],d[2]['level_delta_db'],d[4]['level_delta_db'],h1,
                d[1]['subfundamental_relative_total_error_db'],d[1]['subfundamental_reliable'],d[4]['recorded_support']])
    print('PLOTS DONE',a.output,flush=True)


if __name__=='__main__':
    main()
