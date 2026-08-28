import { describe, expect, it } from "vitest";
import {
  STEP_TICKS,
  TICKS_PER_BEAT,
  emptyPattern,
  hasStep,
  stepCount,
  tapTempo,
  toggleStep,
} from "./sequencer";

describe("the pattern grid", () => {
  it("builds an empty pattern of whole bars", () => {
    const pattern = emptyPattern("Test", 2, 4);
    expect(pattern.length_ticks).toBe(2 * 4 * TICKS_PER_BEAT);
    expect(pattern.notes).toEqual([]);
    expect(stepCount(pattern)).toBe(32);
  });

  it("toggles cells immutably and reads them back", () => {
    const empty = emptyPattern("Test", 1, 4);
    const withKick = toggleStep(empty, 36, 0);
    expect(hasStep(withKick, 36, 0)).toBe(true);
    expect(hasStep(empty, 36, 0)).toBe(false);
    expect(withKick.notes[0]).toEqual({
      tick: 0,
      duration_ticks: STEP_TICKS,
      key: 36,
      velocity: 100,
      channel: 0,
    });
    const cleared = toggleStep(withKick, 36, 0);
    expect(cleared.notes).toEqual([]);
  });

  it("keeps rows independent at the same step", () => {
    const pattern = toggleStep(toggleStep(emptyPattern("Test", 1, 4), 36, 4), 38, 4);
    expect(hasStep(pattern, 36, 4)).toBe(true);
    expect(hasStep(pattern, 38, 4)).toBe(true);
    const without = toggleStep(pattern, 36, 4);
    expect(hasStep(without, 36, 4)).toBe(false);
    expect(hasStep(without, 38, 4)).toBe(true);
  });
});

describe("tap tempo", () => {
  it("reads steady taps", () => {
    // 120 bpm: a tap every half second.
    const bpm = tapTempo([0, 0.5, 1.0, 1.5]);
    expect(bpm).not.toBeNull();
    expect(bpm!).toBeCloseTo(120, 5);
  });

  it("needs two taps and refuses time running backwards", () => {
    expect(tapTempo([1])).toBeNull();
    expect(tapTempo([2, 1])).toBeNull();
  });

  it("drops a stale session at a long gap, like the host does", () => {
    // Old taps at 140 bpm, a pause, then fresh taps at 100 bpm.
    const bpm = tapTempo([0, 0.428, 10, 10.6, 11.2]);
    expect(bpm).not.toBeNull();
    expect(bpm!).toBeCloseTo(100, 0);
  });

  it("clamps to the transport bounds", () => {
    expect(tapTempo([0, 0.05, 0.1])).toBe(400);
  });
});
