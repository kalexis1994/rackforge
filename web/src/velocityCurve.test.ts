import { describe, expect, it } from "vitest";

import {
  IDENTITY_VELOCITY_CURVE,
  bendFraction,
  evaluateVelocityCurve,
  isIdentityVelocityCurve,
  mapVelocity,
  sanitiseVelocityCurve,
  velocityCurvePath,
  withBendFraction,
  type VelocityCurve,
} from "./velocityCurve";

/**
 * The same properties the host holds itself to, because the square in
 * Settings has to draw what the audio thread will actually do. If these two
 * ever disagree, the drawing is a lie and the curve is the harder of the two
 * to notice.
 */
describe("the velocity reading", () => {
  it("leaves everything alone by default", () => {
    expect(isIdentityVelocityCurve(IDENTITY_VELOCITY_CURVE)).toBe(true);
    for (let velocity = 0; velocity <= 127; velocity += 1) {
      expect(mapVelocity(IDENTITY_VELOCITY_CURVE, velocity)).toBe(velocity);
    }
  });

  it("keeps a note off a note off, and never silences a strike", () => {
    const curve: VelocityCurve = { low: 40, mid_input: 64, mid_output: 80, high: 127 };
    expect(mapVelocity(curve, 0)).toBe(0);
    for (let velocity = 1; velocity <= 127; velocity += 1) {
      expect(mapVelocity(curve, velocity)).toBeGreaterThanOrEqual(1);
    }
    const silent: VelocityCurve = { low: 0, mid_input: 64, mid_output: 0, high: 0 };
    expect(mapVelocity(silent, 100)).toBeGreaterThanOrEqual(1);
  });

  it("never reads a harder strike as quieter", () => {
    const curves: VelocityCurve[] = [
      { low: 0, mid_input: 20, mid_output: 100, high: 127 },
      { low: 0, mid_input: 110, mid_output: 20, high: 127 },
      { low: 30, mid_input: 64, mid_output: 35, high: 90 },
      { low: 60, mid_input: 30, mid_output: 60, high: 60 },
      { low: 0, mid_input: 1, mid_output: 127, high: 127 },
      { low: 0, mid_input: 126, mid_output: 0, high: 127 },
    ];
    for (const curve of curves) {
      let previous = 0;
      for (let velocity = 1; velocity <= 127; velocity += 1) {
        const mapped = mapVelocity(curve, velocity);
        expect(mapped, `${JSON.stringify(curve)} at ${velocity}`).toBeGreaterThanOrEqual(previous);
        previous = mapped;
      }
    }
  });

  it("passes through the point the hand dragged", () => {
    for (const [mid_input, mid_output] of [
      [20, 90],
      [64, 30],
      [100, 110],
      [40, 40],
    ]) {
      const curve: VelocityCurve = { low: 0, mid_input, mid_output, high: 127 };
      expect(Math.abs(mapVelocity(curve, mid_input) - mid_output)).toBeLessThanOrEqual(1);
    }
  });

  it("puts the floor and the ceiling at the ends of the axis", () => {
    const curve: VelocityCurve = { low: 25, mid_input: 64, mid_output: 70, high: 110 };
    expect(Math.abs(mapVelocity(curve, 1) - 25)).toBeLessThanOrEqual(2);
    expect(mapVelocity(curve, 127)).toBe(110);
  });

  it("corrects nonsense rather than obeying it", () => {
    const sane = sanitiseVelocityCurve({ low: 120, mid_input: 200, mid_output: 3, high: 10 });
    expect(sane).toEqual({ low: 10, high: 120, mid_input: 126, mid_output: 10 });
  });

  it("draws a path that starts at the floor and ends at the ceiling", () => {
    const curve: VelocityCurve = { low: 0, mid_input: 32, mid_output: 96, high: 127 };
    const path = velocityCurvePath(curve, 100, 8);
    const points = path
      .replace("M ", "")
      .split(" L ")
      .map((pair) => pair.split(",").map(Number));
    expect(points).toHaveLength(9);
    expect(points[0][0]).toBeCloseTo(0);
    expect(points[0][1]).toBeCloseTo(100, 1);
    expect(points.at(-1)?.[0]).toBeCloseTo(100);
    expect(points.at(-1)?.[1]).toBeCloseTo(0, 1);
    // y grows downwards on a screen, so a rising reading falls across the box.
    for (let i = 1; i < points.length; i += 1) {
      expect(points[i][1]).toBeLessThanOrEqual(points[i - 1][1] + 1e-6);
    }
  });

  it("keeps its curvature when an end moves", () => {
    // The curvature is where the bend sits inside the span, not what number
    // it happens to be: pulling the ceiling down must carry the shape with
    // it rather than flatten the curve against the end being dragged.
    const bent: VelocityCurve = { low: 0, mid_input: 40, mid_output: 100, high: 127 };
    const before = bendFraction(bent);
    const lowered = withBendFraction({ ...bent, high: 80 }, before ?? 0.5);
    expect(bendFraction(lowered)).toBeCloseTo(before ?? 0, 2);
    expect(lowered.mid_output).toBe(63);
    // And a raised floor carries it too.
    const lifted = withBendFraction({ ...bent, low: 30 }, before ?? 0.5);
    expect(bendFraction(lifted)).toBeCloseTo(before ?? 0, 2);
  });

  it("keeps a straight line straight when an end moves", () => {
    const fraction = bendFraction(IDENTITY_VELOCITY_CURVE) ?? 0.5;
    for (const high of [127, 100, 64, 30]) {
      const moved = withBendFraction({ ...IDENTITY_VELOCITY_CURVE, high }, fraction);
      // Every reading on a straight line from the floor to the ceiling.
      for (const velocity of [1, 32, 64, 96, 127]) {
        const straight = (velocity / 127) * high;
        expect(
          Math.abs(mapVelocity(moved, velocity) - straight),
          `ceiling ${high} at ${velocity}`,
        ).toBeLessThanOrEqual(2);
      }
    }
  });

  it("has no fraction to read when the span is nothing", () => {
    expect(bendFraction({ low: 60, mid_input: 64, mid_output: 60, high: 60 })).toBeNull();
  });

  it("reads the unit square the same way at both ends", () => {
    const curve: VelocityCurve = { low: 10, mid_input: 40, mid_output: 90, high: 120 };
    expect(evaluateVelocityCurve(curve, 0)).toBeCloseTo(10 / 127, 5);
    expect(evaluateVelocityCurve(curve, 1)).toBeCloseTo(120 / 127, 5);
  });
});
