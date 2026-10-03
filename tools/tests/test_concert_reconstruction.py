"""Causal reconstruction checked against direct convolution and known filters."""
import importlib.util
from pathlib import Path
import sys
import unittest

import numpy as np

path=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(path))
spec=importlib.util.spec_from_file_location('causal_control',path/'reconstruct-concert-grand-current.py')
CONTROL=importlib.util.module_from_spec(spec);spec.loader.exec_module(CONTROL)


class CausalReconstruction(unittest.TestCase):
    def test_masked_operator_matches_direct_convolution_and_adjoint(self):
        rng=np.random.default_rng(812)
        x=rng.normal(size=(3,127,2));h=rng.normal(size=(2,2,19));w=np.array([.7,2,1])
        op=CONTROL.CausalOperator(x,19,w)
        expected=np.zeros((3,2,127))
        for m in range(3):
            for side in range(2):
                for r in range(2):expected[m,side]+=np.convolve(x[m,:,r],h[r,side])[:127]
        np.testing.assert_allclose(op@h.ravel(),(expected*w[:,None,None]).ravel(),atol=1e-12)
        residual=rng.normal(size=op.shape[0])
        self.assertAlmostEqual(float(np.dot(op@h.ravel(),residual)),float(np.dot(h.ravel(),op.rmatvec(residual))),places=10)

    def test_recover_known_delayed_stereo_filters_on_unseen_excitation(self):
        rng=np.random.default_rng(34)
        kernel=np.zeros((2,2,13));kernel[0,0,3]=.7;kernel[0,1,5]=-.4
        kernel[1,0,7]=.3;kernel[1,1,2]=.6
        def target(x):
            return np.stack([sum(np.convolve(x[:,r],kernel[r,c])[:len(x)] for r in range(2)) for c in range(2)],axis=1)
        x=rng.normal(size=(4,512,2));y=np.stack([target(z) for z in x])
        found,solver=CONTROL.fit(x,y,13,100)
        np.testing.assert_allclose(found,kernel,atol=1e-6)
        reserved=rng.normal(size=(512,2))
        prediction=np.stack([sum(np.convolve(reserved[:,r],found[r,c])[:512] for r in range(2)) for c in range(2)],axis=1)
        self.assertLess(CONTROL.error_db(target(reserved),prediction),-100)


if __name__=='__main__':unittest.main()
