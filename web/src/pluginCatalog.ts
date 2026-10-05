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
  window.addEventListener("rackforge:plugin-assets-published", refresh);
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
    // No instance while the host is online is the host's choice, not a
    // failure: switching instruments in PLAY unloads the one being left, and
    // the session carries no fault for an instance. Calling that "missing"
    // flagged every instrument the player had just come from as broken.
    runtime[plugin.plugin_id] = {
      plugin_id: plugin.plugin_id,
      phase: "available",
      loaded: false,
      healthy: null,
      detail: "Active · Loads on demand",
    };
  }
  return runtime;
}

function currentRuntime(plugins: PluginWebDescriptor[]) {
  return derivePluginRuntimeStates(
    plugins,
    runtimeConnection,
    runtimeSnapshot,
    operations,
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

/** What a plugin is, for a descriptor that predates the field declaring it. */
export type PluginKind = PluginWebDescriptor["kind"];

/**
 * The kind a plugin is, treating an unstated one as an instrument.
 *
 * Instruments came first and were the only thing a package could be, so a
 * descriptor written before the field existed is one — which is also what
 * the card has always shown for them.
 */
export function pluginKind(plugin: PluginWebDescriptor): PluginKind {
  return plugin.kind ?? "instrument";
}

/**
 * Whether PLAY can open this plugin.
 *
 * PLAY is one instrument and its programs. An effect belongs to a chain
 * behind an instrument and a MIDI processor belongs in front of one; neither
 * is something PLAY can be pointed at, and asking for it moves the host into
 * PLAY mode around a plugin that cannot be played.
 */
export function canOpenInPlay(plugin: PluginWebDescriptor): boolean {
  return pluginKind(plugin) === "instrument";
}

/** CONFIG is opt-in: an installed plugin must declare that surface itself. */
export function declaresConfigSurface(plugin: PluginWebDescriptor): boolean {
  return plugin.surfaces.some((surface) => surface.kind === "config");
}

/** The kinds the Plugin Manager lists, in the order it lists them. */
export const PLUGIN_KIND_ORDER: readonly PluginKind[] = [
  "instrument",
  "effect",
  "midi_processor",
];

export interface PluginKindGroup {
  kind: PluginKind;
  plugins: PluginWebDescriptor[];
}

/**
 * The installed plugins split by what they are, in listing order.
 *
 * A kind with nothing in it is left out rather than shown empty: a machine
 * with no effects installed should not be told it has an empty shelf for
 * them. Order within a kind is the order the catalogue gave, so the list
 * does not reshuffle under a plugin being activated.
 */
export function groupPluginsByKind(
  plugins: readonly PluginWebDescriptor[],
): PluginKindGroup[] {
  return PLUGIN_KIND_ORDER.map((kind) => ({
    kind,
    plugins: plugins.filter((plugin) => pluginKind(plugin) === kind),
  })).filter((group) => group.plugins.length > 0);
}
