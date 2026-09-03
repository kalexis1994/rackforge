/**
 * The velocity reading, drawn.
 *
 * This is the same curve the host applies, in the same arithmetic: monotone
 * cubic Hermite through three points with the Fritsch–Carlson tangents. It
 * exists here so the square in Settings draws exactly what the audio thread
 * will do to the next key you press, rather than an artist's impression of
 * it. `crates/rackforge-core/src/velocity_curve.rs` is the other half, and
 * the tests either side hold them to the same numbers.
 */

export interface VelocityCurve {
  /** What the softest strike becomes, at input 0. */
  low: number;
  /** Where the bend sits, and what it becomes. */
  mid_input: number;
  mid_output: number;
  /** What the hardest strike becomes, at input 127. */
  high: number;
}

export const IDENTITY_VELOCITY_CURVE: VelocityCurve = {
  low: 0,
  mid_input: 64,
  mid_output: 64,
  high: 127,
};

const clamp = (value: number, low: number, high: number) =>
  Math.min(high, Math.max(low, value));

/**
 * The bend inside the range, the outputs in the order the axis runs, every
 * number a whole one. A curve out of a file someone edited by hand is
 * corrected rather than refused — as the host does.
 */
export function sanitiseVelocityCurve(curve: VelocityCurve): VelocityCurve {
  const low = clamp(Math.round(curve.low), 0, 127);
  const high = clamp(Math.round(curve.high), 0, 127);
  const floor = Math.min(low, high);
  const ceiling = Math.max(low, high);
  return {
    low: floor,
    high: ceiling,
    mid_input: clamp(Math.round(curve.mid_input), 1, 126),
    mid_output: clamp(Math.round(curve.mid_output), floor, ceiling),
  };
}

/**
 * Where the bend sits inside the output span, 0 at the floor and 1 at the
 * ceiling. This is the curvature: it is the shape of the reading, and it is
 * relative, so moving the floor or the ceiling must not change it. A span of
 * nothing has no fraction to read, and the caller keeps the one it had.
 */
export function bendFraction(curve: VelocityCurve): number | null {
  const sane = sanitiseVelocityCurve(curve);
  const span = sane.high - sane.low;
  if (span <= 0) return null;
  return (sane.mid_output - sane.low) / span;
}

/**
 * The same curvature over a new floor and ceiling: the bend is placed back at
 * its own fraction of the span rather than clamped into it. Clamping is what
 * flattened a curve against the end you were dragging — pull the ceiling down
 * and the bend stayed where it was in absolute terms until the ceiling ran
 * into it, so the shape changed under your hand.
 */
export function withBendFraction(curve: VelocityCurve, fraction: number): VelocityCurve {
  const sane = sanitiseVelocityCurve(curve);
  const span = sane.high - sane.low;
  return sanitiseVelocityCurve({
    ...sane,
    mid_output: Math.round(sane.low + clamp(fraction, 0, 1) * span),
  });
}

export function isIdentityVelocityCurve(curve: VelocityCurve): boolean {
  const sane = sanitiseVelocityCurve(curve);
  return sane.low === 0 && sane.high === 127 && sane.mid_output === sane.mid_input;
}

/** The curve on the unit square, which is where it is drawn. */
export function evaluateVelocityCurve(curve: VelocityCurve, x: number): number {
  const sane = sanitiseVelocityCurve(curve);
  const xs = [0, sane.mid_input / 127, 1];
  const ys = [sane.low / 127, sane.mid_output / 127, sane.high / 127];
  return monotoneHermite(xs, ys, clamp(x, 0, 1));
}

/** What a strike of `velocity` becomes, as the host will read it. */
export function mapVelocity(curve: VelocityCurve, velocity: number): number {
  if (velocity <= 0) return 0;
  const sane = sanitiseVelocityCurve(curve);
  if (isIdentityVelocityCurve(sane)) return Math.min(127, Math.round(velocity));
  const mapped = evaluateVelocityCurve(sane, Math.min(127, velocity) / 127);
  return clamp(Math.round(mapped * 127), 1, 127);
}

function monotoneHermite(xs: number[], ys: number[], x: number): number {
  const secant = [0, 0];
  for (let i = 0; i < 2; i += 1) {
    const run = xs[i + 1] - xs[i];
    secant[i] = run > 1e-6 ? (ys[i + 1] - ys[i]) / run : 0;
  }
  const tangent = [secant[0], 0, secant[1]];
  // A turn, or a plateau on one side: flat here, so neither segment
  // overshoots into the other and the reading never falls.
  tangent[1] = secant[0] * secant[1] <= 0 ? 0 : 0.5 * (secant[0] + secant[1]);
  for (let i = 0; i < 2; i += 1) {
    if (Math.abs(secant[i]) <= 1e-9) {
      tangent[i] = 0;
      tangent[i + 1] = 0;
      continue;
    }
    const a = tangent[i] / secant[i];
    const b = tangent[i + 1] / secant[i];
    const magnitude = Math.sqrt(a * a + b * b);
    if (magnitude > 3) {
      const scale = 3 / magnitude;
      tangent[i] = scale * a * secant[i];
      tangent[i + 1] = scale * b * secant[i];
    }
  }
  const segment = x <= xs[1] ? 0 : 1;
  const run = xs[segment + 1] - xs[segment];
  if (run <= 1e-6) return ys[segment + 1];
  const t = clamp((x - xs[segment]) / run, 0, 1);
  const t2 = t * t;
  const t3 = t2 * t;
  const h00 = 2 * t3 - 3 * t2 + 1;
  const h10 = t3 - 2 * t2 + t;
  const h01 = -2 * t3 + 3 * t2;
  const h11 = t3 - t2;
  return clamp(
    h00 * ys[segment] +
      h10 * run * tangent[segment] +
      h01 * ys[segment + 1] +
      h11 * run * tangent[segment + 1],
    0,
    1,
  );
}

/** The curve as an SVG path across a square of `size`, y down. */
export function velocityCurvePath(curve: VelocityCurve, size: number, steps = 48): string {
  const points: string[] = [];
  for (let i = 0; i <= steps; i += 1) {
    const x = i / steps;
    const y = evaluateVelocityCurve(curve, x);
    points.push(`${(x * size).toFixed(2)},${((1 - y) * size).toFixed(2)}`);
  }
  return `M ${points.join(" L ")}`;
}
