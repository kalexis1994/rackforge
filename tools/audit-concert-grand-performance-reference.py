"""Evaluate existing complete MIDI renders with the isolated corpus's one gain."""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args()
    path=Path(__file__).with_name('evaluate-concert-grand-full-sound.py')
    spec=importlib.util.spec_from_file_location('piece_evaluation',path)
    e=importlib.util.module_from_spec(spec);spec.loader.exec_module(e)
    report=json.loads((a.output/'evaluation.json').read_text())
    gain=10**(report['gain_db']/20)
    result=dict(gain_db=report['gain_db'],policy='Same gain from isolated-note training corpus; original sfizz MIDI reference; aligned MIDI time zero, no per-piece or per-event gain/time matching. Pedal algorithms differ, so this is a performance comparison, not physical identification.',pieces={})
    for stem,passage in [('chopin-nocturno-op9-2',(3.7,4.4)),('liszt-la-campanella',(146.7,148.4))]:
        model_meta=json.loads((Path('C:/SHARED/Modelo-14B')/f'Modelo-14B-{stem}.json').read_text())
        reference=Path('C:/SHARED/SteinwayD-MIDI')/f'SteinwayD-StageORTF-{stem}.wav'
        reference_meta=json.loads(reference.with_suffix('.json').read_text())
        native_path=Path('local/concert-grand-coherent-unison/attack14b-midi')/stem/'performance.f32'
        assert model_meta['midi_sha256']==reference_meta['midi_sha256']
        assert e.digest(native_path)==model_meta['raw_sha256']
        assert e.digest(reference)==reference_meta['wav_sha256']
        assert model_meta['binary_sha256']==report['binary_sha256']
        native=np.memmap(native_path,mode='r',dtype='<f4').reshape(-1,2)
        ref,rate=sf.read(reference,always_2d=True)
        assert rate==e.RATE and abs(len(ref)-len(native))<=1
        frames=min(len(ref),len(native))
        native=np.asarray(native[:frames],dtype=np.float64)*gain
        ref=ref[:frames]
        rows=[]
        for start in range(0,frames-e.RATE,e.RATE):
            bands=[]
            for x in (ref,native):
                hz,power=e.psd(x[start:start+e.RATE])
                bands.append([float(e.db(power[(hz>=lo)&(hz<hi)].sum())) for lo,hi in e.BANDS])
            rows.append(dict(second=start/e.RATE,
                reference_level_db=float(e.db(e.power(ref[start:start+e.RATE]))),
                model_level_db=float(e.db(e.power(native[start:start+e.RATE]))),
                band_delta_db=(np.array(bands[1])-bands[0]).tolist()))
        lo,hi=[round(x*rate) for x in passage]
        passage_metrics={}
        for key,x in [('reference',ref[lo:hi]),('model',native[lo:hi])]:
            hz,power=e.psd(x)
            passage_metrics[key]=dict(total_db=float(e.db(e.power(x))),
                bands_db=[float(e.db(power[(hz>=low)&(hz<high)].sum())) for low,high in e.BANDS])
        result['pieces'][stem]=dict(midi_sha256=model_meta['midi_sha256'],
            native_sha256=e.digest(native_path),reference_sha256=reference_meta['wav_sha256'],
            sample_rate=rate,frames=frames,per_second=rows,passage=passage,
            passage_metrics=passage_metrics,
            passage_band_delta_db=(np.array(passage_metrics['model']['bands_db'])-passage_metrics['reference']['bands_db']).tolist())
        print('PIECE',stem,'passage band differences',result['pieces'][stem]['passage_band_delta_db'],flush=True)
    (a.output/'performance-comparison.json').write_text(json.dumps(result,indent=2),encoding='utf-8')


if __name__=='__main__':
    main()
