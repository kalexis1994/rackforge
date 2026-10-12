"""Inspect native coefficients; this does not trace intermediate arithmetic."""
import argparse
import json
from pathlib import Path
import struct
import numpy as np
from concert_grand_wave_cache import digest


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--runtime-report',type=Path,required=True)
    p.add_argument('--report',type=Path,required=True)
    a = p.parse_args()
    if a.report.exists():
        raise ValueError('preserve old diagnostic')
    parent = json.loads(a.runtime_report.read_text())
    results = []
    for row in parent['rates']:
        path = Path(row['coefficient_path'])
        if digest(path)!=row['coefficient_sha256']:
            raise ValueError('measured coefficients changed')
        raw = path.read_bytes()
        if raw[:8]!=b'RFRCM001':
            raise ValueError('reciprocal coefficient format required')
        n,ports,dt = struct.unpack('<IId',raw[8:24])
        values = np.frombuffer(raw,dtype='<f8',offset=24)
        if len(values)!=9*n+2*n*ports:
            raise ValueError('coefficient size mismatch')
        c = values[:8*n].reshape(n,8)
        nonzero = abs(c[:,0][c[:,0]!=0])
        results.append(dict(rate_hz=1/dt,coefficient_sha256=digest(path),
            subnormal_coefficients=int(np.sum((abs(values)>0)&(abs(values)<np.finfo(float).tiny))),
            minimum_nonzero_transition_real=float(nonzero.min())))
    report = dict(stage='native subnormal coefficient inspection',producer_sha256=digest(__file__),
        runtime_report_sha256=digest(a.runtime_report),cases=results,
        intermediate_arithmetic_or_live_states_traced=False,
        explains_timing_variance=False,full_piano_realtime=False)
    a.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print('subnormal coefficients',[r['subnormal_coefficients'] for r in results])


if __name__=='__main__':
    main()
