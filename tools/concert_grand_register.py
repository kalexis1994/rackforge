"""Explicit small-register construction with the frozen prototype wire law.

All keys retain their own wires and heads on one reciprocal board. This is
not identified piano scaling: wound bass, string counts and key-dependent
felt/action remain pending. A dense-matrix guard prevents accidental loading
of an entire MIDI keyboard with the current constructor.
"""
import numpy as np
import concert_grand_orthotropic_plate as plate
from concert_grand_shared_strings import WirePort, SharedStrings, SharedHammers
from concert_grand_nonlinear_string import Wire
from concert_grand_coupled_contact import Felt


def make_register(modal, geometry, keys, transverse=96, choir=3,
                  max_single_matrix_bytes=512 * 2**20):
    keys = list(keys)
    if (not keys or keys != sorted(set(keys))
            or any(not isinstance(k, int) or not 21 <= k <= 108 for k in keys)
            or choir not in (1, 3) or not isinstance(transverse, int) or transverse <= 0):
        raise ValueError('ordered distinct piano keys and positive modal count')
    coordinates = len(keys) * choir * 4 * transverse + len(modal.frequency)
    required = coordinates**2 * 8
    if required > max_single_matrix_bytes:
        raise ValueError(f'one dense core matrix requires {required} bytes; use a smaller register or structured mechanics')
    xs = geometry['panel']['length'] * (.1 + .8 * (np.array(keys) - 21) / 87)
    phi = plate.shape(geometry, xs, np.full(len(keys), geometry['panel']['width'] / 2)).T
    cents = [-.4, 0., .6] if choir == 3 else [0.]
    ports = [WirePort(key, Wire(.62 * 2**((60 - key) / 12), .0081, 850. * 2**(offset / 600)), phi[:, index])
             for index, key in enumerate(keys) for offset in cents]
    core = SharedStrings(ports, modal.frequency, modal.damping, transverse, 2 * transverse)
    system = SharedHammers(core, Felt(.002, 7.5e8, 2.6, .125, .02, .1))
    metadata = dict(keys=keys, wires_per_key=choir, wire_tuning_offsets_cents=cents,
                    mechanical_coordinates=system.size, one_dense_core_matrix_bytes=required,
                    matrix_budget_scope='One matrix, not peak RAM; several matrices/factors are constructed.',
                    wire_lengths_m={str(key): .62 * 2**((60 - key) / 12) for key in keys},
                    parameters_identified=False, same_assumed_wire_and_felt_law=True)
    return system, metadata
