"""Cached endpoints require immutable physical sources and historical code proof."""
import sys
import json
import hashlib
import tempfile
import unittest
import importlib.util
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_contact_seed import save_seed
from concert_grand_wave_cache import digest
from concert_grand_seed_resume import authenticated_seed,PRODUCER
from concert_grand_bounded_observer import energy_interval
spec=importlib.util.spec_from_file_location('coast512',Path(__file__).resolve().parents[1]/PRODUCER)
audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)


class SeedResumeTests(unittest.TestCase):
    def fixture(self,root):
        blob=b'original producer\n';(root/PRODUCER).write_bytes(b'updated observer wrapper\n')
        physical=root/'physical.py';physical.write_bytes(b'unchanged equations\n')
        parent=root/'parent.json';parent.write_text('{}')
        source={PRODUCER:hashlib.sha256(blob).hexdigest(),'physical.py':digest(physical)}
        old=dict(stage='independent complete angular certificate',all_gates_passed=False,
            rates=[128000,256000,512000],cases=[{'rate':r} for r in audit.RATES],
            contact_rate=3072000,duration_s=.0005,board_sha256='board',cache_R_sha256='R',
            geometry={'note':60},microphones=[[1.,2.,3.]],parent_sha256={str(parent):digest(parent)},
            source_sha256=source,criteria={'seed_relative':1e-10},
            baseline_reproduction={'contact':{'energy':0.}},coast_reproduction={'256000':{'coast':{'energy':0.}}})
        state=tuple(np.ones(shape,dtype=float)*(i+1) for i,shape in enumerate([(9,),(9,),(5,2,6),(2,6)]))
        bindings=dict(source_sha256=source,parent_sha256=old['parent_sha256'],board_sha256='board',
            cache_R_sha256='R',geometry=old['geometry'],contact_rate=3072000,contact_duration_s=.003,incident_speed_m_s=5.)
        seed=root/'seed.npz';_,identity=save_seed(seed,state,bindings)
        old['contact_seed']=dict(path=str(seed),roundtrip_exact=True,**identity)
        report=root/'checkpoint.json';report.write_text(json.dumps(old))
        return state,seed,report,old,blob

    def test_historical_producer_can_resume_but_only_same_physics(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);state,seed,report,old,blob=self.fixture(root)
            with patch('concert_grand_seed_resume.subprocess.run',return_value=SimpleNamespace(stdout=blob)):
                loaded,_,proof=authenticated_seed(report,seed,'a'*40,old,root)
                for a,b in zip(loaded,state):np.testing.assert_array_equal(a,b)
                self.assertEqual(proof['checkpoint_sha256'],digest(report))
                (root/'physical.py').write_text('changed dynamics')
                with self.assertRaisesRegex(ValueError,'source'):authenticated_seed(report,seed,'a'*40,old,root)

    def test_wrong_commit_code_load_or_parent_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);_,seed,report,old,blob=self.fixture(root)
            with patch('concert_grand_seed_resume.subprocess.run',return_value=SimpleNamespace(stdout=b'other code')):
                with self.assertRaisesRegex(ValueError,'producer'):authenticated_seed(report,seed,'a'*40,old,root)
            with patch('concert_grand_seed_resume.subprocess.run',return_value=SimpleNamespace(stdout=blob)):
                with self.assertRaisesRegex(ValueError,'binding'):authenticated_seed(report,seed,'a'*40,{**old,'cache_R_sha256':'other'},root)
                Path(next(iter(old['parent_sha256']))).write_text('changed report')
                with self.assertRaisesRegex(ValueError,'parent'):authenticated_seed(report,seed,'a'*40,old,root)

    def test_whole_angular_overlap_map_is_json_serializable_with_numpy_intervals(self):
        row=dict(rate=256000,**{key:energy_interval(1.,1e-9) for key in
            ['wave_initial_interval','wave_final_interval','wave_radiation_interval']})
        old=json.loads(json.dumps(dict(cases=[{**row,**{key:energy_interval(1.,2e-9) for key in row if key!='rate'}}])))
        result=audit.prior_field_overlap([row],old)
        self.assertTrue(all(type(v) is bool for v in result['256000'].values()))
        self.assertTrue(all(json.loads(json.dumps(result))['256000'].values()))
        row['wave_radiation_interval']=energy_interval(2.,1e-9)
        self.assertFalse(audit.prior_field_overlap([row],old)['256000']['wave_radiation_interval'])


if __name__=='__main__':unittest.main()
