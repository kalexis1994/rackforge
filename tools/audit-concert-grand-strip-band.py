"""Use the frozen wide-band verifier for the finite-width physical control.

The monkey-patched solver is scoped to this independent command process;
the original delta-line module/file and its measurements stay unchanged.
"""
import json,sys
from pathlib import Path
from importlib.util import spec_from_file_location,module_from_spec
import concert_grand_finite_strip_plate as strip

def main():
    spec=spec_from_file_location('strip_band_verifier',Path(__file__).with_name('audit-concert-grand-high-band-board.py'))
    verifier=module_from_spec(spec);spec.loader.exec_module(verifier)
    verifier.plate.solve=strip.solve
    verifier.main()
    report=Path(sys.argv[sys.argv.index('--report')+1]);r=json.loads(report.read_text())
    r['physical_idealization']='Finite-width rib/straight-bridge strips, same integrated line mass and EI; distinct from delta-line control.'
    r['source_sha256'].update({name:verifier.digest(Path(__file__).with_name(name)) for name in ['audit-concert-grand-strip-band.py','concert_grand_finite_strip_plate.py']})
    verifier.write(report,r)

if __name__=='__main__':main()
