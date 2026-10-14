import json
from pathlib import Path
import subprocess
import tempfile
import unittest
import numpy as np
from concert_grand_hybrid_midi import excerpt,voice_events,bridge_force
from concert_grand_native_strings import fixture,write_fixture
from concert_grand_realtime_pressure import PressureRuntime


class HybridMidiHelpers(unittest.TestCase):
    def test_window_preserves_original_events_and_labels_synthetic_closure(self):
        e=lambda tick,kind,key,value:dict(tick=tick,kind=kind,key=key,value=value,channel=0)
        original=[e(0,'cc',64,127),e(0,'cc',67,113),e(1000,'on',63,28),
            e(1200,'off',63,0),e(1500,'cc',64,80),e(1600,'on',63,33),e(4000,'off',63,0)]
        plan=dict(contact_rate_hz=1000,events=original)
        window=excerpt(plan,1.,.2)
        self.assertEqual(window['removed_cold_silence_s'],1.)
        self.assertEqual(window['original_events'],[dict(v,time_s=(v['tick']-1000)/1000) for v in original[2:-1]])
        self.assertEqual(window['synthetic_events'],[
            dict(time_s=1.,kind='off',key=63,value=0,channel=0,synthetic=True),
            dict(time_s=1.,kind='cc',key=64,value=0,channel=0,synthetic=True)])
        self.assertEqual(voice_events(window,63),[(0.,64,127),(0.,67,113),(0.,0,28),
            (.2,1,0),(.5,64,80),(.6,0,33),(1.,1,0),(1.,64,0)])

    def test_contact_resampling_rejects_ultrasonic_alias_and_keeps_filter_tails(self):
        with tempfile.TemporaryDirectory() as tmp:
            capture=128000;contact=512000;duration=.04;n=round(contact*duration)
            t=np.arange(n)/contact;signal=np.sin(2*np.pi*1000*t)+np.sin(2*np.pi*50000*t)
            path=Path(tmp)/'contact.f64';signal.astype('<f8').tofile(path)
            coast=np.zeros(round(capture*.08));start=round(capture*.02)
            result=bridge_force(coast,[dict(path=str(path),frames=n,start_capture_frame=start)],contact,capture)
            expected=np.sin(2*np.pi*1000*np.arange(len(result))/48000)
            section=slice(round(.025*48000),round(.055*48000))
            self.assertLess(np.linalg.norm(result[section]-expected[section])/np.linalg.norm(expected[section]),.003)
            self.assertGreater(np.linalg.norm(result[round(.0195*48000):round(.02*48000)]),.001)
            self.assertTrue(np.isfinite(result).all());self.assertEqual(len(result),3840)


class HybridMidiNative(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.folder=Path(cls.temp.name);cls.exe=cls.folder/'strings.exe'
        root=Path(__file__).resolve().parents[2]
        subprocess.run(['rustc','--edition=2021','-O','--cfg','concert_phase_library',
            str(root/'tools/concert-grand-phase-trajectory.rs'),'-o',str(cls.exe)],check=True)
        cls.body_exe=cls.folder/'body.exe'
        subprocess.run(['rustc','--edition=2021','-O',str(root/'tools/render-concert-grand-force-body.rs'),
            '-o',str(cls.body_exe)],check=True)
        cls.native_exe=cls.folder/'native.exe'
        subprocess.run(['rustc','--edition=2021','-O',str(root/'tools/concert-grand-native-strings.rs'),
            '-o',str(cls.native_exe)],check=True)

    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def render(self,events,soft=127,duration=.13,pedal=127,damper_scale=1.):
        folder=self.folder/str(len(list(self.folder.iterdir())));folder.mkdir()
        f=fixture(note=63,transverse=12,rate=32000,duration=duration);f['launch_speeds'][:]=0.
        write_fixture(folder/'f.bin',f)
        (folder/'events').write_text(f'0 64 {pedal}\n0 67 {soft}\n'+events,encoding='utf-8')
        np.full(12,.4,dtype='<f8').tofile(folder/'loss')
        data=json.loads(subprocess.check_output([str(self.exe),str(folder/'f.bin'),'midi-voice','8','16',
            str(folder/'events'),str(folder/'force'),str(folder/'loss'),f'--damper-scale={damper_scale}'],text=True))
        return data,np.fromfile(folder/'force',dtype='<f8')

    def test_delayed_key_strike_lifts_damper_before_string_vibrates(self):
        # Adding cold silence must not turn a held note into a damped tick.
        # At MIDI note-on the hammer is already striking, so key-driven
        # damper lift must be complete, independently of pedal position.
        a,fa=self.render('0 0 28\n.12 1 0\n',pedal=0,duration=.17)
        b,fb=self.render('.04 0 28\n.16 1 0\n',pedal=0,duration=.17)
        np.testing.assert_allclose(fb[5120:17920],fa[:12800],rtol=1e-12,atol=1e-14)
        self.assertEqual(a['launch_work_j'],b['launch_work_j'])

    def test_undamped_string_ignores_key_release_and_pedal_but_retains_decay(self):
        a,fa=self.render('.04 0 28\n.15 1 0\n',pedal=0,duration=.17,damper_scale=0.)
        b,fb=self.render('.04 0 28\n.06 1 0\n.08 64 127\n.1 64 0\n',
            pedal=0,duration=.17,damper_scale=0.)
        np.testing.assert_array_equal(fa,fb)
        self.assertGreater(np.linalg.norm(fb[-1280:]),.01)
        np.testing.assert_array_equal(a['q'],b['q'])

    def test_soft_pedal_leaves_unstruck_wire_alive_and_repeated_attack_keeps_history(self):
        first,a=self.render('0 0 28\n0.05 1 0\n')
        repeated,b=self.render('0 0 28\n0.05 1 0\n0.1 0 28\n0.12 1 0\n')
        self.assertEqual(repeated['attacks'],2)
        self.assertEqual(first['launch_work_j'][0],repeated['launch_work_j'][0])
        self.assertNotEqual(repeated['launch_work_j'][0],repeated['launch_work_j'][1])
        np.testing.assert_array_equal(a[:12800],b[:12800])
        np.testing.assert_array_equal(np.asarray(repeated['q'])[:48],0.)
        self.assertGreater(np.linalg.norm(np.asarray(repeated['q'])[48:96]),0.)
        self.assertGreater(repeated['felt_heat_j'],0.)
        self.assertEqual(len(b),16640)

    def test_body_file_sink_preserves_continuous_pressure_and_filter_history(self):
        runtime=PressureRuntime(np.array([400.,900.]),np.diag([4.,7.]),np.array([[1.,.2],[.3,.7]]),
            np.eye(2),np.array([[.001],[.002]]),np.array([[[.1,.2]],[[.3,-.1]]]),48000)
        coefficient=self.folder/'body.bin';runtime.write(coefficient)
        forces=np.zeros((1200,2));forces[0]=[1.,-.2];forces[87]=[.5,.1]
        input_path=self.folder/'body-input';forces.astype('<f8').tofile(input_path)
        out=self.folder/'pressure';data=json.loads(subprocess.check_output([str(self.body_exe),str(coefficient),
            str(input_path),'2',str(out)],text=True))
        expected=np.asarray([runtime.process(f)[1] for f in forces]);actual=np.fromfile(out,dtype='<f8').reshape(-1,2)
        np.testing.assert_allclose(actual,expected,rtol=2e-11,atol=1e-18)
        self.assertEqual(data['frames'],1200);self.assertFalse(data['body_feedback'])
        self.assertGreater(np.linalg.norm(actual[200:]),0.)

    def test_contact_bridge_force_includes_direct_felt_load_on_endpoint_lift(self):
        data,_=self.render('0 0 28\n0.009 1 0\n',soft=0,duration=.01)
        segment=data['contact_segments'][0];force=np.fromfile(segment['path'],dtype='<f8')[0]
        f=fixture(note=63,transverse=12,rate=512000,velocity=28,duration=1/512000)
        path=self.folder/'micro.bin';write_fixture(path,f)
        micro=json.loads(subprocess.check_output([str(self.native_exe),str(path),'verify'],text=True))
        # A second spatial AVF audit reads the same before/after positions.
        f['q'][-1]=0.;f['endpoint']=np.asarray(micro['q']);f['parameters'][9:11]=0.
        path2=self.folder/'kernel.bin';write_fixture(path2,f)
        kernel=json.loads(subprocess.check_output([str(self.native_exe),str(path2),'kernel'],text=True))
        nt=12;mass=f['parameters'][0]*f['parameters'][1]/2;n=np.arange(1,nt+1)
        cross=f['parameters'][0]*f['parameters'][1]*(-1.)**(n+1)/(np.pi*n*np.sqrt(mass))
        expected=.125*micro['force_trace'][0]-sum(kernel['boundary_gradient'])
        for i in range(3):expected-=cross @ np.asarray(micro['v'])[i*48+24:i*48+36]*512000
        self.assertAlmostEqual(force,expected,delta=max(abs(expected)*1e-9,1e-18))


if __name__=='__main__':unittest.main()
