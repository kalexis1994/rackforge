import { describe, expect, it } from "vitest";
import { scrollEdges } from "./useScrollEdges";

describe("scrollEdges", () => {
  it("fades nothing when everything fits", () => {
    expect(scrollEdges(0, 400, 400)).toEqual({ start: false, end: false });
    // A fraction of a pixel of overflow is layout rounding, not content.
    expect(scrollEdges(0, 400.6, 400)).toEqual({ start: false, end: false });
  });

  it("fades the end at the start, both in the middle, the start at the end", () => {
    expect(scrollEdges(0, 700, 400)).toEqual({ start: false, end: true });
    expect(scrollEdges(150, 700, 400)).toEqual({ start: true, end: true });
    expect(scrollEdges(300, 700, 400)).toEqual({ start: true, end: false });
    expect(scrollEdges(299.5, 700, 400)).toEqual({ start: true, end: false });
  });
});
