import { describe, expect, it, vi } from "vitest";
import { StartupTimeline } from "./startupPolicy";

describe("StartupTimeline", () => {
  it("advances monotonically and treats repeats as idempotent", () => {
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    const timeline = new StartupTimeline("test");
    timeline.advance("audio_ready");
    timeline.advance("audio_ready");
    timeline.advance("control_ready");
    timeline.advance("background_ready");
    expect(timeline.current()).toBe("background_ready");
  });

  it("allows an optional controller phase to be skipped but not reintroduced", () => {
    vi.spyOn(console, "info").mockImplementation(() => undefined);
    const timeline = new StartupTimeline("test");
    timeline.advance("audio_ready");
    timeline.advance("background_ready");
    expect(() => timeline.advance("control_ready")).toThrow(/cannot regress/i);
  });
});
