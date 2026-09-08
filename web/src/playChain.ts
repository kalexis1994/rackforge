import type { PluginWebDescriptor } from "./types";

/**
 * The PLAY chain: the instrument the player is on, then the effects lined
 * up after it, in order. It is what the chain drawer edits and what the
 * host will route audio through; until it does, the drawer says so.
 */
export interface PlayChainEffect {
  /** Unique within the chain: the same plugin may be in it twice. */
  id: string;
  plugin_id: string;
  enabled: boolean;
}

export interface PlayChain {
  instrument_id: string;
  effects: PlayChainEffect[];
}

/** What an instrument's manifest suggests after itself. */
export interface SuggestedChainEntry {
  plugin: string;
  preset?: string | null;
}

export interface SuggestedEffect {
  plugin_id: string;
  preset: string | null;
  /** The installed plugin, or null when the player does not have it. */
  descriptor: PluginWebDescriptor | null;
  inChain: boolean;
}

type ChainStorage = Pick<Storage, "getItem" | "setItem">;

const STORAGE_PREFIX = "rackforge.play.chain.";

export function chainStorageKey(instrumentId: string): string {
  return STORAGE_PREFIX + instrumentId;
}

export function emptyChain(instrumentId: string): PlayChain {
  return { instrument_id: instrumentId, effects: [] };
}

function browserStorage(): ChainStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** A stored chain read back, or the empty chain when there is none or it is not one. */
export function readStoredChain(
  instrumentId: string,
  storage: ChainStorage | null = browserStorage(),
): PlayChain {
  const empty = emptyChain(instrumentId);
  if (!storage) return empty;
  try {
    const raw = storage.getItem(chainStorageKey(instrumentId));
    if (!raw) return empty;
    return normaliseChain(instrumentId, JSON.parse(raw));
  } catch {
    return empty;
  }
}

export function storeChain(
  chain: PlayChain,
  storage: ChainStorage | null = browserStorage(),
): void {
  try {
    storage?.setItem(chainStorageKey(chain.instrument_id), JSON.stringify(chain));
  } catch {
    /* a full or blocked store loses the chain for next time, not for now */
  }
}

function normaliseChain(instrumentId: string, parsed: unknown): PlayChain {
  const chain = emptyChain(instrumentId);
  if (typeof parsed !== "object" || parsed === null) return chain;
  const effects = (parsed as { effects?: unknown }).effects;
  if (!Array.isArray(effects)) return chain;
  const seen = new Set<string>();
  for (const candidate of effects) {
    if (typeof candidate !== "object" || candidate === null) continue;
    const { id, plugin_id, enabled } = candidate as Record<string, unknown>;
    if (typeof id !== "string" || typeof plugin_id !== "string" || seen.has(id)) continue;
    seen.add(id);
    chain.effects.push({ id, plugin_id, enabled: enabled !== false });
  }
  return chain;
}

/** The chain with `pluginId` appended, under an id no other effect holds. */
export function withEffect(chain: PlayChain, pluginId: string): PlayChain {
  const taken = new Set(chain.effects.map((effect) => effect.id));
  let ordinal = 1;
  while (taken.has(`${pluginId}#${ordinal}`)) ordinal += 1;
  return {
    ...chain,
    effects: [
      ...chain.effects,
      { id: `${pluginId}#${ordinal}`, plugin_id: pluginId, enabled: true },
    ],
  };
}

export function withoutEffect(chain: PlayChain, effectId: string): PlayChain {
  return { ...chain, effects: chain.effects.filter((effect) => effect.id !== effectId) };
}

export function withEffectEnabled(
  chain: PlayChain,
  effectId: string,
  enabled: boolean,
): PlayChain {
  return {
    ...chain,
    effects: chain.effects.map((effect) =>
      effect.id === effectId ? { ...effect, enabled } : effect,
    ),
  };
}

/** The chain with the effect one place earlier (-1) or later (+1); unchanged at the ends. */
export function withEffectMoved(
  chain: PlayChain,
  effectId: string,
  direction: -1 | 1,
): PlayChain {
  const from = chain.effects.findIndex((effect) => effect.id === effectId);
  const to = from + direction;
  if (from < 0 || to < 0 || to >= chain.effects.length) return chain;
  const effects = [...chain.effects];
  const [moved] = effects.splice(from, 1);
  effects.splice(to, 0, moved);
  return { ...chain, effects };
}

/** The installed, enabled effect plugins, by name. */
export function effectPlugins(plugins: PluginWebDescriptor[]): PluginWebDescriptor[] {
  return plugins
    .filter((plugin) => plugin.kind === "effect" && plugin.active)
    .sort((a, b) => a.plugin_name.localeCompare(b.plugin_name));
}

/** The instrument's suggestions, resolved against what is installed and what is in the chain. */
export function suggestedEffects(
  suggested: SuggestedChainEntry[] | undefined,
  plugins: PluginWebDescriptor[],
  chain: PlayChain,
): SuggestedEffect[] {
  return (suggested ?? []).map((entry) => ({
    plugin_id: entry.plugin,
    preset: entry.preset ?? null,
    descriptor: plugins.find((plugin) => plugin.plugin_id === entry.plugin) ?? null,
    inChain: chain.effects.some((effect) => effect.plugin_id === entry.plugin),
  }));
}
