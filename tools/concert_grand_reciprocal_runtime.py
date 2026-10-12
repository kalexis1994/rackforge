"""Symmetric residue normalization: one spatial row per reciprocal pole."""
from pathlib import Path
import struct
import numpy as np
from concert_grand_modal_runtime import ModalRuntime


class ReciprocalRuntime:
    def __init__(self, modal):
        norm = np.sum(abs(modal.drive)**2, axis=1)
        alpha = np.ones(len(norm), dtype=complex)
        active = norm > 1e-30
        alpha[active] = np.sum(modal.drive[active].conj()*modal.output[active], axis=1)/norm[active]
        defect = np.linalg.norm(modal.output-alpha[:, None]*modal.drive, axis=1)
        weight = np.linalg.norm(modal.drive, axis=1)
        self.reciprocity_defect_relative = float(np.sum(defect*weight)
            /max(np.sum(np.linalg.norm(modal.output, axis=1)*weight), 1e-30))
        if self.reciprocity_defect_relative > 1e-10:
            raise ValueError('modal residues not reciprocal')
        self.scale = np.sqrt(alpha)
        if np.any(abs(self.scale) < 1e-30):
            raise ValueError('singular reciprocal normalization')
        self.reference = modal
        self.modal = ModalRuntime(modal.roots, modal.vectors/self.scale,
            modal.drive*self.scale[:, None], modal.drive*self.scale[:, None],
            modal.multiplicity, modal.condition, modal.eigen_residual)

    def write(self, path, rate):
        step = self.modal.coefficients(rate)
        n, p = step.drive.shape
        values = np.column_stack([part for value in [step.transition, step.average_state,
                        step.drive_step, step.drive_average] for part in [value.real, value.imag]])
        with Path(path).open('xb') as stream:
            stream.write(struct.pack('<8sIId', b'RFRCM001', n, p, step.dt))
            for value in [values, step.drive.real, step.drive.imag, self.modal.multiplicity]:
                stream.write(np.asarray(value, dtype='<f8').tobytes())
        return step
