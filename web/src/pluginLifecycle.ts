import { requestSessionSnapshot } from "./gateway";
import { hostJson } from "./host";
import { beginPluginOperation, invalidatePluginCatalog } from "./pluginCatalog";
import { type PluginWebDescriptor } from "./types";

export interface InstalledPluginResult {
  plugin_id: string;
  version: string;
  already_installed: boolean;
  activation_required: boolean;
}

export const PLUGIN_ACTIVATION_TIMEOUT_MS = 45_000;

export async function synchronizePluginEnvironment() {
  await Promise.allSettled([
    invalidatePluginCatalog(),
    requestSessionSnapshot(),
  ]);
}

export async function activateInstalledPlugin(
  result: InstalledPluginResult,
): Promise<PluginWebDescriptor> {
  const finishOperation = beginPluginOperation(
    result.plugin_id,
    "activate",
    "Activating plugin…",
  );
  try {
    const activation = await hostJson<{ status?: string }>(
      `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}/activate`,
      { method: "POST" },
    );
    if (activation.status === "active") {
      const descriptor = await hostJson<PluginWebDescriptor>(
        `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
      );
      await synchronizePluginEnvironment();
      return descriptor;
    }
    const startedAt = performance.now();
    let lastError: unknown;
    while (performance.now() - startedAt < PLUGIN_ACTIVATION_TIMEOUT_MS) {
      try {
        const descriptor = await hostJson<PluginWebDescriptor>(
          `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
        );
        if (descriptor.active) {
          await synchronizePluginEnvironment();
          return descriptor;
        }
      } catch (error) {
        lastError = error;
      }
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
    throw lastError instanceof Error
      ? lastError
      : new Error("RackForge installed the plugin but activation did not finish in time.");
  } finally {
    finishOperation();
  }
}

export async function setInstalledPluginActive(pluginId: string, active: boolean) {
  const action = active ? "activate" : "deactivate";
  const finishOperation = beginPluginOperation(
    pluginId,
    action,
    active ? "Activating plugin…" : "Deactivating plugin…",
  );
  try {
    const response = await hostJson<{ status?: string; plugin_id?: string }>(
      `/api/v1/plugins/${encodeURIComponent(pluginId)}/${action}`,
      { method: "POST" },
    );
    const startedAt = performance.now();
    let lastError: unknown;
    while (performance.now() - startedAt < PLUGIN_ACTIVATION_TIMEOUT_MS) {
      try {
        const descriptor = await hostJson<PluginWebDescriptor>(
          `/api/v1/plugins/${encodeURIComponent(pluginId)}`,
        );
        if (descriptor.active === active && !descriptor.transitioning) {
          await synchronizePluginEnvironment();
          return response;
        }
      } catch (error) {
        lastError = error;
      }
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
    throw lastError instanceof Error
      ? lastError
      : new Error(`RackForge did not finish ${active ? "activating" : "deactivating"} the plugin.`);
  } finally {
    finishOperation();
  }
}
