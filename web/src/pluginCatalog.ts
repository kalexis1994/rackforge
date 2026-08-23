import { useEffect, useSyncExternalStore } from "react";
import { hostJson, IS_BROWSER_HOST } from "./host";
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
let browserAssetRefreshInstalled = false;
let browserAssetReadyRefreshDone = false;

/**
 * Re-reads installed plugin descriptors when the browser's service worker
 * starts serving package files.
 *
 * An installed plugin's iframe and branding live in Cache Storage. During a
 * reload the catalog can finish just before a newly activated worker claims
 * the page; that transient state must not remain cached as "no web view" for
 * the rest of the visit.
 */
function ensureBrowserAssetRefresh() {
  if (
    browserAssetRefreshInstalled ||
    !IS_BROWSER_HOST ||
    !("serviceWorker" in navigator)
  ) {
    return;
  }
  browserAssetRefreshInstalled = true;
  const refresh = () => {
    browserAssetReadyRefreshDone = true;
    void refreshPluginCatalog(true).catch(() => undefined);
  };
  navigator.serviceWorker.addEventListener("controllerchange", refresh);
  void navigator.serviceWorker.ready.then(() => {
    if (navigator.serviceWorker.controller && !browserAssetReadyRefreshDone) {
      refresh();
    }
  }).catch(() => undefined);
}

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
  ensureBrowserAssetRefresh();
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
