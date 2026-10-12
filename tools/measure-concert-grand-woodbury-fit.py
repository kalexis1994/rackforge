"""Dense/low-rank objective parity and preparation cost on the loaded dataset."""
import argparse
import json
from pathlib import Path
import time
import numpy as np
from scipy.signal import find_peaks
from threadpoolctl import threadpool_limits
from concert_grand_coupled_mobility_fit import objective_gradient as dense
from concert_grand_woodbury_mobility_fit import objective_gradient as lowrank
from concert_grand_wave_cache import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--teacher-report', type=Path, required=True)
    parser.add_argument('--bank-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve prior measurement')
    teacher = json.loads(args.teacher_report.read_text())
    bank = json.loads(args.bank_report.read_text())
    if digest(teacher['data_path']) != teacher['data_sha256'] or digest(bank['model_path']) != bank['model_sha256']:
        raise ValueError('source data changed')
    root = Path(__file__).resolve().parent.parent
    names = [Path(__file__).name, 'concert_grand_coupled_mobility_fit.py', 'concert_grand_woodbury_mobility_fit.py']
    with np.load(teacher['data_path']) as t, np.load(bank['model_path']) as b:
        f = t['training_frequency_hz']
        h = t['mobility_training']
        trace = np.trace(h.real,axis1=1,axis2=2)
        peaks, _ = find_peaks(trace,prominence=trace.max()*1e-4)
        selected = np.unique(np.r_[np.arange(0,len(f),12),
            np.concatenate([np.arange(max(0,p-4),min(len(f),p+5)) for p in peaks]),len(f)-1]).astype(int)
        base = 2*b['gamma']
        port_count = b['ports'].shape[1]
        x = np.r_[np.zeros(len(base)),np.random.default_rng(17).normal(0,.025,(len(base),8)).ravel()]
        arguments = (x,b['omega'],base,b['ports'],f[selected],h[selected],8)
        with threadpool_limits(limits=6,user_api='blas'):
            original, actual = dense(*arguments), lowrank(*arguments)
            error = float(np.linalg.norm(actual[1]-original[1])/max(np.linalg.norm(original[1]),1e-30))
            loss_error = float(abs(actual[0]-original[0])/max(abs(original[0]),1e-30))
            if max(error,loss_error) > 1e-8:
                raise ValueError('Woodbury changed objective or gradient')
            timings = {name:[] for name in ['dense','woodbury']}
            for _ in range(5):
                for name, function in [('dense',dense),('woodbury',lowrank)]:
                    start = time.perf_counter()
                    function(*arguments)
                    timings[name].append(time.perf_counter()-start)
    report = dict(stage='loaded fit solver parity and offline timing',
        source_sha256={name:digest(root/'tools'/name) for name in names},
        teacher_report_sha256=digest(args.teacher_report), bank_report_sha256=digest(args.bank_report),
        frequencies=len(selected), modes=len(base), ports=port_count, loss_rank=8,
        objective_relative=loss_error, gradient_relative=error,
        seconds=timings, median_speedup=float(np.median(timings['dense'])/np.median(timings['woodbury'])),
        scope='Offline objective+gradient only on this CPU; same mathematical problem, not callback/full-render acceleration.',
        full_piano_realtime=False)
    args.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print('offline solver speedup',report['median_speedup'],'gradient relative',error)


if __name__ == '__main__':
    main()
