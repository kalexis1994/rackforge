import { describe, expect, it } from "vitest";
import {
  MAX_PLAY_CHAIN_EFFECTS,
  chainOf,
  effectPlugins,
  emptyChain,
  isAddableSuggestion,
  sameChain,
  suggestedEffects,
  withEffect,
  withEffectEnabled,
  withEffectMoved,
  withSuggestedEffects,
  withoutEffect,
} from "./playChain";
import type { PluginInstance, PluginWebDescriptor } from "./types";

function descriptor(
  id: string,
  kind: PluginWebDescriptor["kind"],
  active = true,
): PluginWebDescriptor {
  return {
    plugin_id: id,
    plugin_name: id.split(".").pop() ?? id,
    version: "1.0.0",
    kind,
    active,
    managed: true,
    api_version: 1,
    surfaces: [],
    resources: [],
  };
}

function instance(pluginId: string): PluginInstance {
  return {
    instance_id: `play.${pluginId}`,
    plugin_id: pluginId,
    plugin_name: pluginId,
    ui_layouts: [],
    config_available: false,
    sounds: [],
  } as unknown as PluginInstance;
}

describe("the PLAY chain", () => {
  it("appends effects under ids no other effect holds", () => {
    let chain = withEffect(emptyChain("live.main.instrument.1"), "org.rackforge.rig");
    chain = withEffect(chain, "org.rackforge.rig");
    expect(chain.effects.map((effect) => effect.id)).toEqual(["fx-1", "fx-2"]);
    chain = withoutEffect(chain, "fx-1");
    chain = withEffect(chain, "org.rackforge.rig");
    expect(chain.effects.map((effect) => effect.id)).toEqual(["fx-2", "fx-1"]);
  });

  it("stops at the host's ceiling", () => {
    let chain = emptyChain("p");
    for (let n = 0; n < MAX_PLAY_CHAIN_EFFECTS + 2; n += 1) chain = withEffect(chain, "a");
    expect(chain.effects).toHaveLength(MAX_PLAY_CHAIN_EFFECTS);
  });

  it("moves an effect one place and stays put at the ends", () => {
    let chain = withEffect(withEffect(emptyChain("p"), "a"), "b");
    chain = withEffectMoved(chain, "fx-2", -1);
    expect(chain.effects.map((effect) => effect.plugin_id)).toEqual(["b", "a"]);
    expect(withEffectMoved(chain, "fx-2", -1)).toBe(chain);
    expect(withEffectMoved(chain, "fx-1", 1)).toBe(chain);
    expect(withEffectMoved(chain, "missing", 1)).toBe(chain);
  });

  it("toggles an effect without touching the others", () => {
    const chain = withEffectEnabled(
      withEffect(withEffect(emptyChain("p"), "a"), "b"),
      "fx-1",
      false,
    );
    expect(chain.effects.map((effect) => effect.enabled)).toEqual([false, true]);
  });

  it("finds the instrument's chain in the session, or none", () => {
    const chains = [withEffect(emptyChain("a"), "x"), withEffect(emptyChain("b"), "y")];
    expect(chainOf(chains, "b").effects[0].plugin_id).toBe("y");
    expect(chainOf(chains, "c")).toEqual(emptyChain("c"));
    expect(chainOf(undefined, "a")).toEqual(emptyChain("a"));
  });

  it("knows when two chains would sound the same", () => {
    const chain = withEffect(withEffect(emptyChain("p"), "a"), "b");
    expect(sameChain(chain, { ...chain, effects: [...chain.effects] })).toBe(true);
    expect(sameChain(chain, withEffectEnabled(chain, "fx-1", false))).toBe(false);
    expect(sameChain(chain, withEffectMoved(chain, "fx-2", -1))).toBe(false);
    expect(sameChain(chain, withoutEffect(chain, "fx-2"))).toBe(false);
  });

  it("offers only the installed, enabled effects the host has loaded, by name", () => {
    const plugins = [
      descriptor("org.rackforge.zeta", "effect"),
      descriptor("org.rackforge.piano", "instrument"),
      descriptor("org.rackforge.alpha", "effect"),
      descriptor("org.rackforge.off", "effect", false),
      descriptor("org.rackforge.unloaded", "effect"),
    ];
    const instances = [instance("org.rackforge.zeta"), instance("org.rackforge.alpha")];
    expect(effectPlugins(plugins, instances).map((plugin) => plugin.plugin_id)).toEqual([
      "org.rackforge.alpha",
      "org.rackforge.zeta",
    ]);
    expect(effectPlugins(plugins).map((plugin) => plugin.plugin_id)).toContain(
      "org.rackforge.unloaded",
    );
  });

  it("offers an effect a host builds on demand, loaded or not", () => {
    const onDemand = { ...descriptor("org.rackforge.comp", "effect"), chainable: true };
    const plugins = [onDemand, descriptor("org.rackforge.unloaded", "effect")];
    expect(effectPlugins(plugins, []).map((plugin) => plugin.plugin_id)).toEqual([
      "org.rackforge.comp",
    ]);
  });

  it("resolves the instrument's suggestions against the catalog and the chain", () => {
    const plugins = [descriptor("org.rackforge.rig", "effect")];
    const chain = withEffect(emptyChain("p"), "org.rackforge.rig");
    const resolved = suggestedEffects(
      [{ plugin: "org.rackforge.rig", preset: "Clean" }, { plugin: "org.rackforge.limiter" }],
      plugins,
      chain,
    );
    expect(resolved).toEqual([
      { plugin_id: "org.rackforge.rig", preset: "Clean", descriptor: plugins[0], inChain: true },
      { plugin_id: "org.rackforge.limiter", preset: null, descriptor: null, inChain: false },
    ]);
    expect(suggestedEffects(undefined, plugins, chain)).toEqual([]);
  });
});

it("adds a suggested effect on the program the instrument named", () => {
  const chain = withEffect(
    { instrument_id: "desktop.piano", effects: [] },
    "org.rackforge.rf-comp",
    "piano_glue",
  );
  expect(chain.effects).toEqual([
    {
      id: "fx-1",
      plugin_id: "org.rackforge.rf-comp",
      enabled: true,
      program_id: "piano_glue",
    },
  ]);
  // Added by hand from the picker, it arrives on the plugin's own default.
  const plain = withEffect({ instrument_id: "desktop.piano", effects: [] }, "org.rackforge.rf-eq");
  expect(plain.effects[0]).not.toHaveProperty("program_id");
});

/**
 * An instrument suggesting a chain is making one recommendation, not a list
 * of unrelated ones: the Concert Grand asks for glue and then a ceiling, in
 * that order, each on a named preset. Taking the whole recommendation costs
 * one decision — and still only ever when somebody asks for it.
 */
describe("taking the whole suggestion", () => {
  const installed = [
    descriptor("org.rackforge.rf-comp", "effect"),
    descriptor("org.rackforge.rf-limiter", "effect"),
  ];
  const suggestion = [
    { plugin: "org.rackforge.rf-comp", preset: "piano_glue" },
    { plugin: "org.rackforge.rf-limiter", preset: "transparent" },
  ];

  it("adds them in the order the instrument asked for, on the presets it named", () => {
    const empty = emptyChain("desktop.piano");
    const chain = withSuggestedEffects(empty, suggestedEffects(suggestion, installed, empty));
    expect(chain.effects.map((effect) => [effect.plugin_id, effect.program_id])).toEqual([
      ["org.rackforge.rf-comp", "piano_glue"],
      ["org.rackforge.rf-limiter", "transparent"],
    ]);
  });

  it("leaves alone what the player already took", () => {
    const started = withEffect(emptyChain("desktop.piano"), "org.rackforge.rf-comp", "piano_glue");
    const chain = withSuggestedEffects(started, suggestedEffects(suggestion, installed, started));
    expect(chain.effects).toHaveLength(2);
    expect(chain.effects.filter((e) => e.plugin_id === "org.rackforge.rf-comp")).toHaveLength(1);
  });

  it("skips what is not installed rather than inventing it", () => {
    const empty = emptyChain("desktop.piano");
    const partial = suggestedEffects(suggestion, [installed[0]], empty);
    const chain = withSuggestedEffects(empty, partial);
    expect(chain.effects.map((effect) => effect.plugin_id)).toEqual(["org.rackforge.rf-comp"]);
  });

  it("stops at the ceiling the host enforces", () => {
    let full = emptyChain("desktop.piano");
    for (let n = 0; n < MAX_PLAY_CHAIN_EFFECTS; n += 1) full = withEffect(full, `filler-${n}`);
    const chain = withSuggestedEffects(full, suggestedEffects(suggestion, installed, full));
    expect(chain.effects).toHaveLength(MAX_PLAY_CHAIN_EFFECTS);
  });

  it("changes nothing when there is nothing left to take", () => {
    let chain = withEffect(emptyChain("desktop.piano"), "org.rackforge.rf-comp", "piano_glue");
    chain = withEffect(chain, "org.rackforge.rf-limiter", "transparent");
    const again = withSuggestedEffects(chain, suggestedEffects(suggestion, installed, chain));
    expect(again).toEqual(chain);
  });

  it("knows which suggestions are still open to the player", () => {
    const started = withEffect(emptyChain("desktop.piano"), "org.rackforge.rf-comp");
    const resolved = suggestedEffects(suggestion, [installed[0]], started);
    expect(resolved.map(isAddableSuggestion)).toEqual([false, false]);
    const fresh = suggestedEffects(suggestion, installed, emptyChain("desktop.piano"));
    expect(fresh.map(isAddableSuggestion)).toEqual([true, true]);
  });
});
