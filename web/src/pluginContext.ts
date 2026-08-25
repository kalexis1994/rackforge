import type { PluginInstance, PluginStateReference } from "./types";

/** Builds the instance portion of a Plugin Web context. */
export function pluginContextInstance(
  instance: PluginInstance,
  isolated: boolean,
  state?: PluginStateReference,
): PluginInstance {
  if (!isolated) return instance;
  return {
    ...instance,
    // Older state references may not carry a program identity. Keep the
    // required Web context field present; new states select a real default.
    selected_sound_id:
      state?.selected_sound_id ?? instance.selected_sound_id ?? "",
  };
}
