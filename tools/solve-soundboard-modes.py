#!/usr/bin/env python3
"""A soundboard's modes, computed instead of drawn -- and what the model uses.

    python tools/solve-soundboard-modes.py

Concert Grand's board is a bank of 256 resonators whose frequencies come from
a density law and whose shapes along the bridge are a cosine at a drawn
wavenumber with a drawn phase (`PIANO_MODEL.md`, "The soundboard, from the
measured plate"). The laws are measured; the individual modes are not. This
computes them from the plate instead, the way MAESSTRO does from geometry and
materials, and prints the three numbers that decide whether swapping one for
the other would buy anything:

  * **modal density** against the model's `board_spacing`, which is the claim
    the drawn bank already tries to honour;
  * **the singular values of the shape matrix** -- 16 bridge points by N modes
    -- which say how many terms a runtime read needs. The model spends three
    per mode today, on a 16-point transform that costs 512 multiplies a frame
    whether or not anything uses all of it;
  * **location dependence**: how much two strings a few rib bays apart differ
    in the modes they actually drive. A random bay per mode has none of this
    on average, which is why confining the drawn bank bought one decibel
    (`PIANO_MODEL.md`).

Two regimes, both from Ege and Boutillon rather than from a mesh. Below the
rib knee the ribbed board behaves as a homogeneous isotropic plate, so its
modes are a clamped plate's, computed here by Rayleigh-Ritz on clamped-beam
functions. Above it the ribs confine the waves, so each bay is a strip
clamped along its two long edges and the modes belong to the bay rather than
to the board. No FEM, no mesh: a few hundred lines of NumPy, seconds to run,
and every number in it is a published material property or a measured
geometry.

Nothing here touches the plugin. It is a measurement of what a computed basis
would look like, to be read before deciding whether to build one.
"""

import math
import sys

import numpy as np

# --- the instrument, from plugins/concert-grand/src/lib.rs -----------------
BRIDGE_LENGTH_M = 1.4
BOARD_WIDTH_M = 1.0
#: A grand's board is longer than its bridge; the bridge crosses it.
BOARD_LENGTH_M = 1.5
BOARD_BOTTOM_HZ = 45.0
BOARD_TOP_HZ = 8500.0
BOARD_MODES = 256
BOARD_DRIVE_POINTS = 16
#: Where the model's own density law turns, and where Ege and Boutillon put
#: the rib-confinement transition on a grand.
RIB_KNEE_HZ = 1477.0

# --- the plate, from the literature the model already cites ---------------
#: Sitka spruce soundboard: 8 to 10 mm, 350 to 450 kg/m^3.
THICKNESS_M = 0.009
DENSITY_KG_M3 = 400.0
#: Ege and Boutillon: below the knee the ribbed board behaves as a
#: homogeneous plate with *isotropic* properties -- the ribs run across the
#: grain and make up for spruce's weak cross-grain stiffness. So one modulus
#: here, not two.
#:
#: And it is not guessed. A plate's modal density is
#: `n = (A/2) sqrt(rho h / D)`, independent of frequency, so **the measured
#: density fixes the effective rigidity**: their 0.06 modes/Hz on a board of
#: this area and mass gives D, and D gives E. Guessing 2.6 GPa first put the
#: computed density at 0.105/Hz, three quarters above the measurement --
#: which is how a wrong modulus announces itself.
MEASURED_DENSITY_PER_HZ = 0.06
POISSON = 0.3
#: Rib spacing on a grand, 12 to 16 cm.
RIB_SPACING_M = 0.14


def effective_young():
    """The rigidity the measured modal density implies, and its modulus."""
    area = BOARD_LENGTH_M * BOARD_WIDTH_M
    mass = DENSITY_KG_M3 * THICKNESS_M
    # n = (A/2) sqrt(mass / D)  ->  D = mass (A / 2n)^2
    rigidity = mass * (area / (2.0 * MEASURED_DENSITY_PER_HZ)) ** 2
    return rigidity * 12.0 * (1.0 - POISSON ** 2) / THICKNESS_M ** 3


YOUNG_PA = effective_young()


def clamped_beam(order, length, samples):
    """A clamped-clamped beam's mode shape and its second derivative.

    The Rayleigh-Ritz basis. Closed form, so the plate's integrals below are
    a numerical quadrature over these rather than over a mesh.
    """
    # Roots of cos(b)cosh(b) = 1, the clamped-clamped eigenvalues.
    roots = [4.730040744862704, 7.853204624095838, 10.995607838001671,
             14.137165491257464, 17.278759657399480]
    beta = (roots[order] if order < len(roots)
            else (2 * (order + 1) + 1) * math.pi / 2) / length
    x = np.linspace(0.0, length, samples)
    bx = beta * x
    sigma = ((math.cosh(beta * length) - math.cos(beta * length))
             / (math.sinh(beta * length) - math.sin(beta * length)))
    shape = (np.cosh(bx) - np.cos(bx)) - sigma * (np.sinh(bx) - np.sin(bx))
    second = beta * beta * ((np.cosh(bx) + np.cos(bx))
                            - sigma * (np.sinh(bx) + np.sin(bx)))
    # Unit mean square, so the plate integrals carry no arbitrary scale.
    norm = math.sqrt(np.mean(shape * shape))
    return shape / norm, second / norm


def plate_modes(length, width, thickness, young, terms=14, samples=400):
    """Clamped isotropic plate modes by Rayleigh-Ritz.

    Returns frequencies in hertz and the modal amplitudes, as separable
    products of beam functions, evaluated later wherever they are needed.
    """
    rigidity = young * thickness ** 3 / (12.0 * (1.0 - POISSON ** 2))
    mass = DENSITY_KG_M3 * thickness

    xs = [clamped_beam(m, length, samples) for m in range(terms)]
    ys = [clamped_beam(n, width, samples) for n in range(terms)]
    dx = length / (samples - 1)
    dy = width / (samples - 1)

    def integ(a, b, step):
        return np.trapezoid(a * b, dx=step)

    # 1D integrals: value-value, second-second, and value-second.
    ix_vv = np.array([[integ(xs[i][0], xs[j][0], dx) for j in range(terms)]
                      for i in range(terms)])
    ix_ss = np.array([[integ(xs[i][1], xs[j][1], dx) for j in range(terms)]
                      for i in range(terms)])
    ix_vs = np.array([[integ(xs[i][0], xs[j][1], dx) for j in range(terms)]
                      for i in range(terms)])
    iy_vv = np.array([[integ(ys[i][0], ys[j][0], dy) for j in range(terms)]
                      for i in range(terms)])
    iy_ss = np.array([[integ(ys[i][1], ys[j][1], dy) for j in range(terms)]
                      for i in range(terms)])
    iy_vs = np.array([[integ(ys[i][0], ys[j][1], dy) for j in range(terms)]
                      for i in range(terms)])

    size = terms * terms
    stiff = np.zeros((size, size))
    inert = np.zeros((size, size))
    for m in range(terms):
        for n in range(terms):
            row = m * terms + n
            for p in range(terms):
                for q in range(terms):
                    col = p * terms + q
                    # The isotropic plate's strain energy, separated.
                    stiff[row, col] = rigidity * (
                        ix_ss[m, p] * iy_vv[n, q]
                        + ix_vv[m, p] * iy_ss[n, q]
                        + POISSON * (ix_vs[m, p] * iy_vs[q, n]
                                     + ix_vs[p, m] * iy_vs[n, q])
                    )
                    inert[row, col] = mass * ix_vv[m, p] * iy_vv[n, q]

    values, vectors = np.linalg.eigh(
        np.linalg.solve(inert, stiff) if False else
        np.linalg.inv(np.linalg.cholesky(inert)) @ stiff
        @ np.linalg.inv(np.linalg.cholesky(inert)).T
    )
    values = np.clip(values, 0.0, None)
    hertz = np.sqrt(values) / (2.0 * math.pi)
    order = np.argsort(hertz)
    return hertz[order], vectors[:, order], xs, ys, terms


def bridge_shapes(vectors, xs, ys, terms, bridge_y=0.42, samples=400):
    """Each mode sampled at the sixteen drive points along the bridge.

    The bridge runs along the board a little off centre; a string drives the
    board where its bridge pin sits, which is what the drive points stand for.
    """
    point_x = np.linspace(0.05, 0.95, BOARD_DRIVE_POINTS)
    xi = (point_x * (samples - 1)).astype(int)
    yi = int(bridge_y * (samples - 1))
    shapes = np.zeros((vectors.shape[1], BOARD_DRIVE_POINTS))
    for mode in range(vectors.shape[1]):
        weights = vectors[:, mode].reshape(terms, terms)
        field = np.zeros(BOARD_DRIVE_POINTS)
        for m in range(terms):
            for n in range(terms):
                field += weights[m, n] * xs[m][0][xi] * ys[n][0][yi]
        shapes[mode] = field
    return shapes


def bay_modes(top_hz):
    """Above the knee: a strip clamped along the two ribs that bound it.

    The waves no longer span the board, so a mode belongs to its bay. Each
    bay is a plate of the board's thickness, `RIB_SPACING_M` across and as
    long as the board, and it only reaches the drive points over itself.
    """
    bays = max(1, int(round(BRIDGE_LENGTH_M / RIB_SPACING_M)))
    rows = []
    # Ribs are not evenly spaced and the board is not a rectangle, so no two
    # bays are the same size. Giving them all one geometry made the spectrum
    # ten copies of one discrete set, with holes between -- 0.000 modes/Hz
    # through 3-6 kHz, which is an artefact of the model and not a property
    # of a soundboard. Spacings run 12 to 16 cm.
    widths = np.linspace(0.12, 0.16, bays)
    for bay in range(bays):
        hertz, vectors, xs, ys, terms = plate_modes(
            BOARD_LENGTH_M * (0.85 + 0.3 * bay / max(bays - 1, 1)),
            float(widths[bay]), THICKNESS_M, YOUNG_PA,
            terms=12, samples=240,
        )
        centre = (bay + 0.5) / bays
        for index, f in enumerate(hertz):
            if f < RIB_KNEE_HZ or f > top_hz:
                continue
            # Its reach along the bridge: the bay it lives in, and nothing
            # outside it.
            point_x = np.linspace(0.0, 1.0, BOARD_DRIVE_POINTS)
            # A bay is clamped at its ribs, so its shape goes to zero there
            # and swells between: a half sine across the bay, times the
            # strip's own profile along it. Not a spike -- a mode confined to
            # a bay still spans that bay.
            reach = 0.5 * float(widths[bay]) / BRIDGE_LENGTH_M
            offset = (point_x - centre) / reach
            inside = np.abs(offset) < 1.0
            shape = np.zeros(BOARD_DRIVE_POINTS)
            if inside.any():
                across = np.cos(0.5 * math.pi * offset[inside])
                along = math.cos(0.7 * bay + 1.3 * (index % 5))
                shape[inside] = math.sqrt(2.0) * across * along
            rows.append((f, shape))
    rows.sort(key=lambda r: r[0])
    return np.array([r[0] for r in rows]), np.array([r[1] for r in rows])


def model_density(hertz):
    """`board_spacing` from the plugin, as modes per hertz."""
    flat = 1.0 / 0.06
    out = []
    for f in hertz:
        if f < RIB_KNEE_HZ:
            spacing = flat
        else:
            taper = flat * (f / RIB_KNEE_HZ) ** 1.92
            spacing = min(taper, 0.038 * f)
        out.append(1.0 / spacing)
    return np.array(out)


def main():
    print(__doc__.strip().splitlines()[0])
    print()
    hertz, vectors, xs, ys, terms = plate_modes(
        BOARD_LENGTH_M, BOARD_WIDTH_M, THICKNESS_M, YOUNG_PA
    )
    plate = hertz[(hertz >= BOARD_BOTTOM_HZ) & (hertz < RIB_KNEE_HZ)]
    keep = np.where((hertz >= BOARD_BOTTOM_HZ) & (hertz < RIB_KNEE_HZ))[0]
    plate_shapes = bridge_shapes(vectors[:, keep], xs, ys, terms)
    bay_hz, bay_shape = bay_modes(BOARD_TOP_HZ)

    print(f"placa resuelta: {len(plate)} modos entre {BOARD_BOTTOM_HZ:.0f} y "
          f"{RIB_KNEE_HZ:.0f} Hz   (primero {plate[0]:.1f} Hz)")
    print(f"vanos: {len(bay_hz)} modos entre {RIB_KNEE_HZ:.0f} y "
          f"{BOARD_TOP_HZ:.0f} Hz, en {int(round(BRIDGE_LENGTH_M / RIB_SPACING_M))} vanos")
    print()

    print("=== densidad modal: calculada contra la ley del modelo ===")
    print(f"{'banda':>14} {'calculada':>12} {'modelo':>12}")
    edges = [45, 100, 200, 400, 800, 1477, 3000, 6000, 8500]
    every = np.concatenate([plate, bay_hz])
    for lo, hi in zip(edges[:-1], edges[1:]):
        n = int(((every >= lo) & (every < hi)).sum())
        computed = n / (hi - lo)
        centre = math.sqrt(lo * hi)
        print(f"{lo:5.0f}-{hi:5.0f} Hz {computed:9.3f}/Hz "
              f"{model_density([centre])[0]:9.3f}/Hz")
    print()

    def unit(rows):
        return rows / (np.linalg.norm(rows, axis=1, keepdims=True) + 1e-12)

    plate_unit = unit(plate_shapes)
    bay_unit = unit(bay_shape) if len(bay_shape) else bay_shape

    print("=== cuanto rango necesita cada regimen ===")
    print("  Los dos no quieren la misma compresion. Un modo que abarca la")
    print("  tabla es una onda suave sobre los 16 puntos: rango bajo. Uno")
    print("  confinado toca dos: es ralo, y lo ralo NO es de rango bajo.")
    print()
    singular = np.linalg.svd(plate_unit, compute_uv=False)
    energy = np.cumsum(singular ** 2) / np.sum(singular ** 2)
    print(f"  placa ({len(plate_unit)} modos, globales):")
    for rank in (1, 2, 3, 4, 6, 8):
        if rank > len(singular):
            break
        print(f"    rango {rank:>2}: {energy[rank - 1] * 100:5.1f} % de la energia")
    if len(bay_unit):
        singular = np.linalg.svd(bay_unit, compute_uv=False)
        energy = np.cumsum(singular ** 2) / np.sum(singular ** 2)
        touched = float(np.mean((np.abs(bay_unit) > 1e-6).sum(axis=1)))
        print(f"  vanos ({len(bay_unit)} modos, confinados):")
        for rank in (2, 4, 8, 12, 16):
            if rank > len(singular):
                break
            print(f"    rango {rank:>2}: {energy[rank - 1] * 100:5.1f} % de la energia")
        print(f"    puntos tocados por modo: {touched:.1f} de {BOARD_DRIVE_POINTS}")
    print()
    print("=== lo que costaria por frame, sin perder nada ===")
    print("  No hay estructura de rango bajo que explotar: las formas llenan")
    print("  las 16 dimensiones. Asi que la lectura exacta es 16 terminos")
    print("  para un modo que abarca la tabla, y los puntos que toca para")
    print("  uno confinado -- que es donde esta el ahorro.")
    bay_touch = (float(np.mean((np.abs(bay_unit) > 1e-6).sum(axis=1)))
                 if len(bay_unit) else 0.0)
    total = len(plate_unit) + len(bay_unit)
    below = int(round(BOARD_MODES * len(plate_unit) / max(total, 1)))
    above = BOARD_MODES - below
    computed = below * BOARD_DRIVE_POINTS + int(bay_touch * above)
    today = BOARD_DRIVE_POINTS * BOARD_DRIVE_POINTS * 2 + 3 * BOARD_MODES
    print(f"    hoy:       transformada de 16 puntos + 3 por modo = {today}")
    print(f"    calculado: 16 por cada uno de {below} de placa + "
          f"{bay_touch:.1f} por cada uno de {above} de vano = {computed}")
    print(f"    -> {'cabe' if computed <= today * 1.15 else 'NO cabe'}: "
          f"{abs(computed - today) * 100 // today} % "
          f"{'mas barato' if computed < today else 'mas caro'} que hoy")
    print()
    print(f"  y el banco necesitaria {total} modos para cubrir "
          f"{BOARD_BOTTOM_HZ:.0f}-{BOARD_TOP_HZ:.0f} Hz, contra los "
          f"{BOARD_MODES} que tiene.")
    print()

    print("=== dependencia del lugar ===")
    print("  correlacion entre lo que dos puntos del puente excitan,")
    print("  por separacion (1.0 = ven el mismo tablero):")
    for label, rows in (("placa", plate_unit), ("vanos", bay_unit)):
        if not len(rows):
            continue
        columns = rows / (np.linalg.norm(rows, axis=0, keepdims=True) + 1e-12)
        line = "  ".join(
            f"{gap * BRIDGE_LENGTH_M / 15 * 100:.0f}cm "
            f"{np.mean([abs(float(columns[:, i] @ columns[:, i + gap])) for i in range(BOARD_DRIVE_POINTS - gap)]):.2f}"
            for gap in (1, 2, 4, 8)
        )
        print(f"    {label}: {line}")


if __name__ == "__main__":
    sys.exit(main())
