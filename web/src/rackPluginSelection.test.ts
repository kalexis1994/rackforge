import { describe, expect, it } from "vitest";
import { buildRackInstrumentInstances } from "./rackInstrumentSelection";
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

describe("Rack instrument selection", () => {
  it("includes enabled catalog instruments without replacing the PLAY instance", () => {
    const choices = buildRackInstrumentInstances(
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

  it("excludes effects, inactive plugins and transitioning runtimes", () => {
    const choices = buildRackInstrumentInstances(
      [activePlay],
      [
        plugin("org.rackforge.piano", "Concert Grand"),
        plugin("org.rackforge.headless", "Headless Instrument", { surfaces: [] }),
        plugin("org.rackforge.effect", "Delay", { kind: "effect" }),
        plugin("org.rackforge.off", "Inactive", { active: false }),
        plugin("org.rackforge.loading", "Loading", { transitioning: true }),
      ],
    );

    expect(choices.map((choice) => choice.plugin_id)).toEqual([
      "org.rackforge.piano",
      "org.rackforge.headless",
    ]);
  });

  it("accepts an older playable host descriptor that omitted kind", () => {
    const legacy = plugin("org.rackforge.legacy", "Legacy Instrument");
    delete (legacy as Partial<PluginWebDescriptor>).kind;

    expect(buildRackInstrumentInstances([], [legacy])[0]?.plugin_id).toBe(
      "org.rackforge.legacy",
    );
  });
});
