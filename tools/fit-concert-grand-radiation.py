"""Generate reproducible joint mechanical/acoustic calibration experiments.

The native engine, not an audio equalizer, evaluates these profiles. All modes
retain positive masses and the same surface in bridge loading and radiation.
The reduced mode-shape mixture is empirical; this does not identify a Steinway
soundboard or microphone position from its edited recordings.
"""
import argparse
import json
from pathlib import Path

import numpy as np
from scipy.stats import qmc


def generate(count, center=None, fixed_density=None, room_geometry=False, shared_fir=False, quadrature_scale=1):
    points = qmc.Sobol(9, scramble=True, seed=20261002).random_base2(int(np.log2(count)))
    profiles = {
        'causal-sin-refit': {'tuning':{'CAUSAL_RADIATION_FIR':1}, 'params':{}},
    }
    for i, (height, shape, density, loss, mobility, bass, treble, extra, pressure) in enumerate(points):
        distance = .6*(3.2/.6)**height
        mean = .00015*(.00065/.00015)**mobility
        # Approximate level compensation is ONE instrument sensitivity. It is
        # evaluated as part of the engine, with its auxiliary paths unchanged.
        sensitivity = np.clip(3.*(.0005/mean)*(distance/2.64)*10**((-3.+6.*pressure)/20), .125, 16.)
        profiles[f'joint-radiation-{i:03}'] = {
            'params':{'25':float(distance), '31':float(3.+5.*loss), '32':float(.9+.28*density)},
            'tuning':{
                'CAUSAL_RADIATION_FIR':1,
                'BOARD_SHAPE':float(.7+.3*shape),
                'PHYSICAL_BRIDGE_MOBILITY':float(mean),
                'PHYSICAL_BRIDGE_BASS_RATIO':float(.8*(1.6/.8)**bass),
                'PHYSICAL_BRIDGE_TREBLE_RATIO':float(.5*(2./.5)**treble),
                'UNISON_EXTRA_LOSS':float(1.+2.5*extra),
                'PHYSICAL_PRESSURE_SCALE':float(sensitivity),
            },
        }
        if center is not None:
            t, c = center['tuning'], center['params']
            distance = np.clip(float(c['25'])*2**(.35*(2*height-1)), .5, 16.)
            mean = t['PHYSICAL_BRIDGE_MOBILITY']*2**(.5*(2*mobility-1))
            sensitivity = np.clip(t['PHYSICAL_PRESSURE_SCALE']*(distance/float(c['25']))
                                  *t['PHYSICAL_BRIDGE_MOBILITY']/mean
                                  *10**((-1.5+3.*pressure)/20), .125, 16.)
            profiles[f'joint-radiation-{i:03}'] = {
                'params':{'25':float(distance), '31':float(c['31']+1.5*(2*loss-1)),
                          '32':float(c['32'])},
                'tuning':{**t, 'CAUSAL_RADIATION_FIR':1,
                    'BOARD_SHAPE':float(np.clip(t['BOARD_SHAPE']+.12*(2*shape-1), .5, 1.)),
                    'PHYSICAL_BRIDGE_MOBILITY':float(mean),
                    'PHYSICAL_BRIDGE_BASS_RATIO':float(t['PHYSICAL_BRIDGE_BASS_RATIO']*2**(.5*(2*bass-1))),
                    'PHYSICAL_BRIDGE_TREBLE_RATIO':float(t['PHYSICAL_BRIDGE_TREBLE_RATIO']*2**(.5*(2*treble-1))),
                    'UNISON_EXTRA_LOSS':float(max(.1,t['UNISON_EXTRA_LOSS']+.75*(2*extra-1))),
                    'PHYSICAL_PRESSURE_SCALE':float(sensitivity)},
            }
    if center is not None:
        profiles['center'] = json.loads(json.dumps(center))
    if fixed_density is not None:
        for profile in profiles.values():
            profile['params']['32'] = fixed_density
    if room_geometry:
        for profile in profiles.values():
            profile['tuning']['BOARD_ROOM_GEOMETRY'] = 1
    for profile in profiles.values():
        if shared_fir: profile['tuning']['SHARED_RADIATION_FIR'] = 1
        if quadrature_scale != 1: profile['tuning']['RADIATION_QUADRATURE_SCALE'] = quadrature_scale
    return profiles


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--count', type=int, choices=(16,32,64), default=32)
    p.add_argument('--center', type=Path)
    p.add_argument('--fixed-density', type=float)
    p.add_argument('--room-geometry', action='store_true')
    p.add_argument('--shared-fir', action='store_true')
    p.add_argument('--quadrature-scale', type=int, choices=(1,2,3,4), default=1)
    a = p.parse_args()
    a.output.parent.mkdir(parents=True, exist_ok=True)
    if a.fixed_density is not None:
        assert .633 <= a.fixed_density <= 1.581
    center = json.loads(a.center.read_text()) if a.center else None
    a.output.write_text(json.dumps(generate(a.count, center, a.fixed_density, a.room_geometry, a.shared_fir, a.quadrature_scale), indent=2), encoding='utf-8')
    print(f'{a.count} Sobol profiles plus uncalibrated causal control: {a.output}')


if __name__ == '__main__':
    main()
