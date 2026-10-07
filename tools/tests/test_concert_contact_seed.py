"""Stored physical endpoints must retain every state and authenticated binding."""
import sys
import json
import unittest
import tempfile
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from concert_grand_contact_seed import save_seed,load_seed,overlapping_intervals


class ContactSeedTests(unittest.TestCase):
    def fixture(self):
        state=tuple(np.arange(n,dtype=float).reshape(shape)+i/7 for i,(n,shape) in enumerate(
            [(9,(9,)),(9,(9,)),(60,(5,2,6)),(12,(2,6))]))
        return state,dict(contact_rate=3072000,source_sha256={'physical.py':'abc'},cache_R_sha256='def')

    def test_roundtrip_all_live_coordinates_and_forbid_overwrite(self):
        state,bindings=self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'seed.npz'
            restored,identity=save_seed(path,state,bindings)
            for actual,expected in zip(restored,state):np.testing.assert_array_equal(actual,expected)
            self.assertGreater(identity['payload_bytes'],sum(x.nbytes for x in state))
            restored[2][0,0,0]+=10
            again,_=load_seed(path,bindings)
            np.testing.assert_array_equal(again[2],state[2])
            with self.assertRaises(FileExistsError):save_seed(path,state,bindings)

    def test_changed_load_source_rate_or_payload_are_rejected(self):
        state,bindings=self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'seed.npz';save_seed(path,state,bindings)
            for key,value in [('contact_rate',1536000),('source_sha256',{'physical.py':'changed'}),('cache_R_sha256','changed')]:
                with self.assertRaisesRegex(ValueError,'mismatch'):load_seed(path,{**bindings,key:value})
            with path.open('ab') as stream:stream.write(b'changed')
            with self.assertRaisesRegex(ValueError,'mismatch'):load_seed(path,bindings)

    def test_dimension_metadata_and_partial_manifest_are_rejected(self):
        state,bindings=self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'seed.npz';save_seed(path,state,bindings)
            manifest=path.with_suffix('.json');metadata=json.loads(manifest.read_text())
            metadata['shapes'][2]=[60];manifest.write_text(json.dumps(metadata))
            with self.assertRaisesRegex(ValueError,'dimensions'):load_seed(path,bindings)
            path.unlink()
            with self.assertRaises(FileExistsError):save_seed(path,state,bindings)

    def test_invalid_state_does_not_create_files(self):
        state,bindings=self.fixture()
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'seed.npz'
            for bad in [state[:3],(*state[:3],np.array([np.nan])),(*state[:3],np.array([1],dtype=np.int64))]:
                with self.assertRaises(ValueError):save_seed(path,bad,bindings)
                self.assertFalse(path.exists());self.assertFalse(path.with_suffix('.json').exists())

    def test_independent_field_bounds_must_overlap_and_be_valid(self):
        a=dict(lower_j=1.,estimate_j=1.5,upper_j=2.)
        self.assertTrue(overlapping_intervals(a,dict(lower_j=1.9,estimate_j=2.,upper_j=2.1)))
        self.assertFalse(overlapping_intervals(a,dict(lower_j=2.01,estimate_j=2.02,upper_j=2.03)))
        with self.assertRaises(ValueError):overlapping_intervals(a,dict(lower_j=2.,estimate_j=1.,upper_j=3.))


if __name__=='__main__':unittest.main()
