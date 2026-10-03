"""Relative correction must preserve the baseline and its independent branch."""
import importlib.util
from pathlib import Path
import sys
import unittest

import numpy as np

path=Path(__file__).resolve().parents[1];sys.path.insert(0,str(path))
spec=importlib.util.spec_from_file_location('relative_test',path/'fit-concert-grand-causal-observation.py')
REL=importlib.util.module_from_spec(spec);spec.loader.exec_module(REL)


class RelativeObservation(unittest.TestCase):
    def test_identity_preserves_delayed_observation_and_auxiliary(self):
        rng=np.random.default_rng(411)
        regional=rng.normal(size=(3,1024,2));regional[:,:17]=0
        auxiliary=rng.normal(size=(1024,2));freq=np.geomspace(35,12000,64)
        kernel,tail=REL.relative_kernel(np.zeros((3,64,2)),freq)
        np.testing.assert_allclose(REL.corrected(regional,auxiliary,kernel),regional.sum(axis=0)+auxiliary,atol=1e-12)
        self.assertTrue(all(t<-150 for t in tail))

    def test_constant_gain_changes_only_observation_branch(self):
        rng=np.random.default_rng(981)
        regional=rng.normal(size=(3,1024,2));auxiliary=rng.normal(size=(1024,2))
        kernel,_=REL.relative_kernel(np.full((3,64,2),20*np.log10(2)),np.geomspace(35,12000,64))
        np.testing.assert_allclose(REL.corrected(regional,auxiliary,kernel),2*regional.sum(axis=0)+auxiliary,atol=1e-12)


if __name__=='__main__':unittest.main()
