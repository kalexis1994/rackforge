import type { PluginInstance, PluginWebDescriptor } from "./types";

/**
 * What a plugin does in a Rack node.
 *
 * An instrument is played: MIDI goes in, audio comes out. An effect is patched:
 * audio goes in and audio comes out, so it has no note range, no key split and
 * nothing to transpose. The distinction decides how a new Slot is wired, and
 * nothing else — both own a graph node, both hold state, both carry presets.
 */
export type RackPluginRole = "instrument" | "effect";

/**
 * Builds the plugin surface available to Rack and Song Part editors.
 *
 * Session instances are runtime details and may contain only the global PLAY
 * plugin on replaceable hosts such as Android. The installed catalog is the
 * authority for which enabled plugins can own an isolated graph node.
 *
 * Effects belong here as much as instruments do. The engine has always been
 * able to run them — a Rack graph compiles a hardware audio input into a plugin
 * node, and the audio thread mixes the result — but the picker filtered them
 * out, so an effect could be installed and validated and still never reach a
 * board.
 */
export function buildRackPluginInstances(
  instances: PluginInstance[],
  plugins: PluginWebDescriptor[],
): PluginInstance[] {
  const liveInstances = new Map(
    instances.map((instance) => [instance.plugin_id, instance]),
  );
  const catalogById = new Map(
    plugins.map((plugin) => [plugin.plugin_id, plugin]),
  );
  const available = plugins
    .filter(
      (plugin) =>
        (plugin.kind === "instrument" ||
          plugin.kind === "effect" ||
          (plugin.kind == null && plugin.surfaces.some((surface) => surface.kind === "play"))) &&
        plugin.active &&
        !plugin.transitioning,
    )
    .map((plugin) => liveInstances.get(plugin.plugin_id) ?? {
      instance_id: "rack-slot." + plugin.plugin_id,
      plugin_id: plugin.plugin_id,
      plugin_name: plugin.plugin_name,
      ui_layouts: plugin.surfaces.map((surface) => surface.kind),
      config_available: plugin.surfaces.some((surface) => surface.kind === "config"),
      sounds: [],
    });

  // A legacy host may expose a playable instance before its catalog endpoint
  // becomes available. Preserve that temporary compatibility without allowing
  // MIDI processors, inactive packages or transitioning runtimes into the
  // picker — a MIDI processor emits no audio, so a Slot cannot own one.
  for (const instance of instances) {
    if (available.some((candidate) => candidate.plugin_id === instance.plugin_id)) {
      continue;
    }
    const descriptor = catalogById.get(instance.plugin_id);
    if (
      descriptor &&
      (
        (descriptor.kind != null &&
          descriptor.kind !== "instrument" &&
          descriptor.kind !== "effect") ||
        !descriptor.active ||
        descriptor.transitioning
      )
    ) {
      continue;
    }
    available.push(instance);
  }

  return [...available].sort((left, right) =>
    left.plugin_name.localeCompare(right.plugin_name),
  );
}

/**
 * What role a plugin plays in a Rack node.
 *
 * Anything the catalog does not call an effect is treated as an instrument,
 * including a plugin the catalog has not described yet: a Slot wired for MIDI
 * is the behaviour every existing Rack already has, so it is the safe answer
 * when the descriptor is missing.
 */
export function rackPluginRole(
  pluginId: string,
  plugins: PluginWebDescriptor[],
): RackPluginRole {
  const descriptor = plugins.find((plugin) => plugin.plugin_id === pluginId);
  return descriptor?.kind === "effect" ? "effect" : "instrument";
}

/** The plugins in `instances` that play the given role. */
export function rackPluginsOfRole(
  instances: PluginInstance[],
  plugins: PluginWebDescriptor[],
  role: RackPluginRole,
): PluginInstance[] {
  return instances.filter(
    (instance) => rackPluginRole(instance.plugin_id, plugins) === role,
  );
}
