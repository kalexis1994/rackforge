import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from concert_grand_local_hann_controls import maximum_nested, local_controls


class LocalControlTests(unittest.TestCase):
    def test_nested_lists_and_nonfinite(self):
        self.assertEqual(maximum_nested(dict(v=[1e-12, 2e-12], a=[[3e-12], [9e-11]])), 9e-11)
        with self.assertRaises(ValueError):
            maximum_nested([float('nan')])

    def test_real_checkpoint_preserves_mesh_failure(self):
        import json
        path = Path(__file__).resolve().parents[2]/'docs/concert-grand-physics-roadmap-2026-10-02/medidas-P6-contacto-banda-superior/Hann-local-malla.json'
        r = json.loads(path.read_text(encoding='utf-8'))
        gates = local_controls(r, True)
        self.assertTrue(gates['fine_local_oracle'])
        self.assertFalse(gates['fine_load_cells'])
        self.assertEqual(sum(gates.values()), 8)
        r['load_probes'].pop()
        with self.assertRaises(ValueError):
            local_controls(r, True)


if __name__ == '__main__':
    unittest.main()
