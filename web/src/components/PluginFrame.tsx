import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSelector } from "react-redux";
import { NavLink, useNavigate } from "react-router";
import { AsyncSpinner } from "../components/AsyncSpinner";
import { ParameterLinkHost } from "../components/ParameterLinkHost";
import { PluginSurfaceState } from "../components/PluginSurfaceState";
import { RfLoader } from "../components/RfLoader";
import { PluginRemovalDialog } from "../dialogs/PluginRemovalDialog";
import { ResourceExplorerDialog } from "../dialogs/lazyResourceExplorer";
import { dispatchCommand, dispatchCommandAwait, materializePluginState, requestPluginParameters, requestPluginStateParameters, setPluginParameter, setPluginStateParameter } from "../gateway";
import { useResolvedLighting } from "../hooks/useResolvedLighting";
import { bindNativePluginResource, hostJson, isDesktopHost, isNativeHost, selectNativePluginSound } from "../host";
import { surfaceSettled, surfaceStarted } from "../bootReadiness";
import { beginPluginOperation, refreshPluginCatalog, usePluginDescriptor } from "../pluginCatalog";
import { pluginContextInstance } from "../pluginContext";
import { synchronizePluginEnvironment } from "../pluginLifecycle";
import { PluginRemovalOptions, PluginRemovalResult, pluginRemovalSummary } from "../pluginRemoval";
import { findEditorField, isProgramEditorValue } from "../programEditor";
import { validPluginProgramName } from "../programName";
import { postResourceApi } from "../resourceApi";
import { type RootState } from "../store";
import { type PluginInstance, type PluginResourceRequirement, type PluginStateReference, type PluginWebSurfaceKind, type ResourceGrant, type ResourceSelection } from "../types";
import { Trash2 } from "lucide-react";
import { BrandSplashArtwork } from "./BrandSplashArtwork";

export function PluginConfigSurface({ instance }: { instance: PluginInstance }) {
  const navigate = useNavigate();
  const { descriptor } = usePluginDescriptor(instance.plugin_id);
  const [showRemove, setShowRemove] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);

  const removePlugin = async (options: PluginRemovalOptions) => {
    if (!descriptor) return;
    const finishOperation = beginPluginOperation(
      descriptor.plugin_id,
      "remove",
      `Removing ${descriptor.plugin_name}…`,
    );
    setRemoving(true);
    setRemoveError(null);
    try {
      const result = await hostJson<PluginRemovalResult>(
        `/api/v1/plugins/${encodeURIComponent(descriptor.plugin_id)}`,
        {
          method: "DELETE",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(options),
        },
      );
      await synchronizePluginEnvironment();
      navigate("/plugins", {
        replace: true,
        state: { pluginRemovalMessage: pluginRemovalSummary(result) },
      });
    } catch (error) {
      setRemoveError(
        error instanceof Error ? error.message : "Could not remove the plugin.",
      );
    } finally {
      finishOperation();
      setRemoving(false);
    }
  };

  return (
    <section className="plugin-surface-shell">
      <div className="plugin-surface-toolbar">
        <div className="plugin-surface-identity">
          <NavLink to="/plugins" aria-label="Back to plugins">
            <span className="plugin-back-glyph" aria-hidden="true">←</span>
          </NavLink>
          <div>
            <span className="card-kicker">Plugin configuration</span>
            <strong>{instance.plugin_name}</strong>
          </div>
        </div>
        {descriptor?.managed ? (
          <button
            className="plugin-detail-remove-button"
            onClick={() => {
              setRemoveError(null);
              setShowRemove(true);
            }}
          >
            <Trash2 aria-hidden="true" />
            <span>Remove plugin</span>
          </button>
        ) : null}
      </div>
      <PluginFrame
        key={instance.instance_id}
        instance={instance}
        surface="config"
      />
      {showRemove && descriptor ? (
        <PluginRemovalDialog
          pluginName={descriptor.plugin_name}
          active
          removing={removing}
          error={removeError}
          onClose={() => setShowRemove(false)}
          onConfirm={removePlugin}
        />
      ) : null}
    </section>
  );
}

export const ISOLATED_PARAMETER_DEBOUNCE_MS = 48;

export const LIVE_PARAMETER_SYNC_MS = 100;

export interface PendingIsolatedParameterWrite {
  value: number;
  requestIds: string[];
  timer: number;
}

/**
 * Descriptors that have already had their one forced refresh, by plugin,
 * version and surface. Once per visit: a plugin that really has no web view
 * must not refresh the catalogue for ever.
 */
const surfaceHealAttempts = new Set<string>();

export function PluginFrame({
  instance,
  surface,
  onSurfaceInfoChange,
  isolated = false,
  isolatedState,
  onIsolatedStateChange,
  onSelectSound,
  parameterLinkInstanceId,
}: {
  instance: PluginInstance;
  surface: PluginWebSurfaceKind;
  onSurfaceInfoChange?: (info: { label: string; value: string } | null) => void;
  isolated?: boolean;
  isolatedState?: PluginStateReference;
  onIsolatedStateChange?: (state: PluginStateReference) => void;
  onSelectSound?: (soundId: string) => Promise<unknown>;
  parameterLinkInstanceId?: string;
}) {
  const catalogDescriptor = usePluginDescriptor(instance.plugin_id);
  const descriptor = catalogDescriptor.descriptor;
  const selectedSurface = descriptor?.surfaces.find(
    (candidate) => candidate.kind === surface,
  );
  const healKey = `${instance.plugin_id}:${descriptor?.version ?? "unknown"}:${surface}`;
  // A descriptor that already carries this surface keeps the frame up while
  // the catalogue refreshes behind it. Treating every refresh as "not known
  // yet" tore the iframe down and loaded the plugin again from nothing --
  // on every refresh the service worker asks for as it takes the page, and
  // after every activation -- which a cold start showed as one loader
  // after another. Only a surface nobody has seen yet waits for the list.
  //
  // One without it gets a single forced refresh before being declared
  // unavailable: in the browser the catalogue can be read a moment before
  // the worker that serves installed plugins is ready, and that answer --
  // "no web view" -- used to stand until someone refreshed by hand.
  const descriptorStatus = descriptor && selectedSurface
    ? "ready"
    : catalogDescriptor.status === "error"
      ? "error"
      : catalogDescriptor.status === "ready" && surfaceHealAttempts.has(healKey)
        ? "unavailable"
        : "loading";
  useEffect(() => {
    if (catalogDescriptor.status !== "ready" || (descriptor && selectedSurface)) return;
    if (surfaceHealAttempts.has(healKey)) return;
    surfaceHealAttempts.add(healKey);
    void refreshPluginCatalog(true).catch(() => undefined);
  }, [catalogDescriptor.status, descriptor, healKey, selectedSurface]);
  const surfaceIdentity = [
    instance.plugin_id,
    descriptor?.version ?? "loading",
    selectedSurface?.entry_url ?? surface,
  ].join(":");
  const [loadedFrameIdentity, setLoadedFrameIdentity] = useState<string | null>(null);
  const frameLoaded = loadedFrameIdentity === surfaceIdentity;
  const [frameDocumentGeneration, setFrameDocumentGeneration] = useState(0);
  // The splash's own lifecycle: the icon fill reaches the top, THEN the
  // whole overlay fades, THEN it unmounts. Removing it on iframe load was
  // an abrupt cut.
  const [completedSplashIdentity, setCompletedSplashIdentity] = useState<string | null>(null);
  const [hiddenSplashIdentity, setHiddenSplashIdentity] = useState<string | null>(null);
  const splashDone = completedSplashIdentity === surfaceIdentity;
  const splashGone = hiddenSplashIdentity === surfaceIdentity;
  const readinessKey = `${instance.instance_id}:${surface}`;
  const surfaceSettledNow =
    splashDone ||
    descriptorStatus === "error" ||
    descriptorStatus === "unavailable" ||
    (surface === "config" && !instance.config_available);
  useEffect(() => {
    surfaceStarted(readinessKey);
    return () => surfaceSettled(readinessKey);
  }, [readinessKey]);
  useEffect(() => {
    if (surfaceSettledNow) surfaceSettled(readinessKey);
  }, [readinessKey, surfaceSettledNow]);
  const splashLitRef = useRef<HTMLImageElement | null>(null);
  const frameLoadedRef = useRef(false);
  const [resourceBusy, setResourceBusy] = useState<string | null>(null);
  const snapshot = useSelector((state: RootState) => state.rackforge.snapshot);
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const liveParameterValuesRef = useRef<Map<number, number>>(new Map());
  const pendingResourceRequestRef = useRef<string | null>(null);
  const isolatedStateRef = useRef<PluginStateReference | undefined>(isolatedState);
  const [isolatedContextState, setIsolatedContextState] = useState<
    PluginStateReference | undefined
  >(isolatedState);
  const isolatedStateInputKey = isolatedState
    ? [
        isolatedState.plugin_id,
        isolatedState.plugin_version,
        isolatedState.state_version,
        isolatedState.blob_sha256,
        isolatedState.selected_sound_id ?? "",
      ].join(":")
    : "";
  const [previousIsolatedStateInputKey, setPreviousIsolatedStateInputKey] =
    useState(isolatedStateInputKey);
  const onIsolatedStateChangeRef = useRef(onIsolatedStateChange);
  const isolatedMaterializeRef = useRef<Promise<PluginStateReference> | null>(null);
  const isolatedWriteChainRef = useRef<Promise<void>>(Promise.resolve());
  const isolatedWritesRef = useRef<Map<number, PendingIsolatedParameterWrite>>(new Map());
  const [resourceRequest, setResourceRequest] = useState<{
    requestId: string;
    resource: PluginResourceRequirement;
  } | null>(null);
  const [isolatedBootstrapError, setIsolatedBootstrapError] = useState<string | null>(null);

  useEffect(
    () => () => {
      onSurfaceInfoChange?.(null);
    },
    [onSurfaceInfoChange],
  );

  const sendPluginResponse = useCallback(
    (requestId: string, ok: boolean, error?: string, result?: unknown) => {
      frameRef.current?.contentWindow?.postMessage(
        {
          protocol: "rackforge.plugin.web@1",
          kind: "response",
          request_id: requestId,
          ok,
          ...(error ? { error } : {}),
          ...(result !== undefined ? { result } : {}),
        },
        window.location.origin,
      );
    },
    [],
  );

  if (previousIsolatedStateInputKey !== isolatedStateInputKey) {
    setPreviousIsolatedStateInputKey(isolatedStateInputKey);
    setIsolatedContextState(isolatedState);
  }

  useEffect(() => {
    isolatedStateRef.current = isolatedContextState;
  }, [isolatedContextState]);

  useEffect(() => {
    onIsolatedStateChangeRef.current = onIsolatedStateChange;
  }, [onIsolatedStateChange]);

  useEffect(() => {
    frameLoadedRef.current = frameLoaded;
  }, [frameLoaded]);

  const publishIsolatedState = useCallback((state: PluginStateReference) => {
    isolatedStateRef.current = state;
    // Keep the iframe context authoritative even when the surrounding Rack
    // draft has not rerendered yet. Program selection depends on this field.
    setIsolatedContextState(state);
    onIsolatedStateChangeRef.current?.(state);
  }, []);

  const ensureIsolatedState = useCallback((): Promise<PluginStateReference> => {
    if (isolatedStateRef.current) return Promise.resolve(isolatedStateRef.current);
    if (!isolatedMaterializeRef.current) {
      const pending = materializePluginState(instance.plugin_id)
        .then((state) => {
          publishIsolatedState(state);
          return state;
        })
        .finally(() => {
          if (isolatedMaterializeRef.current === pending) {
            isolatedMaterializeRef.current = null;
          }
        });
      isolatedMaterializeRef.current = pending;
    }
    return isolatedMaterializeRef.current;
  }, [instance.plugin_id, publishIsolatedState]);

  // An isolated Rack Slot is not the global PLAY instance. Build its initial
  // immutable state before publishing the iframe context; otherwise plugins
  // that require a selected program can reject the incomplete context and
  // remain on their static boot screen forever.
  useEffect(() => {
    if (!isolated || isolatedStateRef.current) return;
    let cancelled = false;
    setIsolatedBootstrapError(null);
    void ensureIsolatedState().catch((error: unknown) => {
      if (cancelled) return;
      setIsolatedBootstrapError(
        error instanceof Error
          ? error.message
          : "Could not initialize the Rack Slot instrument.",
      );
    });
    return () => {
      cancelled = true;
    };
  }, [ensureIsolatedState, isolated]);

  const loadParameterSchemaForLink = useCallback(async () => {
    if (isolated) {
      const state = await ensureIsolatedState();
      return requestPluginStateParameters(state);
    }
    return requestPluginParameters(instance.instance_id);
  }, [ensureIsolatedState, instance.instance_id, isolated]);

  const flushIsolatedParameterWrite = useCallback((
    parameterIndex: number,
    pending: PendingIsolatedParameterWrite,
  ) => {
    isolatedWritesRef.current.delete(parameterIndex);
    const run = async () => {
      for (let attempt = 0; attempt < 3; attempt += 1) {
        const base = await ensureIsolatedState();
        const result = await setPluginStateParameter(base, parameterIndex, pending.value);
        const current = isolatedStateRef.current;
        if (
          current &&
          current.blob_sha256 !== base.blob_sha256 &&
          current.blob_sha256 !== result.state.blob_sha256
        ) {
          continue;
        }
        publishIsolatedState(result.state);
        return result.value;
      }
      throw new Error("Rack Slot state kept changing while the parameter was edited.");
    };
    const operation = isolatedWriteChainRef.current.then(run, run);
    isolatedWriteChainRef.current = operation.then(() => undefined, () => undefined);
    operation
      .then((canonical) => {
        for (const requestId of pending.requestIds) {
          sendPluginResponse(requestId, true, undefined, { value: canonical });
        }
      })
      .catch((error: unknown) => {
        const message = error instanceof Error
          ? error.message
          : "Could not set Rack Slot parameter.";
        for (const requestId of pending.requestIds) {
          sendPluginResponse(requestId, false, message);
        }
      });
  }, [ensureIsolatedState, publishIsolatedState, sendPluginResponse]);

  const queueIsolatedParameterWrite = useCallback((
    requestId: string,
    parameterIndex: number,
    value: number,
  ) => {
    let pending = isolatedWritesRef.current.get(parameterIndex);
    if (pending) {
      window.clearTimeout(pending.timer);
      pending.value = value;
      pending.requestIds.push(requestId);
    } else {
      pending = { value, requestIds: [requestId], timer: 0 };
      isolatedWritesRef.current.set(parameterIndex, pending);
    }
    const queued = pending;
    queued.timer = window.setTimeout(
      () => flushIsolatedParameterWrite(parameterIndex, queued),
      ISOLATED_PARAMETER_DEBOUNCE_MS,
    );
  }, [flushIsolatedParameterWrite]);

  useEffect(() => () => {
    for (const pending of isolatedWritesRef.current.values()) {
      window.clearTimeout(pending.timer);
      for (const requestId of pending.requestIds) {
        sendPluginResponse(requestId, false, "Rack Slot editor was closed.");
      }
    }
    isolatedWritesRef.current.clear();
  }, [sendPluginResponse]);

  const resetParameterForLink = useCallback(async (parameterIndex: number) => {
    const current = await loadParameterSchemaForLink();
    const parameter = current.schema.parameters.find(
      (candidate) => candidate.index === parameterIndex,
    );
    if (!parameter) throw new Error(`Plugin parameter ${parameterIndex} no longer exists.`);
    if (parameter.flags.read_only || parameter.kind.type === "meter") {
      throw new Error(`${parameter.name} is read-only and cannot be reset.`);
    }

    const selectedSoundId = isolated
      ? isolatedContextState?.selected_sound_id
      : instance.selected_sound_id;
    let resetValue: number | undefined;
    if (selectedSoundId) {
      const programState = await materializePluginState(instance.plugin_id, selectedSoundId);
      const programParameters = await requestPluginStateParameters(programState);
      resetValue = programParameters.values.find(
        (candidate) => candidate.index === parameterIndex,
      )?.value;
    }
    if (resetValue === undefined) {
      if ("default" in parameter.kind) {
        resetValue = Number(parameter.kind.default);
      } else if (parameter.kind.type === "trigger") {
        resetValue = 0;
      }
    }
    if (resetValue === undefined || !Number.isFinite(resetValue)) {
      throw new Error(`No program value is available for ${parameter.name}.`);
    }

    let canonicalValue: number;
    if (isolated) {
      const pending = isolatedWritesRef.current.get(parameterIndex);
      if (pending) {
        window.clearTimeout(pending.timer);
        flushIsolatedParameterWrite(parameterIndex, pending);
      }
      await isolatedWriteChainRef.current;
      const base = await ensureIsolatedState();
      const result = await setPluginStateParameter(base, parameterIndex, resetValue);
      publishIsolatedState(result.state);
      canonicalValue = result.value;
    } else {
      canonicalValue = await setPluginParameter(
        instance.instance_id,
        parameterIndex,
        resetValue,
      );
    }

    frameRef.current?.contentWindow?.postMessage(
      {
        protocol: "rackforge.plugin.web@1",
        kind: "parameter_changed",
        parameter_index: parameterIndex,
        value: canonicalValue,
      },
      window.location.origin,
    );
  }, [
    ensureIsolatedState,
    flushIsolatedParameterWrite,
    instance.instance_id,
    instance.plugin_id,
    instance.selected_sound_id,
    isolated,
    isolatedContextState?.selected_sound_id,
    loadParameterSchemaForLink,
    publishIsolatedState,
  ]);

  const pluginContextReady = !isolated || isolatedContextState !== undefined;
  const lighting = useResolvedLighting();
  const pluginContext = useMemo(() => {
    const contextInstance = pluginContextInstance(
      instance,
      isolated,
      isolatedContextState,
    );
    return {
      protocol: "rackforge.plugin.web@1",
      kind: "context",
      surface,
      instance: contextInstance,
      program_draft:
        snapshot?.program_draft?.instance_id === instance.instance_id
          ? snapshot.program_draft
          : null,
      audition:
        snapshot?.audition?.instance_id === instance.instance_id
          ? snapshot.audition
          : null,
      host: {
        active_mode: snapshot?.active_mode ?? "play",
        master_level: snapshot?.master_level ?? 0,
        master_pan: snapshot?.master_pan ?? 0,
        // A plugin draws its own surface and may want to sit in the same light
        // as the machine around it. Advisory: nothing obliges a plugin to read
        // this, and a plugin that ignores it is not broken.
        lighting,
      },
    };
  }, [instance, isolated, isolatedContextState, lighting, snapshot, surface]);

  // Parameter changes can originate outside the iframe (MIDI Learn links,
  // semantic .rfcontroller profiles, automation, or another RackForge
  // surface). Keep the visible plugin UI synchronized with the canonical
  // audio instance and publish only values that actually changed. The
  // request is serialized and visibility-aware so slow bridges cannot build
  // an unbounded polling backlog.
  useEffect(() => {
    const parameterValues = liveParameterValuesRef.current;
    parameterValues.clear();
    if (!frameLoaded || isolated || !selectedSurface) return;

    let cancelled = false;
    let timer = 0;
    const synchronize = async () => {
      if (cancelled) return;
      if (document.visibilityState !== "hidden") {
        try {
          const result = await requestPluginParameters(instance.instance_id);
          if (cancelled) return;
          for (const parameter of result.values) {
            if (parameterValues.get(parameter.index) === parameter.value) continue;
            parameterValues.set(parameter.index, parameter.value);
            frameRef.current?.contentWindow?.postMessage(
              {
                protocol: "rackforge.plugin.web@1",
                kind: "parameter_changed",
                parameter_index: parameter.index,
                value: parameter.value,
              },
              window.location.origin,
            );
          }
        } catch {
          // The regular plugin request path owns user-facing errors. A
          // transient reconnect during background synchronization must not
          // replace a useful plugin UI with repeated warnings.
        }
      }
      if (!cancelled) {
        timer = window.setTimeout(synchronize, LIVE_PARAMETER_SYNC_MS);
      }
    };
    void synchronize();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      parameterValues.clear();
    };
  }, [
    frameDocumentGeneration,
    frameLoaded,
    instance.instance_id,
    isolated,
    selectedSurface,
  ]);

  // The icon reveal: a dim copy of the plugin icon sits under a full-color
  // copy clipped from the top, and the clip retreats bottom-to-top. The
  // iframe gives no real progress, so the fill eases toward ~90% on its
  // own clock and completes the moment the frame reports loaded. The DOM
  // node is driven directly from the animation frame -- rendering React
  // sixty times a second for a clip-path would be its own jank.
  useEffect(() => {
    if (splashGone) return;
    let raf = 0;
    let progress = 0;
    const start = performance.now();
    const step = (now: number) => {
      const lit = splashLitRef.current;
      if (lit) {
        const seconds = (now - start) / 1000;
        const target = frameLoadedRef.current
          ? 1
          : 0.9 * (1 - Math.exp(-seconds / 0.9));
        progress += (Math.max(target, progress) - progress) * 0.12;
        lit.style.clipPath = `inset(${((1 - progress) * 100).toFixed(2)}% 0 0 0)`;
        if (frameLoadedRef.current && progress > 0.995) {
          lit.style.clipPath = "inset(0 0 0 0)";
          setCompletedSplashIdentity(surfaceIdentity);
          return;
        }
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [splashGone, surfaceIdentity]);

  // Insurance for the reveal: animation frames stop in a hidden window
  // (minimized, background tab), and the splash must never outlive the
  // interface it was covering. Once the frame is loaded, a plain timer
  // completes the splash even if no frame ever fires.
  useEffect(() => {
    if (!frameLoaded || splashDone) return;
    const timer = window.setTimeout(
      () => setCompletedSplashIdentity(surfaceIdentity),
      1800,
    );
    return () => window.clearTimeout(timer);
  }, [frameLoaded, splashDone, surfaceIdentity]);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame || !selectedSurface) return;

    const send = (message: unknown) =>
      frame.contentWindow?.postMessage(message, window.location.origin);
    const onMessage = (event: MessageEvent) => {
      if (
        event.source !== frame.contentWindow ||
        event.origin !== window.location.origin ||
        !event.data ||
        event.data.protocol !== "rackforge.plugin.web@1"
      ) {
        return;
      }
      if (event.data.kind === "ready") {
        setLoadedFrameIdentity(surfaceIdentity);
        setFrameDocumentGeneration((generation) => generation + 1);
        if (pluginContextReady) send(pluginContext);
        return;
      }
      if (
        event.data.kind !== "request" ||
        typeof event.data.request_id !== "string"
      ) {
        return;
      }
      const respond = (ok: boolean, error?: string, result?: unknown) =>
        send({
          protocol: "rackforge.plugin.web@1",
          kind: "response",
          request_id: event.data.request_id,
          ok,
          ...(error ? { error } : {}),
          ...(result !== undefined ? { result } : {}),
        });
      const params =
        event.data.params && typeof event.data.params === "object"
          ? event.data.params
          : {};
      const draft =
        snapshot?.program_draft?.instance_id === instance.instance_id
          ? snapshot.program_draft
          : undefined;
      if (
        event.data.method === "plugin.parameters" &&
        (surface === "play" || surface === "config")
      ) {
        if (isolated) {
          ensureIsolatedState()
            .then(requestPluginStateParameters)
            .then((result) => respond(true, undefined, result))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not read Rack Slot parameters.",
              ),
            );
        } else requestPluginParameters(instance.instance_id)
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not read plugin parameters.",
            ),
          );
      } else if (
        event.data.method === "plugin.set_parameter" &&
        (surface === "play" || surface === "config") &&
        Number.isInteger(params.parameter_index) &&
        typeof params.value === "number" &&
        Number.isFinite(params.value)
      ) {
        if (isolated) {
          queueIsolatedParameterWrite(
            event.data.request_id,
            Number(params.parameter_index),
            params.value,
          );
        } else setPluginParameter(
          instance.instance_id,
          Number(params.parameter_index),
          params.value,
        )
          .then((value) => respond(true, undefined, { value }))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not set plugin parameter.",
            ),
          );
      } else if (
        event.data.method === "plugin.select_sound" &&
        (surface === "play" || surface === "config") &&
        typeof params.sound_id === "string" &&
        instance.sounds.some(
          (sound) => sound.id === params.sound_id,
        )
      ) {
        if (onSelectSound) {
          onSelectSound(params.sound_id)
            .then((result) => {
              if (
                isolated &&
                result &&
                typeof result === "object" &&
                "state" in result
              ) {
                publishIsolatedState(
                  (result as { state: PluginStateReference }).state,
                );
              }
              respond(true, undefined, result);
            })
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error ? error.message : "Could not select this sound.",
              ),
            );
        } else if (isNativeHost()) {
          selectNativePluginSound({
            instance_id: instance.instance_id,
            sound_id: params.sound_id,
          })
            .then((result) => respond(true, undefined, result))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not select this program.",
              ),
            );
        } else {
          const soundId = params.sound_id;
          dispatchCommandAwait({
            type: "select_sound",
            instance_id: instance.instance_id,
            sound_id: soundId,
          })
            .then(() => respond(true, undefined, { sound_id: soundId }))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not select this program.",
              ),
            );
        }
      } else if (
        event.data.method === "plugin.select_resource" &&
        surface === "config" &&
        typeof params.resource_id === "string"
      ) {
        const resource = descriptor?.resources.find(
          (candidate) => candidate.id === params.resource_id,
        );
        if (!resource) {
          respond(false, "Resource is not declared by this plugin.");
        } else if (pendingResourceRequestRef.current) {
          respond(false, "Another resource selection is already open.");
        } else if (isNativeHost() || isDesktopHost()) {
          pendingResourceRequestRef.current = event.data.request_id;
          const extensions = Array.isArray(params.extensions)
            ? params.extensions
                .filter(
                  (extension: unknown): extension is string =>
                    typeof extension === "string" &&
                    /^\.?[a-z0-9]+$/i.test(extension),
                )
                .slice(0, 16)
            : undefined;
          bindNativePluginResource({
            plugin_id: instance.plugin_id,
            resource_id: resource.id,
            kind: resource.kind,
            extensions,
          })
            .then((grant) => respond(true, undefined, grant))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not select this resource.",
              ),
            )
            .finally(() => {
              pendingResourceRequestRef.current = null;
            });
        } else {
          pendingResourceRequestRef.current = event.data.request_id;
          setResourceRequest({
            requestId: event.data.request_id,
            resource,
          });
        }
      } else if (
        event.data.method === "plugin.resource_bindings" &&
        (surface === "play" || surface === "config")
      ) {
        postResourceApi("/api/v1/resources/grants", {
          plugin_id: instance.plugin_id,
        })
          .then((grants) => respond(true, undefined, grants))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not read resource bindings.",
            ),
          );
      } else if (
        event.data.method === "plugin.resource_status" &&
        surface === "config"
      ) {
        postResourceApi("/api/v1/resources/status", {
          plugin_id: instance.plugin_id,
        })
          .then((status) => respond(true, undefined, status))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not read installed resources.",
            ),
          );
      } else if (
        event.data.method === "plugin.resource_entries" &&
        surface === "config" &&
        typeof params.grant_id === "string" &&
        (params.parent_id === null || params.parent_id === undefined ||
          typeof params.parent_id === "string")
      ) {
        postResourceApi("/api/v1/resources/browse", {
          plugin_id: instance.plugin_id,
          grant_id: params.grant_id,
          parent_id: typeof params.parent_id === "string" ? params.parent_id : null,
        })
          .then((entries) => respond(true, undefined, entries))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not browse this resource.",
            ),
          );
      } else if (
        event.data.method === "plugin.preview_resource" &&
        surface === "config" &&
        params.edit_mode === 1 &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        ) &&
        typeof params.file_name === "string" &&
        params.file_name.length > 0 &&
        params.file_name.length <= 160 &&
        params.bytes instanceof ArrayBuffer &&
        params.bytes.byteLength > 0 &&
        params.bytes.byteLength <= 128 * 1024 * 1024
      ) {
        setResourceBusy("Updating builder audition…");
        const fileName = params.file_name.replace(/[^a-z0-9._ -]+/gi, "-");
        hostJson<ResourceSelection>(
          `/api/v1/resources/uploads?name=${encodeURIComponent(fileName)}`,
          {
            method: "POST",
            headers: { "content-type": "application/octet-stream" },
            body: new Blob([params.bytes], { type: "application/vnd.rackforge.bank+zip" }),
          },
        )
          .then((selection) => postResourceApi<ResourceGrant>(
            "/api/v1/resources/bind-selection",
            {
              plugin_id: instance.plugin_id,
              resource_id: params.target_resource_id,
              selection_id: selection.selection_id,
            },
          ))
          .then((grant) => postResourceApi("/api/v1/resources/load", {
            plugin_id: instance.plugin_id,
            instance_id: instance.instance_id,
            target_resource_id: params.target_resource_id,
            grant_id: grant.grant_id,
            entry_id: null,
            persist: false,
            preview: true,
            bundle: null,
          }))
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not audition this resource.",
            ),
          )
          .finally(() => setResourceBusy(null));
      } else if (
        (((event.data.method === "plugin.load_resource" ||
          event.data.method === "plugin.install_resource") && surface === "config") ||
          (event.data.method === "plugin.activate_resource" && surface === "play")) &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        ) &&
        typeof params.grant_id === "string" &&
        (params.entry_id === null || params.entry_id === undefined ||
          typeof params.entry_id === "string")
      ) {
        const operationLabel = event.data.method === "plugin.activate_resource"
          ? "Activating resource…"
          : event.data.method === "plugin.install_resource"
            ? "Installing resource…"
            : "Loading resource…";
        setResourceBusy(operationLabel);
        postResourceApi("/api/v1/resources/load", {
          plugin_id: instance.plugin_id,
          instance_id: instance.instance_id,
          target_resource_id: params.target_resource_id,
          grant_id: params.grant_id,
          entry_id: typeof params.entry_id === "string" ? params.entry_id : null,
          persist: event.data.method !== "plugin.load_resource",
          preview: false,
          bundle: params.bundle === "nki_dependencies" || params.bundle === "sfz_dependencies"
            ? params.bundle
            : null,
        })
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not load this resource.",
            ),
          )
          .finally(() => setResourceBusy(null));
      } else if (
        event.data.method === "plugin.clear_resource" &&
        surface === "config" &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        )
      ) {
        setResourceBusy("Clearing resource…");
        postResourceApi("/api/v1/resources/clear", {
          plugin_id: instance.plugin_id,
          instance_id: instance.instance_id,
          target_resource_id: params.target_resource_id,
        })
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not clear this resource.",
            ),
          )
          .finally(() => setResourceBusy(null));
      } else if (
        event.data.method === "plugin.set_surface_info" &&
        surface === "play" &&
        (params.label === undefined ||
          (typeof params.label === "string" && params.label.length <= 24)) &&
        (params.value === null ||
          params.value === undefined ||
          (typeof params.value === "string" && params.value.length <= 96))
      ) {
        const label = typeof params.label === "string" ? params.label.trim() : "";
        const value = typeof params.value === "string" ? params.value.trim() : "";
        onSurfaceInfoChange?.(value ? { label, value } : null);
        respond(true, undefined, { published: value.length > 0 });
      } else if (
        event.data.method === "plugin.begin_program_edit" &&
        (surface === "play" || surface === "config") &&
        (params.program_id === null ||
          (typeof params.program_id === "string" &&
            instance.sounds.some(
              (sound) => sound.id === params.program_id && sound.editable,
            )))
      ) {
        if (isolated) {
          respond(false, "Program document editing is unavailable in a Rack Slot session.");
        } else {
          dispatchCommandAwait({
            type: "begin_program_edit",
            instance_id: instance.instance_id,
            ...(typeof params.program_id === "string"
              ? { program_id: params.program_id }
              : {}),
          })
            .then(() => respond(true))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error ? error.message : "Could not begin program editing.",
              ),
            );
        }
      } else if (
        event.data.method === "plugin.edit_program_field" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id &&
        typeof params.field_id === "string" &&
        findEditorField(draft.editor.pages, params.field_id) &&
        isProgramEditorValue(params.value)
      ) {
        dispatchCommand({
          type: "edit_program_draft_field",
          draft_id: draft.draft_id,
          field_id: params.field_id,
          value: params.value,
          preview: params.preview === true,
        });
        respond(true);
      } else if (
        event.data.method === "plugin.set_program_name" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id &&
        validPluginProgramName(params.name)
      ) {
        try {
          const document = JSON.parse(draft.document_json) as Record<
            string,
            unknown
          >;
          document.name = params.name.trim();
          dispatchCommand({
            type: "replace_program_draft",
            draft_id: draft.draft_id,
            document_json: JSON.stringify(document),
          });
          respond(true);
        } catch {
          respond(false, "The active program document is invalid.");
        }
      } else if (
        event.data.method === "plugin.replace_program_draft" &&
        surface === "config" &&
        !isolated &&
        draft &&
        params.draft_id === draft.draft_id &&
        params.document &&
        typeof params.document === "object" &&
        !Array.isArray(params.document)
      ) {
        try {
          const documentJson = JSON.stringify(params.document);
          if (new TextEncoder().encode(documentJson).byteLength > 16_384) {
            respond(false, "The program document exceeds the plugin transfer limit.");
          } else {
            dispatchCommandAwait({
              type: "replace_program_draft",
              draft_id: draft.draft_id,
              document_json: documentJson,
            })
              .then(() => respond(true))
              .catch((error: unknown) =>
                respond(
                  false,
                  error instanceof Error
                    ? error.message
                    : "Could not replace the program draft.",
                ),
              );
          }
        } catch {
          respond(false, "The imported program document is not valid JSON.");
        }
      } else if (
        event.data.method === "plugin.save_program" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id
      ) {
        dispatchCommandAwait({
          type: "save_program_draft",
          draft_id: draft.draft_id,
        })
          .then(() => respond(true))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not save this program.",
            ),
          );
      } else if (
        event.data.method === "plugin.cancel_program" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id
      ) {
        dispatchCommandAwait({
          type: "cancel_program_edit",
          draft_id: draft.draft_id,
        })
          .then(() => respond(true))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not cancel program editing.",
            ),
          );
      } else if (
        event.data.method === "plugin.restore_program_preview" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id
      ) {
        dispatchCommand({
          type: "restore_program_draft_preview",
          draft_id: draft.draft_id,
        });
        respond(true);
      } else {
        respond(false, "Method is not available for this plugin surface.");
      }
    };
    const onLoad = () => {
      if (pluginContextReady) send(pluginContext);
    };
    window.addEventListener("message", onMessage);
    frame.addEventListener("load", onLoad);
    if (pluginContextReady) send(pluginContext);
    return () => {
      window.removeEventListener("message", onMessage);
      frame.removeEventListener("load", onLoad);
    };
  }, [
    descriptor,
    ensureIsolatedState,
    instance,
    isolated,
    isolatedContextState,
    onSelectSound,
    onSurfaceInfoChange,
    publishIsolatedState,
    queueIsolatedParameterWrite,
    selectedSurface,
    snapshot,
    surface,
    surfaceIdentity,
    pluginContext,
    pluginContextReady,
  ]);

  const editLease =
    (surface === "play" || surface === "config") &&
    snapshot?.program_draft?.instance_id === instance.instance_id &&
    snapshot.audition?.instance_id === instance.instance_id
      ? snapshot.audition.lease_id
      : null;

  useEffect(() => {
    if (editLease === null) return;
    const keepAlive = () =>
      dispatchCommand({ type: "keep_audition_alive", lease_id: editLease });
    keepAlive();
    const timer = window.setInterval(keepAlive, 5000);
    return () => window.clearInterval(timer);
  }, [editLease]);

  if (surface === "config" && !instance.config_available) {
    return (
      <PluginSurfaceState
        title="CONFIG mode unavailable"
        detail={`${instance.plugin_name} uses PLAY as its complete editor. Save and restore its state with RackForge presets.`}
      />
    );
  }

  if (descriptorStatus === "loading") {
    return (
      <RfLoader
        className="plugin-surface-loader"
        label={instance.plugin_name}
        detail="Loading plugin interface…"
        size="medium"
      />
    );
  }
  if (descriptorStatus === "error") {
    return (
      <PluginSurfaceState
        title="Plugin web view could not be loaded"
        detail="RackForge could not read this plugin's web manifest."
      />
    );
  }
  if (descriptorStatus === "unavailable" || !selectedSurface) {
    return (
      <PluginSurfaceState
        title="Web view unavailable"
        detail={
          descriptor?.web_ui_unavailable_reason ?? (descriptor
            ? `${instance.plugin_name} does not provide a ${surface.toUpperCase()} web view.`
            : `${instance.plugin_name} does not include a RackForge Web interface.`)
        }
      />
    );
  }
  const finishResourceSelection = (
    ok: boolean,
    error?: string,
    grant?: ResourceGrant,
  ) => {
    const requestId = resourceRequest?.requestId;
    if (!requestId) return;
    sendPluginResponse(requestId, ok, error, grant);
    pendingResourceRequestRef.current = null;
    setResourceRequest(null);
  };

  return (
    <>
      <div
        className="plugin-frame-stage"
        style={descriptor?.branding?.background_color ? {
          backgroundColor: descriptor.branding.background_color,
        } : undefined}
      >
        <iframe
          key={selectedSurface.entry_url}
          ref={frameRef}
          className={`plugin-frame${frameLoaded ? " loaded" : ""}`}
          title={`${instance.plugin_name} ${surface}`}
          src={selectedSurface.entry_url}
          sandbox="allow-scripts allow-same-origin allow-downloads"
          referrerPolicy="same-origin"
          onLoad={() => {
            setLoadedFrameIdentity(surfaceIdentity);
            setFrameDocumentGeneration((generation) => generation + 1);
            // Plugin surfaces are same-origin, sandboxed documents. Give them
            // RackForge's low-specificity scrollbar defaults while allowing a
            // plugin stylesheet to replace the theme deliberately.
            try {
              const frameDocument = frameRef.current?.contentDocument;
              if (
                frameDocument?.head &&
                !frameDocument.querySelector("link[data-rackforge-scrollbars]")
              ) {
                const link = frameDocument.createElement("link");
                link.rel = "stylesheet";
                link.href = new URL(
                  "rackforge-scrollbars.css",
                  window.document.baseURI,
                ).href;
                link.dataset.rackforgeScrollbars = "true";
                frameDocument.head.append(link);
              }
            } catch {
              // A plugin that intentionally navigates away from the host
              // origin remains isolated and simply keeps its own scrollbar.
            }
            // A cached plugin can post `ready` before React's effect installs
            // the message listener. The load event happens after the plugin
            // has installed its own listener, so publishing the idempotent
            // context here closes that race without plugin-specific timing.
            if (pluginContextReady) {
              frameRef.current?.contentWindow?.postMessage(
                pluginContext,
                window.location.origin,
              );
            }
          }}
        />
        {!splashGone && (
          <div
            className={`plugin-brand-splash${splashDone ? " done" : ""}`}
            aria-label={`Loading ${instance.plugin_name}`}
            onTransitionEnd={(event) => {
              if (event.target === event.currentTarget && splashDone) {
                setHiddenSplashIdentity(surfaceIdentity);
              }
            }}
          >
            {descriptor?.branding ? (
              <>
                <BrandSplashArtwork
                  key={surfaceIdentity}
                  splashUrl={descriptor.branding.splash_url}
                  iconUrl={descriptor.branding.icon_url}
                  litRef={splashLitRef}
                />
              </>
            ) : (
              <RfLoader label={instance.plugin_name} detail="Loading plugin interface…" size="medium" />
            )}
          </div>
        )}
        {resourceBusy ? (
          <div className="plugin-operation-overlay" role="status" aria-live="polite">
            <AsyncSpinner label={resourceBusy} size="large" />
            <strong>{resourceBusy}</strong>
            <small>RackForge is validating and applying the selected file.</small>
          </div>
        ) : null}
        {isolatedBootstrapError ? (
          <div className="plugin-operation-overlay plugin-operation-error" role="alert">
            <strong>Rack Slot instrument could not start</strong>
            <small>{isolatedBootstrapError}</small>
          </div>
        ) : null}
      </div>
      <ParameterLinkHost
        frameRef={frameRef}
        frameLoaded={frameLoaded}
        frameDocumentGeneration={frameDocumentGeneration}
        instanceId={parameterLinkInstanceId ?? instance.instance_id}
        links={snapshot?.parameter_links ?? []}
        loadParameters={loadParameterSchemaForLink}
        resetParameter={resetParameterForLink}
      />
      {resourceRequest ? (
        <Suspense
          fallback={
            <div className="resource-explorer-backdrop">
              <RfLoader label="RackForge storage" detail="Opening explorer…" />
            </div>
          }
        >
          <ResourceExplorerDialog
            pluginId={instance.plugin_id}
            resource={resourceRequest.resource}
            onCancel={() =>
              finishResourceSelection(
                false,
                "Resource selection was cancelled by the user.",
              )
            }
            onBound={(grant) => finishResourceSelection(true, undefined, grant)}
          />
        </Suspense>
      ) : null}
    </>
  );
}
