import { afterEach, describe, expect, it, vi } from "vitest";
import { DeferredConnectionOutage } from "./connectionOutage";

describe("deferred Core connection outage", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("does not alarm when the transport reconnects during the grace period", () => {
    vi.useFakeTimers();
    const publish = vi.fn();
    const outage = new DeferredConnectionOutage(4_000, publish);

    outage.begin();
    vi.advanceTimersByTime(1_200);
    outage.recover();
    vi.advanceTimersByTime(4_000);

    expect(publish).not.toHaveBeenCalled();
  });

  it("reports one persistent outage across repeated reconnect attempts", () => {
    vi.useFakeTimers();
    const publish = vi.fn();
    const outage = new DeferredConnectionOutage(4_000, publish);

    outage.begin();
    vi.advanceTimersByTime(1_200);
    outage.begin();
    vi.advanceTimersByTime(2_800);

    expect(publish).toHaveBeenCalledTimes(1);
  });
});
