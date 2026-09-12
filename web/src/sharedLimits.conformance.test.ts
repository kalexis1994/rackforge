import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { DEFAULT_INSTRUMENT_ID } from "./firstRun";
import { MAX_PLAY_CHAIN_EFFECTS } from "./playChain";
import { RACK_GRAPH_SCHEMA_VERSION } from "./rackGraph";
import {
  LANE_SLOTS,
  MAX_NOTE_LOCKS,
  MAX_PATTERN_NOTES,
  MAX_PATTERN_TICKS,
  MAX_SEQUENCER_LANES,
  MAX_TEMPO_BPM,
  MIN_TEMPO_BPM,
  SWING_MAX,
  SWING_STRAIGHT,
  TICKS_PER_BEAT,
} from "./sequencer";
import { SESSION_SCHEMA_VERSION } from "./sessionCommandProtocol";

/**
 * A limit the host enforces is useless to a player if this screen does not
 * know it. So the interface carries its own copy of a handful of them, and a
 * copy drifts — quietly. The drawer lets you add a ninth effect and the host
 * refuses the chain; a schema number moves on one side and a session stops
 * loading for no reason anyone can see.
 *
 * The crates that declare these numbers write them to
 * `crates/rackforge-core/fixtures/shared-limits-v1.json`, and this is where
 * the interface answers for its copies. Regenerate with
 * `UPDATE_SHARED_LIMITS=1 cargo test -p rackforge-core`.
 */

interface SharedLimit {
  value: number | string;
  owner: string;
}

const { contract, limits }: { contract: string; limits: Record<string, SharedLimit> } =
  JSON.parse(
    readFileSync(
      fileURLToPath(
        new URL("../../crates/rackforge-core/fixtures/shared-limits-v1.json", import.meta.url),
      ),
      "utf8",
    ),
  );

/** Every limit this side carries a copy of, under the name the host uses. */
const HERE: Record<string, number | string> = {
  DEFAULT_INSTRUMENT_ID,
  MAX_PLAY_CHAIN_EFFECTS,
  SESSION_SCHEMA_VERSION,
  RACK_GRAPH_SCHEMA_VERSION,
  TICKS_PER_BEAT,
  MAX_SEQUENCER_LANES,
  SWING_STRAIGHT,
  SWING_MAX,
  LANE_SLOTS,
  MAX_PATTERN_NOTES,
  MAX_PATTERN_TICKS,
  MAX_NOTE_LOCKS,
  MIN_TEMPO_BPM,
  MAX_TEMPO_BPM,
};

describe(`the interface knows the host's limits (${contract})`, () => {
  /**
   * Not only that the copies match, but that there are no limits left over.
   * A number added to the host and never claimed here is exactly the drift
   * this file exists to prevent, and it would otherwise pass in silence.
   */
  it("claims every limit the host declares, and invents none", () => {
    expect(Object.keys(HERE).sort()).toEqual(Object.keys(limits).sort());
  });

  for (const [name, { value, owner }] of Object.entries(limits)) {
    it(`agrees with ${owner} on ${name}`, () => {
      expect(HERE[name]).toBe(value);
    });
  }
});
