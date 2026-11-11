import { describe, expect, it } from "vitest";
import {
  DEFAULT_WEB_BUFFER_FRAMES,
  requestedWebBufferFrames,
  webBufferChoices,
} from "./audioBuffer";

describe("web audio buffer choices", () => {
  it("shows only the fixed quantum on browsers without variable render support", () => {
    expect(webBufferChoices(false)).toEqual([128]);
    expect(requestedWebBufferFrames("256", false)).toBe(DEFAULT_WEB_BUFFER_FRAMES);
  });

  it("restores a supported size and rejects stale or malformed preferences", () => {
    expect(webBufferChoices(true)).toEqual([128, 256, 512]);
    expect(requestedWebBufferFrames("256", true)).toBe(256);
    for (const stored of [null, "", "64", "1024", "garbage"]) {
      expect(requestedWebBufferFrames(stored, true)).toBe(256);
    }
  });
});
