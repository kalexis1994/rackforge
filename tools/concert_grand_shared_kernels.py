"""Exact algebraic acceleration of the frozen shared-phrase equations.

Batch equal cosine grids across wires; keep every coordinate and acoustic
state. Diagonal carrier blocks use permutations instead of dense products.
The original classes stay available as trajectory references.
"""
import numpy as np
from scipy.fft import dct, idct
from concert_grand_shared_strings import SharedStrings, SharedCoast


class BatchedStrings(SharedStrings):
    def __init__(self, reference):
        # Immutable mechanical matrices are shared, never copied to disk.
        self.__dict__ = reference.__dict__.copy()
        entries = self.entries
        base = entries[0]['base']
        if any((e['base'].nt, e['base'].nl, e['base'].nodes) !=
               (base.nt, base.nl, base.nodes) for e in entries):
            raise ValueError('batch requires equal modal counts/cosine grids')
        self.u = np.array([np.arange(e['su'].start, e['su'].stop) for e in entries])
        self.v = np.array([np.arange(e['sv'].start, e['sv'].stop) for e in entries])
        self.w = np.array([np.arange(e['sw'].start, e['sw'].stop) for e in entries])
        self.ports_matrix = np.array([e['phi'] for e in entries])
        self.cross_matrix = np.array([e['cross'] for e in entries])
        self.ku_matrix = np.array([e['base'].ku for e in entries])
        self.kv_matrix = np.array([e['base'].kv for e in entries])
        self.lengths = np.array([e['port'].wire.length for e in entries])
        self.ea = np.array([e['port'].wire.axial_rigidity for e in entries])
        self.weights = np.array([e['base'].weight for e in entries])
        self.nodes = base.nodes

    def physical(self, q):
        b = self.board_transform @ q[self.sb]
        result = q.copy()
        result[self.sb] = b
        result[self.v] -= self.cross_matrix * (self.ports_matrix @ b)[:, None]
        return result

    def pull_gradient(self, g):
        result = g.copy()
        board = g[self.sb] - self.ports_matrix.T @ np.sum(self.cross_matrix*g[self.v], axis=1)
        result[self.sb] = self.board_transform.T @ board
        return result

    def nonlinear_avf(self, a, b):
        if not self.nonlinear:
            return np.zeros_like(a)
        z = np.array([self.physical(a), self.physical(b)])
        packed = np.zeros((2, 3, len(self.entries), self.nodes))
        packed[:, 0, :, 1:self.ku_matrix.shape[1]+1] = z[:, self.u]*self.ku_matrix*self.nodes
        packed[:, 1, :, 1:self.kv_matrix.shape[1]+1] = z[:, self.v]*self.kv_matrix*self.nodes
        packed[:, 2, :, 1:self.kv_matrix.shape[1]+1] = z[:, self.w]*self.kv_matrix*self.nodes
        fields = idct(packed, type=2, axis=-1)
        fields[:, 1] += ((z[:, self.sb] @ self.ports_matrix.T)/self.lengths)[:, :, None]
        u0, v0, w0 = fields[0]
        u1, v1, w1 = fields[1]
        product = lambda x0,x1,y0,y1: (2*x0*y0+x0*y1+x1*y0+2*x1*y1)/6
        square_product = lambda x0,x1,y0,y1: (x0*x0*(3*y0+y1)+2*x0*x1*(y0+y1)+x1*x1*(y0+3*y1))/12
        eta = (v0*v0+v0*v1+v1*v1+w0*w0+w0*w1+w1*w1)/6
        pv = product(u0,u1,v0,v1)+(v0+v1)*(v0*v0+v1*v1)/8+square_product(w0,w1,v0,v1)/2
        pw = product(u0,u1,w0,w1)+(w0+w1)*(w0*w0+w1*w1)/8+square_product(v0,v1,w0,w1)/2
        spectral = dct(np.array([eta,pv,pw]), type=2, axis=-1)
        factor = (self.ea*self.lengths/(2*self.nodes))[:, None]
        g = np.zeros_like(a)
        g[self.u] = factor*self.ku_matrix*spectral[0, :, 1:self.ku_matrix.shape[1]+1]
        g[self.v] = factor*self.kv_matrix*spectral[1, :, 1:self.kv_matrix.shape[1]+1]
        g[self.w] = factor*self.kv_matrix*spectral[2, :, 1:self.kv_matrix.shape[1]+1]
        g[self.sb] = self.ports_matrix.T @ (self.ea/self.lengths*np.sum(self.weights*pv, axis=1))
        return self.pull_gradient(g)


class BatchedCoast(SharedCoast):
    def __init__(self, system, air):
        super().__init__(system, air)
        self.permutations = {}
        for index, (indices, _, basis) in enumerate(self.blocks):
            if np.all(indices < system.string.ns):
                permutation = np.argmax(abs(basis), axis=0)
                signs = basis[permutation, np.arange(len(indices))]
                expected = np.zeros_like(basis)
                expected[permutation, np.arange(len(indices))] = signs
                if not np.array_equal(expected, basis) or not np.all(abs(signs) == 1):
                    raise ValueError('expected diagonal carrier eigenbasis')
                self.permutations[index] = (permutation, signs)

    def project(self, vector):
        result = np.empty_like(vector)
        for index, (indices, _, basis) in enumerate(self.blocks):
            values = vector[indices]
            if index in self.permutations:
                permutation, signs = self.permutations[index]
                result[indices] = values[permutation]*signs
            else:
                result[indices] = basis.T @ values
        return result

    def unproject(self, vector):
        result = np.empty_like(vector)
        for index, (indices, _, basis) in enumerate(self.blocks):
            if index in self.permutations:
                permutation, signs = self.permutations[index]
                result[indices[permutation]] = vector[indices]*signs
            else:
                result[indices] = basis @ vector[indices]
        return result
