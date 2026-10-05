"""Same real Ritz map without allocating a complex copy for each product.

Separate real/imaginary products are algebraically identical. This bounds
temporary memory for the 7392-coordinate reference, without changing modes.
"""
import numpy as np
from concert_grand_matrixfree_air import RitzSurface


class RealMatrix:
    def __init__(self,matrix):
        self.matrix=np.asarray(matrix)
        if not np.issubdtype(self.matrix.dtype,np.floating):raise ValueError('real map required')

    def __matmul__(self,values):
        x=np.asarray(values)
        if np.iscomplexobj(x):return self.matrix@x.real+1j*(self.matrix@x.imag)
        return self.matrix@x

    @property
    def T(self):return RealMatrix(self.matrix.T)

    def __len__(self):return len(self.matrix)


class RealArithmeticSurface(RitzSurface):
    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs);self.vectors=RealMatrix(self.vectors)
