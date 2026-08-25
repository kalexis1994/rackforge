import type { PluginInstance, PluginWebDescriptor } from "./types";

/**
 * Builds the instrument surface available to Rack and Song Part editors.
 *
 * Session instances are runtime details and may contain only the global PLAY
 * plugin on replaceable hosts such as Android. The installed catalog is the
 * authority for which enabled instruments can own an isolated graph node.
 */
export function buildRackInstrumentInstances(
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
  // known effects, inactive packages or transitioning runtimes into the picker.
  for (const instance of instances) {
    if (available.some((candidate) => candidate.plugin_id === instance.plugin_id)) {
      continue;
    }
    const descriptor = catalogById.get(instance.plugin_id);
    if (
      descriptor &&
      (
        (descriptor.kind != null && descriptor.kind !== "instrument") ||
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
