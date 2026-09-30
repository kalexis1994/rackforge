import { describe, expect, it } from "vitest";
import {
  KEY_RANGE_PRESETS,
  PIANO_WHITE_KEYS,
  describeRange,
  isBlackKey,
  moveEdge,
  nearestEdge,
  noteName,
  pianoKeys,
  transposedRange,
} from "./keyRange";

describe("key range", () => {
  it("names notes with middle C as C4", () => {
    expect(noteName(60)).toBe("C4");
    expect(noteName(21)).toBe("A0");
    expect(noteName(108)).toBe("C8");
    expect(noteName(0)).toBe("C-1");
    expect(noteName(127)).toBe("G9");
    expect(noteName(61)).toBe("C♯4");
  });

  it("lays out the piano's 88 keys, 52 of them white", () => {
    const keys = pianoKeys();
    expect(keys).toHaveLength(88);
    expect(keys.filter((key) => !key.black)).toHaveLength(PIANO_WHITE_KEYS);
    expect(keys[0]).toMatchObject({ note: 21, black: false, x: 0 });
    // A♯0 straddles the line between A0 and B0.
    expect(keys[1]).toMatchObject({ note: 22, black: true, x: 0.7 });
    expect(keys.at(-1)).toMatchObject({ note: 108, black: false, x: 51 });
    expect(isBlackKey(66)).toBe(true);
    expect(isBlackKey(64)).toBe(false);
  });

  it("moves the edge a touched key lies beyond, or the nearer one", () => {
    const range = { low: 48, high: 72 };
    expect(nearestEdge(range, 30)).toBe("low");
    expect(nearestEdge(range, 90)).toBe("high");
    expect(nearestEdge(range, 52)).toBe("low");
    expect(nearestEdge(range, 70)).toBe("high");
    expect(nearestEdge(range, 60)).toBe("low");
  });

  it("never inverts a range: an edge dragged past the other swaps with it", () => {
    const range = { low: 48, high: 72 };
    expect(moveEdge(range, "low", 50)).toEqual({ range: { low: 50, high: 72 }, edge: "low" });
    expect(moveEdge(range, "low", 80)).toEqual({ range: { low: 72, high: 80 }, edge: "high" });
    expect(moveEdge(range, "high", 40)).toEqual({ range: { low: 40, high: 48 }, edge: "low" });
    expect(moveEdge(range, "high", 200)).toEqual({ range: { low: 48, high: 127 }, edge: "high" });
    expect(moveEdge(range, "low", -5).range.low).toBe(0);
  });

  it("describes a range and what the instrument receives transposed", () => {
    expect(describeRange({ low: 36, high: 79 })).toBe("C2–G5, 44 keys");
    expect(describeRange({ low: 60, high: 60 })).toBe("C4 only");
    expect(transposedRange({ low: 0, high: 127 }, 12)).toEqual({ low: 12, high: 127 });
    expect(transposedRange({ low: 120, high: 127 }, 12)).toBeNull();
    expect(KEY_RANGE_PRESETS.map((preset) => preset.label)).toEqual([
      "All notes",
      "88 keys",
      "Below C4",
      "C4 and up",
    ]);
  });
});
