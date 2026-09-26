import { describe, expect, it, vi } from "vitest";

vi.mock("./gateway", () => ({ requestSessionSnapshot: vi.fn() }));

import { awaitPluginInstance } from "./pluginLifecycle";
import type { SessionSnapshot } from "./types";

const snapshot = (pluginIds: string[], active?: string) =>
  ({
    active_instance_id: active,
    instances: pluginIds.map((pluginId, index) => ({
      instance_id: `instance-${index}`,
      plugin_id: pluginId,
    })),
  }) as unknown as SessionSnapshot;

describe("awaitPluginInstance", () => {
  it("asks again until the newly activated plugin is in the session", async () => {
    // The host said "active" before it published the instance: the first two
    // snapshots still show only what was playing.
    const replies = [
      snapshot(["org.example.old"], "instance-0"),
      snapshot(["org.example.old"], "instance-0"),
      snapshot(["org.example.old", "org.example.new"], "instance-0"),
    ];
    const request = vi.fn(async () => replies.shift()!);
    const { instance, snapshot: seen } = await awaitPluginInstance(
      "org.example.new",
      request,
      5_000,
      1,
    );
    expect(request).toHaveBeenCalledTimes(3);
    expect(instance?.instance_id).toBe("instance-1");
    // The snapshot it returns is the one that shows it, so the caller can
    // tell which instrument is still the active one.
    expect(seen.active_instance_id).toBe("instance-0");
  });

  it("gives up after its timeout rather than waiting for ever", async () => {
    const request = vi.fn(async () => snapshot(["org.example.old"]));
    const { instance } = await awaitPluginInstance("org.example.new", request, 20, 1);
    expect(instance).toBeUndefined();
    expect(request.mock.calls.length).toBeGreaterThan(1);
  });
});
