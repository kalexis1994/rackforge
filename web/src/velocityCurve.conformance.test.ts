import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import {
  evaluateVelocityCurve,
  isIdentityVelocityCurve,
  mapVelocity,
  sanitiseVelocityCurve,
  type VelocityCurve,
} from "./velocityCurve";

/**
 * The velocity reading exists twice — on the audio thread in Rust, and here,
 * where the square in Settings draws what that thread is about to do. Two
 * implementations of one curve drift, and this drift is the quiet kind: the
 * drawing goes on looking right while the keyboard plays something else.
 *
 * So neither side is the test any more. Both sides are held to one file of
 * numbers that `crates/rackforge-midi-api` generates and owns, and a test
 * there fails if the file stops matching the host. Regenerate with
 * `UPDATE_VELOCITY_VECTORS=1 cargo test -p rackforge-midi-api`, and read the
 * diff: every line of it is a reading that changed under someone's fingers.
 *
 * `velocityCurve.test.ts` keeps the properties this side owns alone — the
 * fractions and negatives a drag invents, the SVG path, the curvature that
 * has to survive an end being moved. This file keeps only the agreement.
 */

interface ConformanceCurve {
  name: string;
  input: VelocityCurve;
  sanitised: VelocityCurve;
  identity: boolean;
  map: number[];
  evaluate: number[];
}

interface Conformance {
  contract: string;
  evaluate_steps: number;
  curves: ConformanceCurve[];
}

const vectors: Conformance = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "../../crates/rackforge-midi-api/fixtures/velocity-curve-v1.json",
        import.meta.url,
      ),
    ),
    "utf8",
  ),
);

describe(`the velocity reading agrees with the host (${vectors.contract})`, () => {
  it("covers the curves the host recorded", () => {
    expect(vectors.curves.length).toBeGreaterThan(0);
    for (const curve of vectors.curves) {
      expect(curve.map, curve.name).toHaveLength(128);
      expect(curve.evaluate, curve.name).toHaveLength(vectors.evaluate_steps + 1);
    }
  });

  for (const { name, input, sanitised, identity, map, evaluate } of vectors.curves) {
    describe(name, () => {
      it("corrects the curve to the same four numbers", () => {
        expect(sanitiseVelocityCurve(input)).toEqual(sanitised);
      });

      it("agrees on whether it is the identity", () => {
        expect(isIdentityVelocityCurve(input)).toBe(identity);
      });

      it("reads every velocity as the same byte", () => {
        const here = Array.from({ length: 128 }, (_unused, velocity) =>
          mapVelocity(input, velocity),
        );
        expect(here).toEqual(map);
      });

      /**
       * Exactly, not nearly. Both sides run the same sequence of IEEE-754
       * double operations on the same widths, so the only way a bit differs
       * is if one of them stopped being a transliteration of the other —
       * which is the whole thing this file is here to notice.
       */
      it("draws the shape the host plays, bit for bit", () => {
        for (const [step, expected] of evaluate.entries()) {
          const x = step / vectors.evaluate_steps;
          expect(evaluateVelocityCurve(input, x), `${name} at x=${x}`).toBe(expected);
        }
      });
    });
  }
});
