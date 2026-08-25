import { useEffect, useSyncExternalStore } from "react";
import { hostJson, IS_BROWSER_HOST } from "./host";
import type {
  ConnectionStatus,
  PluginWebDescriptor,
  SessionSnapshot,
} from "./types";

export type PluginRuntimePhase =
  | "inactive"
  | "available"
  | "loading"
  | "ready"
  | "unhealthy";

export type PluginOperationKind =
  | "install"
  | "activate"
  | "deactivate"
  | "remove"
  | "open"
  | "refresh";

export interface PluginRuntimeStatus {
  plugin_id: string;
  phase: PluginRuntimePhase;
  loaded: boolean;
  healthy: boolean | null;
  detail: string;
  instance_id?: string;
}

export interface PluginOperation {
  kind: PluginOperationKind;
  label: string;
  token: number;
}

export interface PluginCatalogSnapshot {
  plugins: PluginWebDescriptor[];
  status: "idle" | "loading" | "ready" | "error";
  error: string | null;
  runtime: Record<string, PluginRuntimeStatus>;
}

let snapshot: PluginCatalogSnapshot = {
  plugins: [],
  status: "idle",
  error: null,
  runtime: {},
};
let generation = 0;
let inFlight: Promise<PluginWebDescriptor[]> | null = null;
let runtimeConnection: ConnectionStatus = "connecting";
let runtimeSnapshot: SessionSnapshot | null = null;
let operationToken = 0;
const operations = new Map<string, PluginOperation>();
const previouslyLoaded = new Set<string>();
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

export function derivePluginRuntimeStates(
  plugins: PluginWebDescriptor[],
  connection: ConnectionStatus,
  session: SessionSnapshot | null,
  activeOperations: ReadonlyMap<string, PluginOperation> = new Map(),
  loadedBefore: ReadonlySet<string> = new Set(),
): Record<string, PluginRuntimeStatus> {
  const runtime: Record<string, PluginRuntimeStatus> = {};
  const instancesByPlugin = new Map(
    (session?.instances ?? []).map((instance) => [instance.plugin_id, instance]),
  );
  for (const plugin of plugins) {
    const instance = instancesByPlugin.get(plugin.plugin_id);
    const operation = activeOperations.get(plugin.plugin_id);
    if (operation) {
      runtime[plugin.plugin_id] = {
        plugin_id: plugin.plugin_id,
        phase: "loading",
        loaded: Boolean(instance),
        healthy: instance && connection === "online" ? true : null,
        detail: operation.label,
        instance_id: instance?.instance_id,
      };
      continue;
    }
    if (plugin.transitioning) {
      runtime[plugin.plugin_id] = {
        plugin_id: plugin.plugin_id,
        phase: "loading",
        loaded: Boolean(instance),
        healthy: null,
        detail: "Host is changing the plugin runtime…",
        instance_id: instance?.instance_id,
      };
      continue;
    }
    if (!plugin.active) {
      runtime[plugin.plugin_id] = {
        plugin_id: plugin.plugin_id,
        phase: "inactive",
        loaded: false,
        healthy: null,
        detail: "Inactive",
      };
      continue;
    }
    if (connection === "offline" || connection === "idle") {
      runtime[plugin.plugin_id] = {
        plugin_id: plugin.plugin_id,
        phase: "unhealthy",
        loaded: false,
        healthy: false,
        detail: connection === "idle" ? "Audio runtime is idle" : "Runtime disconnected",
        instance_id: instance?.instance_id,
      };
      continue;
    }
    if (instance && connection === "online") {
      runtime[plugin.plugin_id] = {
        plugin_id: plugin.plugin_id,
        phase: "ready",
        loaded: true,
        healthy: true,
        detail: "Loaded and healthy",
        instance_id: instance.instance_id,
      };
      continue;
    }
    if (connection === "connecting" || !session) {
      runtime[plugin.plugin_id] = {
        plugin_id: plugin.plugin_id,
        phase: "loading",
        loaded: false,
        healthy: null,
        detail: "Checking runtime…",
      };
      continue;
    }
    const disappeared = loadedBefore.has(plugin.plugin_id);
    runtime[plugin.plugin_id] = {
      plugin_id: plugin.plugin_id,
      phase: disappeared ? "unhealthy" : "available",
      loaded: false,
      healthy: disappeared ? false : null,
      detail: disappeared ? "Runtime instance is missing" : "Active · Loads on demand",
    };
  }
  return runtime;
}

function updatePreviouslyLoaded(
  plugins: PluginWebDescriptor[],
  status: PluginCatalogSnapshot["status"],
) {
  const present = new Set(plugins.map((plugin) => plugin.plugin_id));
  const active = new Set(
    plugins.filter((plugin) => plugin.active).map((plugin) => plugin.plugin_id),
  );
  if (status === "ready") {
    for (const pluginId of previouslyLoaded) {
      if (!present.has(pluginId)) previouslyLoaded.delete(pluginId);
    }
    for (const plugin of plugins) {
      if (!plugin.active) previouslyLoaded.delete(plugin.plugin_id);
    }
  }
  for (const instance of runtimeSnapshot?.instances ?? []) {
    if (active.has(instance.plugin_id)) {
      previouslyLoaded.add(instance.plugin_id);
    }
  }
}

function currentRuntime(plugins: PluginWebDescriptor[]) {
  return derivePluginRuntimeStates(
    plugins,
    runtimeConnection,
    runtimeSnapshot,
    operations,
    previouslyLoaded,
  );
}

function runtimeEqual(
  left: Record<string, PluginRuntimeStatus>,
  right: Record<string, PluginRuntimeStatus>,
) {
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);
  if (leftKeys.length !== rightKeys.length) return false;
  return leftKeys.every((pluginId) => {
    const a = left[pluginId];
    const b = right[pluginId];
    return b !== undefined &&
      a.phase === b.phase &&
      a.loaded === b.loaded &&
      a.healthy === b.healthy &&
      a.detail === b.detail &&
      a.instance_id === b.instance_id;
  });
}

function publish(next: Omit<PluginCatalogSnapshot, "runtime"> | PluginCatalogSnapshot) {
  updatePreviouslyLoaded(next.plugins, next.status);
  snapshot = {
    ...next,
    runtime: currentRuntime(next.plugins),
  };
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

export function synchronizePluginRuntime(
  session: SessionSnapshot | null,
  connection: ConnectionStatus,
) {
  runtimeSnapshot = session;
  runtimeConnection = connection;
  updatePreviouslyLoaded(snapshot.plugins, snapshot.status);
  const runtime = currentRuntime(snapshot.plugins);
  if (runtimeEqual(snapshot.runtime, runtime)) return;
  snapshot = { ...snapshot, runtime };
  for (const listener of listeners) listener();
}

/**
 * Publishes a host-owned plugin operation to every surface. The returned
 * function only clears the operation it created, so overlapping work cannot
 * accidentally hide a newer loader.
 */
export function beginPluginOperation(
  pluginId: string,
  kind: PluginOperationKind,
  label: string,
) {
  const token = ++operationToken;
  operations.set(pluginId, { kind, label, token });
  publish(snapshot);
  return () => {
    if (operations.get(pluginId)?.token !== token) return;
    operations.delete(pluginId);
    publish(snapshot);
  };
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
    runtime: pluginId ? catalog.runtime[pluginId] ?? null : null,
  };
}
