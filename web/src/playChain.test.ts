import { describe, expect, it } from "vitest";
import {
  chainStorageKey,
  effectPlugins,
  emptyChain,
  readStoredChain,
  storeChain,
  suggestedEffects,
  withEffect,
  withEffectEnabled,
  withEffectMoved,
  withoutEffect,
} from "./playChain";
import type { PluginWebDescriptor } from "./types";

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

function memoryStorage() {
  const map = new Map<string, string>();
  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    map,
  };
}

describe("the PLAY chain", () => {
  it("appends effects under ids no other effect holds", () => {
    let chain = withEffect(emptyChain("org.rackforge.piano"), "org.rackforge.rig");
    chain = withEffect(chain, "org.rackforge.rig");
    expect(chain.effects.map((effect) => effect.id)).toEqual([
      "org.rackforge.rig#1",
      "org.rackforge.rig#2",
    ]);
    chain = withoutEffect(chain, "org.rackforge.rig#1");
    chain = withEffect(chain, "org.rackforge.rig");
    expect(chain.effects.map((effect) => effect.id)).toEqual([
      "org.rackforge.rig#2",
      "org.rackforge.rig#1",
    ]);
  });

  it("moves an effect one place and stays put at the ends", () => {
    let chain = withEffect(withEffect(emptyChain("p"), "a"), "b");
    chain = withEffectMoved(chain, "b#1", -1);
    expect(chain.effects.map((effect) => effect.id)).toEqual(["b#1", "a#1"]);
    expect(withEffectMoved(chain, "b#1", -1)).toBe(chain);
    expect(withEffectMoved(chain, "a#1", 1)).toBe(chain);
    expect(withEffectMoved(chain, "missing", 1)).toBe(chain);
  });

  it("toggles an effect without touching the others", () => {
    const chain = withEffectEnabled(
      withEffect(withEffect(emptyChain("p"), "a"), "b"),
      "a#1",
      false,
    );
    expect(chain.effects.map((effect) => effect.enabled)).toEqual([false, true]);
  });

  it("round-trips through storage and drops what is not a chain", () => {
    const storage = memoryStorage();
    const chain = withEffectEnabled(
      withEffect(emptyChain("org.rackforge.piano"), "a"),
      "a#1",
      false,
    );
    storeChain(chain, storage);
    expect(readStoredChain("org.rackforge.piano", storage)).toEqual(chain);
    expect(readStoredChain("org.rackforge.other", storage)).toEqual(
      emptyChain("org.rackforge.other"),
    );
    storage.map.set(chainStorageKey("broken"), "{not json");
    expect(readStoredChain("broken", storage)).toEqual(emptyChain("broken"));
    storage.map.set(
      chainStorageKey("odd"),
      JSON.stringify({
        effects: [{ id: "x", plugin_id: "a" }, { id: "x", plugin_id: "b" }, 4, { id: 1 }],
      }),
    );
    expect(readStoredChain("odd", storage).effects).toEqual([
      { id: "x", plugin_id: "a", enabled: true },
    ]);
    expect(readStoredChain("none", null)).toEqual(emptyChain("none"));
  });

  it("offers only the installed, enabled effects, by name", () => {
    const plugins = [
      descriptor("org.rackforge.zeta", "effect"),
      descriptor("org.rackforge.piano", "instrument"),
      descriptor("org.rackforge.alpha", "effect"),
      descriptor("org.rackforge.off", "effect", false),
    ];
    expect(effectPlugins(plugins).map((plugin) => plugin.plugin_id)).toEqual([
      "org.rackforge.alpha",
      "org.rackforge.zeta",
    ]);
  });

  it("resolves the instrument's suggestions against the catalog and the chain", () => {
    const plugins = [descriptor("org.rackforge.rig", "effect")];
    const chain = withEffect(emptyChain("org.rackforge.piano"), "org.rackforge.rig");
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
