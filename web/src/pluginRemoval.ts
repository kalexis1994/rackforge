export interface PluginRemovalOptions {
  delete_presets: boolean;
  delete_plugin_data: boolean;
}

export interface PluginRemovalResult {
  cleanup_pending?: boolean;
  presets_deleted?: boolean;
  plugin_data_deleted?: boolean;
  user_data_cleanup_warning?: string | null;
}

export function pluginRemovalSummary(result: PluginRemovalResult) {
  const parts = ["Plugin package removed."];
  parts.push(result.presets_deleted ? "Presets deleted." : "Presets preserved.");
  parts.push(
    result.plugin_data_deleted
      ? "Imported resources and private plugin data deleted."
      : "Imported resources and private plugin data preserved.",
  );
  if (result.cleanup_pending) {
    parts.push("Locked package files will be cleaned after RackForge closes.");
  }
  if (result.user_data_cleanup_warning) {
    parts.push(`Some selected user data could not be removed: ${result.user_data_cleanup_warning}`);
  }
  return parts.join(" ");
}
