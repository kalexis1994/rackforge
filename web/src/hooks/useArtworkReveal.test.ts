import { describe, expect, it } from "vitest";
import { revealState, type RevealTiming } from "./useArtworkReveal";

const timing: RevealTiming = {
  loaderDelayMs: 100,
  loaderMinimumMs: 400,
  loaderFadeMs: 300,
  timeoutMs: 2_000,
};

describe("revealState", () => {
  it("reveals a quick list with no loader at all", () => {
    expect(revealState(0, 60, 30, timing)).toEqual({
      revealed: false,
      loader: "hidden",
      nextChangeAt: 60,
    });
    expect(revealState(0, 60, 60, timing)).toEqual({
      revealed: true,
      loader: "hidden",
      nextChangeAt: null,
    });
  });

  it("shows the loader only once the wait is long enough to notice", () => {
    expect(revealState(0, null, 50, timing)).toEqual({
      revealed: false,
      loader: "hidden",
      nextChangeAt: 100,
    });
    expect(revealState(0, null, 150, timing)).toEqual({
      revealed: false,
      loader: "shown",
      nextChangeAt: 2_000,
    });
  });

  it("reveals at the deadline even if the artwork never arrives", () => {
    // Nothing reported the artwork ready; the arithmetic alone ends the wait.
    expect(revealState(0, null, 1_999, timing).revealed).toBe(false);
    expect(revealState(0, null, 2_000, timing)).toEqual({
      revealed: true,
      loader: "leaving",
      nextChangeAt: 2_300,
    });
    expect(revealState(0, null, 2_300, timing).loader).toBe("gone");
  });

  it("does not let late artwork move a reveal that already happened", () => {
    expect(revealState(0, 5_000, 2_100, timing)).toEqual(revealState(0, null, 2_100, timing));
  });

  it("keeps a loader that appeared for its minimum, so it never blinks", () => {
    // Ready at 150, just after the loader came up at 100: it stays to 500.
    expect(revealState(0, 150, 200, timing)).toEqual({
      revealed: false,
      loader: "shown",
      nextChangeAt: 500,
    });
    expect(revealState(0, 150, 500, timing)).toEqual({
      revealed: true,
      loader: "leaving",
      nextChangeAt: 800,
    });
    expect(revealState(0, 150, 800, timing).loader).toBe("gone");
  });

  it("reveals as soon as a long wait ends, the loader fading as the list arrives", () => {
    expect(revealState(0, 1_200, 1_200, timing)).toEqual({
      revealed: true,
      loader: "leaving",
      nextChangeAt: 1_500,
    });
  });
});
