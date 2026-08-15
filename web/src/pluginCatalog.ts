import { useEffect, useSyncExternalStore } from "react";
import { hostJson } from "./host";
import type { PluginWebDescriptor } from "./types";

export interface PluginCatalogSnapshot {
  plugins: PluginWebDescriptor[];
  status: "idle" | "loading" | "ready" | "error";
  error: string | null;
}

let snapshot: PluginCatalogSnapshot = {
  plugins: [],
  status: "idle",
  error: null,
};
let generation = 0;
let inFlight: Promise<PluginWebDescriptor[]> | null = null;
const listeners = new Set<() => void>();

function publish(next: PluginCatalogSnapshot) {
  snapshot = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot() {
  return snapshot;
}

export function refreshPluginCatalog(force = false): Promise<PluginWebDescriptor[]> {
  if (inFlight && !force) return inFlight;
  if (snapshot.status === "ready" && !force) return Promise.resolve(snapshot.plugins);

  const requestGeneration = ++generation;
  publish({ ...snapshot, status: "loading", error: null });
  const request = hostJson<PluginWebDescriptor[]>("/api/v1/plugins")
    .then((plugins) => {
      if (requestGeneration === generation) {
        publish({ plugins, status: "ready", error: null });
      }
      return plugins;
    })
    .catch((error: unknown) => {
      if (requestGeneration === generation) {
        publish({
          ...snapshot,
          status: "error",
          error: error instanceof Error ? error.message : "Could not read installed plugins.",
        });
      }
      throw error;
    })
    .finally(() => {
      if (requestGeneration === generation) inFlight = null;
    });
  inFlight = request;
  return request;
}

export function invalidatePluginCatalog(): Promise<PluginWebDescriptor[]> {
  // A forced request owns a new generation, so a slower pre-mutation response
  // can never overwrite the fresh package list.
  return refreshPluginCatalog(true);
}

export function usePluginCatalog(): PluginCatalogSnapshot {
  const current = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  useEffect(() => {
    void refreshPluginCatalog().catch(() => undefined);
  }, []);
  return current;
}

export function usePluginDescriptor(pluginId: string | undefined) {
  const catalog = usePluginCatalog();
  return {
    descriptor: pluginId
      ? catalog.plugins.find((plugin) => plugin.plugin_id === pluginId) ?? null
      : null,
    status: catalog.status,
    error: catalog.error,
  };
}
