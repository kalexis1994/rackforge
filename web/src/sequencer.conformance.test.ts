import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { tapTempo } from "./sequencer";

/**
 * A player taps a tempo in on stage, and this surface folds the same taps
 * with its own copy of the host's arithmetic. Steady taps agree easily; the
 * branches are where they would part company — a gap that means the player
 * started over, a sudden doubling, a hand faster or slower than a tempo is
 * allowed to be. A branch taken on one side and not the other is a tempo
 * that jumps when nobody is looking at it.
 *
 * `crates/rackforge-core` writes the fold down in
 * `fixtures/tap-tempo-v1.json` and a test there fails when the file stops
 * matching the host. Regenerate with
 * `UPDATE_TAP_TEMPO=1 cargo test -p rackforge-core`.
 */

interface TapSequence {
  name: string;
  taps_seconds: number[];
  bpm: number | null;
}

const { contract, sequences }: { contract: string; sequences: TapSequence[] } = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL("../../crates/rackforge-core/fixtures/tap-tempo-v1.json", import.meta.url),
    ),
    "utf8",
  ),
);

describe(`the tap fold answers as the host does (${contract})`, () => {
  it("covers the sequences the host recorded", () => {
    expect(sequences.length).toBeGreaterThan(0);
  });

  /**
   * Exactly, not nearly. Both sides sum the same intervals in the same order
   * in IEEE-754 doubles, so a difference of any size means one of them has
   * stopped being the other's transliteration.
   */
  for (const { name, taps_seconds, bpm } of sequences) {
    it(name, () => {
      expect(tapTempo(taps_seconds)).toBe(bpm);
    });
  }
});
