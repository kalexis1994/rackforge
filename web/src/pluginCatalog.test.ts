import { describe, expect, it } from "vitest";
import {
  canOpenInPlay,
  derivePluginRuntimeStates,
  groupPluginsByKind,
  pluginKind,
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
    kind: "instrument",
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

  it("does not present an enabled plugin as inactive while the host starts it", () => {
    const transitioning = { ...plugin(), transitioning: true };
    const state = derivePluginRuntimeStates(
      [transitioning],
      "online",
      session([]),
    )["org.rackforge.synth"];

    expect(transitioning.active).toBe(true);
    expect(state).toMatchObject({ phase: "loading", loaded: false, healthy: null });
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

function ofKind(
  id: string,
  kind: PluginWebDescriptor["kind"] | undefined,
): PluginWebDescriptor {
  return { ...plugin(), plugin_id: id, plugin_name: id, kind: kind as never };
}

/**
 * The Plugin Manager listed instruments, effects and MIDI processors in one
 * grid and offered every one of them a way into PLAY. PLAY is one instrument
 * and its programs: an effect belongs to a chain behind one, and pressing
 * that button moved the host into PLAY mode around a plugin that cannot be
 * played.
 */
describe("what a plugin is", () => {
  it("reads an unstated kind as an instrument", () => {
    expect(pluginKind(ofKind("legacy", undefined))).toBe("instrument");
    expect(canOpenInPlay(ofKind("legacy", undefined))).toBe(true);
  });

  it("lets PLAY open an instrument and nothing else", () => {
    expect(canOpenInPlay(ofKind("piano", "instrument"))).toBe(true);
    expect(canOpenInPlay(ofKind("reverb", "effect"))).toBe(false);
    expect(canOpenInPlay(ofKind("arp", "midi_processor"))).toBe(false);
  });

  it("groups the library by kind, in listing order", () => {
    const groups = groupPluginsByKind([
      ofKind("reverb", "effect"),
      ofKind("piano", "instrument"),
      ofKind("arp", "midi_processor"),
      ofKind("delay", "effect"),
    ]);
    expect(groups.map((group) => group.kind)).toEqual([
      "instrument",
      "effect",
      "midi_processor",
    ]);
    expect(groups[1].plugins.map((entry) => entry.plugin_id)).toEqual([
      "reverb",
      "delay",
    ]);
  });

  it("leaves out a kind nobody has installed", () => {
    const groups = groupPluginsByKind([ofKind("piano", "instrument")]);
    expect(groups.map((group) => group.kind)).toEqual(["instrument"]);
  });

  it("keeps every plugin, and keeps each one once", () => {
    const library = [
      ofKind("piano", "instrument"),
      ofKind("reverb", "effect"),
      ofKind("arp", "midi_processor"),
      ofKind("legacy", undefined),
    ];
    const listed = groupPluginsByKind(library).flatMap((group) => group.plugins);
    expect(listed).toHaveLength(library.length);
    expect(new Set(listed.map((entry) => entry.plugin_id)).size).toBe(library.length);
  });
});
