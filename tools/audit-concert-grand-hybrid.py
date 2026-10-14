"""Authenticate every hybrid artifact and recompute its mechanical comparisons."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import numpy as np
from threadpoolctl import threadpool_limits
from concert_grand_native_strings import fixture
from concert_grand_modal_viability import project,omega,common_signal,signal_errors
from concert_grand_hybrid import hybrid_signal


def digest(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--measurement',type=Path,required=True);parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args();root=Path(__file__).resolve().parents[1]
    if args.report.exists():raise ValueError('fresh audit path required')
    d=json.loads(args.measurement.read_text(encoding='utf-8'))
    for name,sha in d['source_sha256'].items():
        if digest(root/'tools'/name)!=sha:raise ValueError('source changed: '+name)
    for prefix in ['parent','executable']:
        if digest(d[prefix+'_path'])!=d[prefix+'_sha256']:raise ValueError(prefix+' changed')
    for run in d['runs']+d['references']+[d['stress_reference']]:
        for prefix in ['coefficient','result','observation']:
            if prefix+'_path' in run and digest(run[prefix+'_path'])!=run[prefix+'_sha256']:
                raise ValueError('artifact changed: '+run['label'])
    spec=importlib.util.spec_from_file_location('wire_audit',root/'tools/audit-concert-grand-phase-wire-energy.py')
    audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)

    def read_input(run):
        raw=Path(run['coefficient_path']).read_bytes();magic,nt,nl,nodes=struct.unpack('<8sIII',raw[:20])
        if magic!=b'RFSTR001' or nl!=2*nt:raise ValueError('unexpected fixture')
        a=np.frombuffer(raw[20:],dtype='<f8');size=3*(nl+2*nt)+1;start=20+3*nt
        f=fixture(transverse=nt,rate=a[4],duration=a[19+3*nt])
        f['parameters']=a[:11];f['shapes']=a[11:11+3*nt];f['tensions']=a[11+3*nt:14+3*nt]
        f['nodes']=nodes
        for key,k in [('q',0),('endpoint',1),('v',2)]:f[key]=a[start+k*size:start+(k+1)*size]
        return f

    energy_max=linear_max=0.;cases=[]
    for run in d['runs']:
        f=read_input(run);result=json.loads(Path(run['result_path']).read_text(encoding='utf-8'))
        low=run['nonlinear_transverse'];small,indices=project(f,low,run['rate_hz'])
        high=np.setdiff1d(np.arange(len(f['q'])-1),indices);w=omega(f)
        q=np.asarray(result['q']);v=np.asarray(result['v'])
        def total(a,b):
            small_e=audit.wire_energies(small,np.r_[a[indices],a[-1]],np.r_[b[indices],b[-1]])
            high_e=.5*(b[high]**2+(w[high]*a[high])**2)
            return small_e+np.bincount(high//(4*f['transverse']),weights=high_e,minlength=3)
        a=total(f['q'],f['v']);b=total(q,v);relative=float(max(abs(b-a)/np.maximum(abs(a),1e-30)))
        original_a=audit.wire_energies(f,f['q'],f['v']);original_b=audit.wire_energies(f,q,v)
        original=float(max(abs(original_b-original_a)/np.maximum(abs(original_a),1e-30)))
        np.testing.assert_allclose(relative,run['hybrid_wire_energy_relative_max'],rtol=1e-3,atol=1e-14)
        np.testing.assert_allclose(original,run['original_wire_energy_relative_change'],rtol=1e-8,atol=1e-14)
        if not result['completed'] or result['allocations'] or relative>1e-9 or result['work_relative_max']>1e-9:
            raise ValueError('hybrid invariant failed')
        t=result['completed_microframes']/run['rate_hz'];phase=w[high]*t
        eq=np.cos(phase)*f['q'][high]+np.sin(phase)*f['v'][high]/w[high]
        ev=-w[high]*np.sin(phase)*f['q'][high]+np.cos(phase)*f['v'][high]
        linear=float(np.linalg.norm(np.r_[w[high]*(q[high]-eq),v[high]-ev])/max(np.linalg.norm(np.r_[w[high]*eq,ev]),1e-30))
        if linear>1e-9:raise ValueError('linear carrier changed')
        energy_max=max(energy_max,relative);linear_max=max(linear_max,linear)
    for case in d['cases']:
        run=next(r for r in d['runs'] if r['label']==case['label']);f=read_input(run)
        trace=json.loads(Path(run['observation_path']).read_text(encoding='utf-8'))
        candidate=hybrid_signal(f,run['nonlinear_transverse'],trace,run['rate_hz'])
        ref=next(r for r in d['references'] if r['label'].startswith(f"pulse-{int(case['peak_force_n'])}N-"))
        reference=common_signal(json.loads(Path(ref['observation_path']).read_text(encoding='utf-8')),ref['rate_hz'])
        quality=signal_errors(reference,candidate)
        np.testing.assert_allclose(quality['maximum_probe_velocity_relative'],case['signal_error']['maximum_probe_velocity_relative'],rtol=1e-12)
        trim=96;a=reference[trim:-trim];b=candidate[trim:-trim];window=np.hanning(len(a))[:,None]
        sa=np.fft.rfft(a*window,axis=0);sb=np.fft.rfft(b*window,axis=0);freq=np.fft.rfftfreq(len(a),1/48000)
        bands=[]
        for band in quality['bands']:
            selected=(freq>=band['low_hz'])&(freq<band['high_hz']);norm=max(np.linalg.norm(sa[selected]),1e-30)
            bands.append(band|dict(candidate_to_reference_rms=float(np.linalg.norm(sb[selected])/norm),
                magnitude_spectrum_relative=float(np.linalg.norm(abs(sb[selected])-abs(sa[selected]))/norm)))
        cases.append(dict(label=case['label'],bands=bands))
    record=dict(measurement_path=str(args.measurement.resolve()),measurement_sha256=digest(args.measurement),
        source_sha256=d['source_sha256']|{Path(__file__).name:digest(__file__)},authenticated_native_runs=len(d['runs']),
        references_authenticated=True,hybrid_wire_energy_relative_max=energy_max,high_linear_state_relative_max=linear_max,
        zero_processing_allocations=True,acoustic_pressure=False,audio_output_benchmarked=False,full_piano_realtime=False,
        cases=cases)
    args.report.write_text(json.dumps(record,indent=2,allow_nan=False)+'\n',encoding='utf-8')
    print('Authenticated',len(d['runs']),'native runs; energy',energy_max,'linear',linear_max)
    for c in cases:print(c['label'],[(b['low_hz'],round(b['candidate_to_reference_rms'],4),round(b['magnitude_spectrum_relative'],4)) for b in c['bands']])


if __name__=='__main__':
    with threadpool_limits(limits=1):main()
