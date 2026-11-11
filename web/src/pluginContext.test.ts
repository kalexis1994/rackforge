import { describe, expect, it } from "vitest";
import { pluginContextInstance, shouldPublishPluginContext } from "./pluginContext";
import type { PluginInstance, PluginStateReference } from "./types";

const instance: PluginInstance = {
  instance_id: "rack-slot.org.rackforge.rf-106",
  plugin_id: "org.rackforge.rf-106",
  plugin_name: "RF-106",
  ui_layouts: ["play"],
  config_available: false,
  sounds: [],
};

function state(selectedSoundId?: string): PluginStateReference {
  return {
    schema_version: 1,
    plugin_id: instance.plugin_id,
    plugin_version: "0.2.6",
    state_version: 1,
    blob_sha256: "a".repeat(64),
    byte_length: 128,
    selected_sound_id: selectedSoundId,
  };
}

describe("pluginContextInstance", () => {
  it("keeps the global PLAY instance unchanged", () => {
    expect(pluginContextInstance(instance, false)).toBe(instance);
  });

  it("uses the Rack Slot state as the isolated program identity", () => {
    expect(pluginContextInstance(instance, true, state("factory.rf106.000")))
      .toMatchObject({ selected_sound_id: "factory.rf106.000" });
  });

  it("keeps the required context field for legacy states", () => {
    expect(pluginContextInstance(instance, true, state()).selected_sound_id).toBe("");
  });
});

describe("shouldPublishPluginContext", () => {
  const published = { identity: "rf-106:play", json: '{"selected":"p1"}' };

  it("suppresses an identical Rack re-render", () => {
    expect(shouldPublishPluginContext(published, { ...published })).toBe(false);
  });

  it("publishes real changes and a different surface", () => {
    expect(shouldPublishPluginContext(published, { ...published, json: '{"selected":"p2"}' })).toBe(true);
    expect(shouldPublishPluginContext(published, { ...published, identity: "rf-7:play" })).toBe(true);
  });

  it("always answers a new iframe load or ready handshake", () => {
    expect(shouldPublishPluginContext(published, { ...published }, true)).toBe(true);
  });
});
