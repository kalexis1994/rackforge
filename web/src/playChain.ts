import type { PlayChainEffect, PlayChainState, PluginInstance, PluginWebDescriptor } from "./types";

/**
 * The PLAY chain: the instrument the player is on, then the effects lined
 * up after it, in order. The host holds it in the session (one chain per
 * instrument, `snapshot.play_chains`) and routes the instrument's audio
 * through every enabled effect; these helpers shape the next chain the
 * drawer sends with `set_play_chain`.
 */
export type PlayChain = PlayChainState;

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

/** The most effects a chain carries; the host refuses more. */
export const MAX_PLAY_CHAIN_EFFECTS = 8;

export function emptyChain(instrumentId: string): PlayChain {
  return { instrument_id: instrumentId, effects: [] };
}

/** The chain the session holds for the instrument, or the empty one. */
export function chainOf(
  chains: PlayChainState[] | undefined,
  instrumentId: string,
): PlayChain {
  return chains?.find((chain) => chain.instrument_id === instrumentId) ?? emptyChain(instrumentId);
}

/** The chain with `pluginId` appended, under an id no other effect holds. */
export function withEffect(
  chain: PlayChain,
  pluginId: string,
  programId?: string | null,
): PlayChain {
  if (chain.effects.length >= MAX_PLAY_CHAIN_EFFECTS) return chain;
  const taken = new Set(chain.effects.map((effect) => effect.id));
  let ordinal = 1;
  while (taken.has(`fx-${ordinal}`)) ordinal += 1;
  return {
    ...chain,
    effects: [
      ...chain.effects,
      {
        id: `fx-${ordinal}`,
        plugin_id: pluginId,
        enabled: true,
        // An instrument that names a program with its suggestion means that
        // program: adding it from the drawer arrives on it, not on defaults.
        ...(programId ? { program_id: programId } : {}),
      },
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

/** Two chains that would sound the same. */
export function sameChain(a: PlayChain, b: PlayChain): boolean {
  return (
    a.instrument_id === b.instrument_id
    && a.effects.length === b.effects.length
    && a.effects.every((effect, index) => {
      const other = b.effects[index];
      return (
        effect.id === other.id
        && effect.plugin_id === other.plugin_id
        && effect.enabled === other.enabled
      );
    })
  );
}

/**
 * The effect plugins the chain can take, by name: installed, enabled, and
 * something the host can actually build -- either because it has the plugin
 * loaded already (an instance in the session) or because it says it builds
 * one on demand out of its store.
 */
export function effectPlugins(
  plugins: PluginWebDescriptor[],
  instances?: PluginInstance[],
): PluginWebDescriptor[] {
  return plugins
    .filter(
      (plugin) =>
        plugin.kind === "effect"
        && plugin.active
        && (plugin.chainable === true
          || instances === undefined
          || instances.some((instance) => instance.plugin_id === plugin.plugin_id)),
    )
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

/** A suggestion the player could still take: installed, and not already there. */
export function isAddableSuggestion(suggestion: SuggestedEffect): boolean {
  return suggestion.descriptor !== null && !suggestion.inChain;
}

/**
 * The chain with every suggestion the player does not already have, in the
 * order the instrument asked for them.
 *
 * An instrument suggesting a chain is making one recommendation, not a list
 * of unrelated ones: the Concert Grand asks for glue and then a ceiling, in
 * that order, each on a named preset. Taking the whole recommendation should
 * cost what taking a recommendation costs, which is one decision.
 *
 * What it does not do is take it for the player. A suggestion is an opinion
 * about where an instrument sits in a mix, and the instrument is already
 * voiced without it — so this runs when somebody asks, and never on their
 * behalf.
 */
export function withSuggestedEffects(
  chain: PlayChain,
  suggestions: SuggestedEffect[],
): PlayChain {
  return suggestions.filter(isAddableSuggestion).reduce((next, suggestion) => {
    // Against the chain being built, not the one we started from: the room
    // left by `MAX_PLAY_CHAIN_EFFECTS` shrinks as this goes.
    if (next.effects.some((effect) => effect.plugin_id === suggestion.plugin_id)) {
      return next;
    }
    return withEffect(next, suggestion.plugin_id, suggestion.preset);
  }, chain);
}

export type { PlayChainEffect };
