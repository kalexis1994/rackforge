import { describe, expect, it } from "vitest";
import {
  STEP_TICKS,
  clearMelodicStep,
  cycleMelodicTie,
  cycleMelodicVelocity,
  melodicStepNote,
  noteName,
  setMelodicStep,
  transposeMelodicStep,
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

describe("the melodic lens", () => {
  it("keeps one voice per step and sorts by time", () => {
    let pattern = emptyPattern("Bass", 1, 4, "melodic");
    expect(pattern.view).toBe("melodic");
    pattern = setMelodicStep(pattern, 4, 45);
    pattern = setMelodicStep(pattern, 0, 33);
    pattern = setMelodicStep(pattern, 4, 47); // replaces, never stacks
    expect(pattern.notes.map((note) => [note.tick / STEP_TICKS, note.key])).toEqual([
      [0, 33],
      [4, 47],
    ]);
    expect(melodicStepNote(pattern, 4)?.key).toBe(47);
    expect(melodicStepNote(pattern, 1)).toBeUndefined();
  });

  it("transposes with clamping and names notes like a synth panel", () => {
    let pattern = setMelodicStep(emptyPattern("Lead", 1, 4, "melodic"), 0, 126);
    pattern = transposeMelodicStep(pattern, 0, 12);
    expect(melodicStepNote(pattern, 0)?.key).toBe(127);
    pattern = transposeMelodicStep(pattern, 0, -7);
    expect(melodicStepNote(pattern, 0)?.key).toBe(120);
    expect(noteName(60)).toBe("C3");
    expect(noteName(45)).toBe("A1");
  });

  it("walks the tie ladder and the velocity levels", () => {
    let pattern = setMelodicStep(emptyPattern("Pad", 1, 4, "melodic"), 0, 60);
    pattern = cycleMelodicTie(pattern, 0);
    expect(melodicStepNote(pattern, 0)?.duration_ticks).toBe(2 * STEP_TICKS);
    pattern = cycleMelodicTie(pattern, 0);
    expect(melodicStepNote(pattern, 0)?.duration_ticks).toBe(4 * STEP_TICKS);
    pattern = cycleMelodicTie(pattern, 0);
    expect(melodicStepNote(pattern, 0)?.duration_ticks).toBe(STEP_TICKS);

    pattern = cycleMelodicVelocity(pattern, 0);
    expect(melodicStepNote(pattern, 0)?.velocity).toBe(127);
    pattern = cycleMelodicVelocity(pattern, 0);
    expect(melodicStepNote(pattern, 0)?.velocity).toBe(64);

    pattern = clearMelodicStep(pattern, 0);
    expect(melodicStepNote(pattern, 0)).toBeUndefined();
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
