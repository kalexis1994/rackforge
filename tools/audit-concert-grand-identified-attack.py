"""Audit the frozen body prototype's onset, including the listener's 24 notes.

No refitting, gain changes, transposition or replacement of the installed piano.
"""
import argparse
import csv
import importlib.util
import json
from pathlib import Path

import numpy as np
import soundfile as sf
import concert_grand_attack_metrics as ATTACK


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--work', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    spec = importlib.util.spec_from_file_location('identified_body', Path(__file__).with_name('identify-concert-grand-body.py'))
    body = importlib.util.module_from_spec(spec); spec.loader.exec_module(body)
    artifact = a.work/'identified-body.npz'
    h = np.load(artifact)['kernels']
    selection = json.loads((a.work/'selection.json').read_text())
    assert body.FIT.AUDIT.sha(artifact) == selection['kernel_sha256']
    report = dict(kernel_sha256=selection['kernel_sha256'], source='unchanged frozen captures',
                  listener_verdict='Identified body has less attack and sounds further from Steinway.',
                  status='rejected by listening; not installed', groups={})
    for label, pairs in [('training', body.TRAIN), ('reserved-pitches', body.PITCH_TEST),
                         ('reserved-velocities', body.VELOCITY_TEST)]:
        rows, before, features = {}, {}, {}
        for n, v in pairs:
            key = f'{n}:{v}'
            directory = a.work/'capture'/f'{n}-{v}'
            meta = json.loads((directory/'manifest.json').read_text())
            assert meta['binary_sha256'] == body.SHA and meta['rate'] == body.RATE
            x = np.array(np.memmap(directory/'bridge-drive.f32', dtype='<f4', mode='r').reshape(-1,16))
            actual = body.FIT.AUDIT.load(directory/'performance.f32')
            target, rate = sf.read(a.work/'reference'/f'{n}-{v}.wav', always_2d=True)
            assert rate == body.RATE
            t, b, c = [ATTACK.measure(y,n,rate) for y in (target, actual, body.apply(x,h))]
            before[key], rows[key] = ATTACK.compare(t,b), ATTACK.compare(t,c)
            features[key] = dict(actual=b, identified=c, reference=t)
        report['groups'][label] = dict(guard=ATTACK.guards(rows,before), actual_rows=before,
                                      identified_rows=rows, features=features)
        print(label, json.dumps(report['groups'][label]['guard']), flush=True)
    audition = json.loads((a.output/'audition.json').read_text())['sequence']
    features, rows, before, hashes = {}, {}, {}, {}
    for item in audition:
        n,v = item['note'],item['velocity']; key=f'{n}:{v}'
        measured={}
        for label in ('Actual','Cuerpo-identificado','Steinway'):
            path=a.output/f'{n}-{v}-{label}.wav'
            y,rate=sf.read(path,always_2d=True); assert rate==body.RATE
            measured[label]=ATTACK.measure(y,n,rate)
            hashes[str(path)]=body.FIT.AUDIT.sha(path)
        features[key]=measured
        before[key]=ATTACK.compare(measured['Steinway'],measured['Actual'])
        rows[key]=ATTACK.compare(measured['Steinway'],measured['Cuerpo-identificado'])
    report['audition']=dict(guard=ATTACK.guards(rows,before), features=features,
                            actual_rows=before, identified_rows=rows, wav_sha256=hashes)
    (a.output/'attack-diagnosis.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    with (a.output/'attack-diagnosis.csv').open('w',newline='',encoding='utf-8') as stream:
        writer=csv.writer(stream)
        writer.writerow(['note','velocity','variant','0-5ms_dB','5-20ms_dB','20-50ms_dB','50-100ms_dB',
                         '100-200ms_dB','rise_5-20_dB','peak_time_ms','peak_level_dB','smoothing_ms'])
        for key, values in features.items():
            for label, f in values.items():
                writer.writerow([*key.split(':'),label,*f['levels_db'],f['rise_5_20_db'],
                                 f['peak_time_ms'],f['peak_level_db'],f['smoothing_ms']])
    print('audition',json.dumps(report['audition']['guard']),flush=True)


if __name__ == '__main__': main()
