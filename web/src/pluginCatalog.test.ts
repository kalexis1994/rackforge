import { describe, expect, it } from "vitest";
import {
  derivePluginRuntimeStates,
  type PluginOperation,
} from "./pluginCatalog";
import type {
  PluginInstance,
  PluginWebDescriptor,
  SessionSnapshot,
} from "./types";

function plugin(active = true): PluginWebDescriptor {
  return {
    plugin_id: "org.rackforge.synth",
    plugin_name: "Synth",
    version: "1.0.0",
    active,
    managed: true,
    api_version: 1,
    surfaces: [],
    resources: [],
  };
}

function instance(): PluginInstance {
  return {
    instance_id: "desktop.org.rackforge.synth",
    plugin_id: "org.rackforge.synth",
    plugin_name: "Synth",
    ui_layouts: [],
    config_available: false,
    sounds: [],
  };
}

function session(instances: PluginInstance[]): SessionSnapshot {
  return { instances } as SessionSnapshot;
}

describe("global plugin runtime state", () => {
  it("marks an active published instance as loaded and healthy", () => {
    const state = derivePluginRuntimeStates(
      [plugin()],
      "online",
      session([instance()]),
    )["org.rackforge.synth"];

    expect(state).toMatchObject({
      phase: "ready",
      loaded: true,
      healthy: true,
      instance_id: "desktop.org.rackforge.synth",
    });
  });

  it("does not claim that an inactive or on-demand plugin is healthy", () => {
    const inactive = derivePluginRuntimeStates(
      [plugin(false)],
      "online",
      session([]),
    )["org.rackforge.synth"];
    const available = derivePluginRuntimeStates(
      [plugin()],
      "online",
      session([]),
    )["org.rackforge.synth"];

    expect(inactive).toMatchObject({ phase: "inactive", loaded: false, healthy: null });
    expect(available).toMatchObject({ phase: "available", loaded: false, healthy: null });
  });

  it("keeps loading explicit while Core or a plugin operation is pending", () => {
    const connecting = derivePluginRuntimeStates(
      [plugin()],
      "connecting",
      null,
    )["org.rackforge.synth"];
    const operation: PluginOperation = {
      kind: "activate",
      label: "Activating plugin…",
      token: 9,
    };
    const activating = derivePluginRuntimeStates(
      [plugin()],
      "online",
      session([]),
      new Map([["org.rackforge.synth", operation]]),
    )["org.rackforge.synth"];

    expect(connecting.phase).toBe("loading");
    expect(activating).toMatchObject({ phase: "loading", detail: "Activating plugin…" });
  });

  it("reports a missing previously loaded instance as unhealthy", () => {
    const state = derivePluginRuntimeStates(
      [plugin()],
      "online",
      session([]),
      new Map(),
      new Set(["org.rackforge.synth"]),
    )["org.rackforge.synth"];

    expect(state).toMatchObject({ phase: "unhealthy", loaded: false, healthy: false });
  });

  it("reports active plugins as unhealthy when the runtime disconnects", () => {
    const state = derivePluginRuntimeStates(
      [plugin()],
      "offline",
      session([instance()]),
    )["org.rackforge.synth"];

    expect(state).toMatchObject({ phase: "unhealthy", healthy: false });
  });
});
