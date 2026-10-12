"""Measure local rank omitted by pole-bank spatial directions.

This is a local diagnostic, not a lower bound on a coupled global fit.
Pressure projection uses the same real force-space directions at each probe.
"""
import argparse
import json
from pathlib import Path
import numpy as np
from scipy.signal import find_peaks
from concert_grand_wave_cache import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--teacher-report', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if args.report.exists():
        raise ValueError('preserve diagnostic')
    parent = json.loads(args.teacher_report.read_text())
    if digest(parent['data_path']) != parent['data_sha256']:
        raise ValueError('teacher data changed')
    with np.load(parent['data_path']) as data:
        mobility, pressure = data['mobility_training'], data['pressure_training']
        frequency = data['training_frequency_hz']
        trace = np.trace(mobility.real, axis1=1, axis2=2)
        peaks, _ = find_peaks(trace, prominence=trace.max()*1e-4)
        cases = []
        for count in [2,4,8,12,20]:
            residuals, pressure_residuals = [], []
            for index in peaks:
                r = (mobility[index].real+mobility[index].real.T)/2
                values, vectors = np.linalg.eigh(r)
                basis = vectors[:, -count:]
                approximation = (basis*values[-count:]) @ basis.T
                residuals.append(float(np.linalg.norm(r-approximation)/np.linalg.norm(r)))
                projected = pressure[index] @ basis @ basis.T
                pressure_residuals.append(float(np.linalg.norm(pressure[index]-projected)/np.linalg.norm(pressure[index])))
            cases.append(dict(directions=count, local_real_mobility_relative=residuals,
                local_pressure_force_projection_relative=pressure_residuals,
                mobility_median=float(np.median(residuals)), mobility_worst=max(residuals),
                pressure_median=float(np.median(pressure_residuals)), pressure_worst=max(pressure_residuals)))
    report = dict(stage='local loaded pole spatial-direction diagnostic', producer_sha256=digest(__file__),
        teacher_report_sha256=digest(args.teacher_report), peak_frequency_hz=frequency[peaks].tolist(), cases=cases,
        lower_bound_on_global_coupled_model=False, full_piano_realtime=False,
        scope='Training peaks only; truncation of real mobility eigendirections and corresponding force-space microphone projection. Not independent validation.')
    args.report.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print([(r['directions'],r['mobility_worst'],r['pressure_worst']) for r in cases])


if __name__ == '__main__':
    main()
