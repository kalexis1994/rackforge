import { describe, expect, it } from "vitest";
import {
  buildRackPluginInstances,
  rackPluginRole,
  rackPluginsOfRole,
} from "./rackPluginSelection";
import type { PluginInstance, PluginWebDescriptor } from "./types";

const activePlay: PluginInstance = {
  instance_id: "android-main",
  plugin_id: "org.rackforge.piano",
  plugin_name: "Concert Grand",
  ui_layouts: ["play"],
  config_available: false,
  sounds: [],
  selected_sound_id: "grand",
};

function plugin(
  plugin_id: string,
  plugin_name: string,
  overrides: Partial<PluginWebDescriptor> = {},
): PluginWebDescriptor {
  return {
    plugin_id,
    plugin_name,
    version: "1.0.0",
    kind: "instrument",
    active: true,
    managed: true,
    api_version: 1,
    surfaces: [{ kind: "play", entry_url: "/play" }],
    resources: [],
    ...overrides,
  };
}

describe("Rack plugin selection", () => {
  it("includes enabled catalog instruments without replacing the PLAY instance", () => {
    const choices = buildRackPluginInstances(
      [activePlay],
      [
        plugin("org.rackforge.rf-106", "RF-106"),
        plugin("org.rackforge.piano", "Concert Grand"),
      ],
    );

    expect(choices.map((choice) => choice.plugin_id)).toEqual([
      "org.rackforge.piano",
      "org.rackforge.rf-106",
    ]);
    expect(choices[0]).toBe(activePlay);
    expect(choices[1].instance_id).toBe("rack-slot.org.rackforge.rf-106");
  });

  it("offers effects, which a Slot can own as readily as an instrument", () => {
    const choices = buildRackPluginInstances(
      [activePlay],
      [
        plugin("org.rackforge.piano", "Concert Grand"),
        plugin("org.rackforge.rf-rig", "RF-Rig", { kind: "effect" }),
      ],
    );

    expect(choices.map((choice) => choice.plugin_id)).toEqual([
      "org.rackforge.piano",
      "org.rackforge.rf-rig",
    ]);
  });

  it("excludes MIDI processors, inactive plugins and transitioning runtimes", () => {
    const choices = buildRackPluginInstances(
      [activePlay],
      [
        plugin("org.rackforge.piano", "Concert Grand"),
        plugin("org.rackforge.headless", "Headless Instrument", { surfaces: [] }),
        plugin("org.rackforge.notes", "Arpeggiator", { kind: "midi_processor" }),
        plugin("org.rackforge.off", "Inactive", { active: false }),
        plugin("org.rackforge.loading", "Loading", { transitioning: true }),
      ],
    );

    expect(choices.map((choice) => choice.plugin_id)).toEqual([
      "org.rackforge.piano",
      "org.rackforge.headless",
    ]);
  });

  it("keeps a MIDI processor out even when the session already exposes one", () => {
    const arpeggiator: PluginInstance = {
      ...activePlay,
      instance_id: "arp",
      plugin_id: "org.rackforge.notes",
      plugin_name: "Arpeggiator",
    };

    const choices = buildRackPluginInstances(
      [arpeggiator],
      [plugin("org.rackforge.notes", "Arpeggiator", { kind: "midi_processor" })],
    );

    expect(choices).toEqual([]);
  });

  it("accepts an older playable host descriptor that omitted kind", () => {
    const legacy = plugin("org.rackforge.legacy", "Legacy Instrument");
    delete (legacy as Partial<PluginWebDescriptor>).kind;

    expect(buildRackPluginInstances([], [legacy])[0]?.plugin_id).toBe(
      "org.rackforge.legacy",
    );
  });
});

describe("Rack plugin roles", () => {
  const catalog = [
    plugin("org.rackforge.piano", "Concert Grand"),
    plugin("org.rackforge.rf-rig", "RF-Rig", { kind: "effect" }),
  ];

  it("reads the role from the catalog", () => {
    expect(rackPluginRole("org.rackforge.rf-rig", catalog)).toBe("effect");
    expect(rackPluginRole("org.rackforge.piano", catalog)).toBe("instrument");
  });

  it("treats an undescribed plugin as an instrument", () => {
    // Wiring an unknown plugin for MIDI is what every existing Rack already
    // does, so it is the answer that cannot break one.
    expect(rackPluginRole("org.rackforge.unknown", catalog)).toBe("instrument");
  });

  it("splits a picker list by role", () => {
    const instances = buildRackPluginInstances([], catalog);

    expect(rackPluginsOfRole(instances, catalog, "effect").map((one) => one.plugin_id))
      .toEqual(["org.rackforge.rf-rig"]);
    expect(rackPluginsOfRole(instances, catalog, "instrument").map((one) => one.plugin_id))
      .toEqual(["org.rackforge.piano"]);
  });
});
