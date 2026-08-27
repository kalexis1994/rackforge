import { describe, expect, it } from "vitest";
import { shouldAnimateSurfaceTransition } from "./useSurfaceTransition";

describe("surface transition policy", () => {
  it("animates a visible route change", () => {
    expect(shouldAnimateSurfaceTransition({
      disabled: false,
      firstRender: false,
      reducedMotion: false,
      documentVisible: true,
    })).toBe(true);
  });

  it.each([
    { reason: "initial render", firstRender: true, reducedMotion: false, documentVisible: true, disabled: false },
    { reason: "reduced motion", firstRender: false, reducedMotion: true, documentVisible: true, disabled: false },
    { reason: "hidden document", firstRender: false, reducedMotion: false, documentVisible: false, disabled: false },
    { reason: "disabled surface", firstRender: false, reducedMotion: false, documentVisible: true, disabled: true },
  ])("does not animate for $reason", (input) => {
    expect(shouldAnimateSurfaceTransition(input)).toBe(false);
  });
});
