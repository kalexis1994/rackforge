import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from concert_grand_realtime_pressure import PressureRuntime,fit_digital_state_readout


class RealtimePressureTests(unittest.TestCase):
    def runtime(self, rate=44100):
        return PressureRuntime(np.array([130., 410., 850.]),
            np.array([[65., 3., .2], [3., 90., 4.], [.2, 4., 130.]]),
            np.array([[.6, -.2], [.3, .7], [-.4, .1]]),
            np.array([[1., .2], [-.3, .6], [.1, -.8]]),
            np.array([[.002, .003], [.0021, .0032]]),
            np.array([[[.2, -.1, .4], [.3, .2, -.1]], [[-.2, .3, .1], [.1, -.4, .2]]]), rate)

    def test_native_pressure_states_ports_and_work(self):
        root=Path(__file__).resolve().parents[2]
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder)
            exe=folder/'pressure.exe'
            subprocess.run(['rustc','--edition=2021','-O',str(root/'tools/concert-grand-pressure-body.rs'),'-o',str(exe)],check=True)
            for rate in [44100,48000]:
                runtime=self.runtime(rate)
                path=folder/f'{rate}.bin'
                runtime.write(path)
                native=json.loads(subprocess.check_output([str(exe),str(path),'verify'],text=True))
                scalar=json.loads(subprocess.check_output([str(exe),str(path),'verify','scalar'],text=True))
                np.testing.assert_allclose(native['pressure'],scalar['pressure'],rtol=1e-12,atol=1e-12)
                np.testing.assert_allclose(native['real'],scalar['real'],rtol=1e-12,atol=1e-14)
                np.testing.assert_allclose(native['imag'],scalar['imag'],rtol=1e-12,atol=1e-14)
                self.assertLess(native['held_port_identity_max_relative'],1e-8)
                pressure=[]; work=checksum=0.
                for frame in range(1200):
                    force=np.array([((frame*17+port*13)%97-48)/48 if frame<400 else 0. for port in range(3)])
                    v,p=runtime.process(force)
                    pressure.append(p)
                    work+=runtime.step.dt*force @ v
                    checksum+=np.arange(1,4) @ v
                np.testing.assert_allclose(native['pressure'],pressure,rtol=2e-10,atol=2e-11)
                np.testing.assert_allclose(native['real'],runtime.state.real,rtol=1e-10,atol=1e-13)
                np.testing.assert_allclose(native['imag'],runtime.state.imag,rtol=1e-10,atol=1e-13)
                np.testing.assert_allclose([native['work_j'],native['checksum']],[work,checksum],rtol=1e-10,atol=1e-13)

    def test_general_state_and_force_output_matches_measured_response(self):
        runtime=self.runtime(rate=12000)
        rng=np.random.default_rng(73)
        runtime.set_state_readout(rng.normal(size=(2,2,6)),rng.normal(size=(2,2,2)))
        hz=137.;force=np.array([.3,-.4,.2]);signal=[]
        for frame in range(16000):
            _,p=runtime.process(force*np.cos(2*np.pi*hz*frame/12000))
            signal.append(p)
        times=np.arange(14000,16000)/12000
        design=np.column_stack([np.cos(2*np.pi*hz*times),-np.sin(2*np.pi*hz*times)])
        fit=np.linalg.lstsq(design,np.asarray(signal)[14000:],rcond=None)[0]
        np.testing.assert_allclose(fit[0]+1j*fit[1],runtime.harmonic_response([hz])[0] @ force,rtol=1e-9,atol=1e-9)

    def test_fit_preserves_static_and_first_order_pressure_zeros(self):
        runtime=self.runtime()
        runtime.mapping=np.eye(2)
        train=np.geomspace(13.,1700.,60)
        target=runtime.harmonic_response(train)*np.exp(2j*np.pi*train[:,None,None]*runtime.latency_samples/44100)
        c,d=fit_digital_state_readout(runtime,train,target,ridge=1e-12)
        runtime.set_state_readout(c,d)
        self.assertLess(np.linalg.norm(runtime.harmonic_response([0.])),1e-10)
        fine=runtime.harmonic_response([.01,.02])
        np.testing.assert_allclose(np.linalg.norm(fine[1])/np.linalg.norm(fine[0]),4.,rtol=.002)
        validation=np.array([23.,141.,267.,921.])
        original=self.runtime();original.mapping=np.eye(2)
        np.testing.assert_allclose(runtime.harmonic_response(validation),original.harmonic_response(validation),rtol=1e-3,atol=1e-5)

    def test_discrete_response_matches_time_domain_and_is_causal(self):
        runtime=self.runtime(rate=12000)
        hz=137.
        force=np.array([.3,-.4,.2])
        signal=[]
        for frame in range(16000):
            _,p=runtime.process(force*np.cos(2*np.pi*hz*frame/12000))
            signal.append(p)
        self.assertEqual(np.max(np.abs(signal[:24])),0.)
        times=np.arange(14000,16000)/12000
        design=np.column_stack([np.cos(2*np.pi*hz*times),-np.sin(2*np.pi*hz*times)])
        fit=np.linalg.lstsq(design,np.asarray(signal)[14000:],rcond=None)[0]
        measured=fit[0]+1j*fit[1]
        expected=runtime.harmonic_response([hz])[0] @ force
        np.testing.assert_allclose(measured,expected,rtol=1e-9,atol=1e-9)


if __name__=='__main__':
    unittest.main()
