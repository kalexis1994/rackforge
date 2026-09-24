import {
  connectionChanged,
  errorReceived,
  hostIdleReceived,
  performanceEditStarted,
  performanceReceived,
  snapshotReceived,
  store,
} from "./store";
import { IS_BROWSER_HOST, isVstHost, openSessionChannel, type SessionChannel } from "./host";
import { randomIdToken } from "./ids";
import { invalidatePluginCatalog } from "./pluginCatalog";
import { serializeSessionCommand } from "./sessionCommandProtocol";
import type { SequencerCommand, SequencerStatus } from "./sequencer";
import {
  CONNECTION_INTERRUPTED_MESSAGE,
  DeferredConnectionOutage,
} from "./connectionOutage";
import type {
  CoreCommandAppliedMessage,
  CoreErrorMessage,
  CoreSnapshotMessage,
  HostPreset,
  HostPresetSummary,
  PerformanceEdit,
  PerformanceSnapshot,
  PerformanceSnapshotMessage,
  PluginParameterSnapshot,
  PluginStateParameterResult,
  PluginStateParameterSnapshot,
  PluginStateReference,
  MidiLearnCandidate,
  MidiSourceStatus,
  AudioHealthMessage,
  AudioHealthSnapshot,
  AudioInputStatus,
  OutputMeterMessage,
  OutputMeterSnapshot,
  ParameterLink,
  ControllerMap,
  MidiActivityEvent,
  RegisteredController,
  RfMapFile,
  SessionSnapshot,
  SessionCommand,
} from "./types";

function createClientId() {
  return `web.touch.${randomIdToken()}`;
}

const CLIENT_ID = createClientId();
const RECONNECT_DELAY_MS = 1200;
const CONNECTION_NOTICE_DELAY_MS = 4_000;
const PERFORMANCE_REFRESH_MS = 2000;
const OUTPUT_METER_REFRESH_MS = 50;
// Ten frames a second reads as a moving clock without competing with audio.
const SEQUENCER_STATUS_REFRESH_MS = 100;
const COMMAND_TIMEOUT_MS = 8_000;

let socket: SessionChannel | null = null;
let sessionConnected = false;
let sessionConnecting = false;
let coreReady = false;
let commandId = 0;
let reconnectTimer: number | null = null;
let performanceTimer: number | null = null;
let outputMeterTimer: number | null = null;
let sequencerStatusTimer: number | null = null;
let sequencerStatusInFlight = false;
let gatewayGeneration = 0;
let performanceSnapshotInFlight = false;
let outputMeterInFlight = false;
/// When the outstanding poll was sent. A latch that is only cleared by a
/// matching reply stops the poller for good if that reply never arrives, so
/// a latch older than this is treated as lost and the poll is reissued. The
/// requests are idempotent reads, so reissuing one costs nothing.
let outputMeterSentAt = 0;
let audioHealthInFlight = false;
let audioHealthSentAt = 0;
let sequencerStatusSentAt = 0;
const POLL_LATCH_STALE_MS = 2000;

function latchIsStale(sentAt: number) {
  return sentAt !== 0 && Date.now() - sentAt > POLL_LATCH_STALE_MS;
}
let intentionallyStopped = false;
let pendingPerformanceEdit:
  | {
      resolve: (snapshot: PerformanceSnapshot) => void;
      reject: (error: Error) => void;
      request: {
        expected_revision: string;
        edit: PerformanceEdit;
      } | null;
    }
  | null = null;
let pendingPresetRequest:
  | {
      expected: string;
      resolve: (message: Record<string, unknown>) => void;
      reject: (error: Error) => void;
      timeout?: number;
    }
  | null = null;
const presetRequestQueue: Array<{
  request: Record<string, unknown>;
  expected: string;
  resolve: (message: Record<string, unknown>) => void;
  reject: (error: Error) => void;
  timeout?: number;
}> = [];
const pendingCommands = new Map<number, {
  resolve: (message: CoreCommandAppliedMessage) => void;
  reject: (error: Error) => void;
  timeout: number;
  applied?: CoreCommandAppliedMessage;
}>();
const pendingSnapshotRefreshes = new Set<{
  resolve: (snapshot: SessionSnapshot) => void;
  reject: (error: Error) => void;
  timeout: number;
}>();
const outputMeterListeners = new Set<(meter: OutputMeterSnapshot) => void>();
/*
 * MIDI activity for the Controllers editor: polled beside the output meter,
 * with a latch of its own, never through the one-at-a-time request queue --
 * a slow import there must not freeze the lights. `null` means the next ask
 * only learns where the host's log stands, so messages the host kept from
 * before anyone watched do not light anything.
 */
const midiActivityListeners = new Set<(events: MidiActivityEvent[]) => void>();
let midiActivityCursor: number | null = null;
let midiActivityInFlight = false;
let midiActivitySentAt = 0;
const audioHealthListeners = new Set<(health: AudioHealthSnapshot) => void>();
const sequencerStatusListeners = new Set<(status: SequencerStatus) => void>();
const connectionOutage = new DeferredConnectionOutage(
  CONNECTION_NOTICE_DELAY_MS,
  () => {
    if (!intentionallyStopped && !sessionConnected) {
      store.dispatch(errorReceived(CONNECTION_INTERRUPTED_MESSAGE));
    }
  },
);

function resolveSnapshotRefreshes(snapshot: SessionSnapshot) {
  for (const pending of pendingSnapshotRefreshes) {
    window.clearTimeout(pending.timeout);
    pending.resolve(snapshot);
  }
  pendingSnapshotRefreshes.clear();
}

function rejectSnapshotRefreshes(error: Error) {
  for (const pending of pendingSnapshotRefreshes) {
    window.clearTimeout(pending.timeout);
    pending.reject(error);
  }
  pendingSnapshotRefreshes.clear();
}

function resolvePendingCommandsThrough(revision: number) {
  for (const [id, pending] of pendingCommands) {
    if (!pending.applied || pending.applied.revision > revision) continue;
    pendingCommands.delete(id);
    window.clearTimeout(pending.timeout);
    pending.resolve(pending.applied);
  }
}

function rejectPendingCommands(error: Error) {
  for (const pending of pendingCommands.values()) {
    window.clearTimeout(pending.timeout);
    pending.reject(error);
  }
  pendingCommands.clear();
}

function pumpPresetRequests() {
  if (pendingPresetRequest || !socket || !sessionConnected) return;
  const next = presetRequestQueue.shift();
  if (!next) return;
  pendingPresetRequest = next;
  next.timeout = window.setTimeout(() => {
    if (pendingPresetRequest !== next) return;
    pendingPresetRequest = null;
    next.reject(new Error("RackForge did not complete the preset operation in time."));
    pumpPresetRequests();
  }, 30_000);
  socket.send(JSON.stringify(next.request));
}

function scheduleReconnect() {
  if (intentionallyStopped || reconnectTimer !== null) return;
  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null;
    connectGateway();
  }, RECONNECT_DELAY_MS);
}

function disconnectGateway(generation: number) {
  if (generation !== gatewayGeneration) return null;
  if (!socket && !sessionConnected && !sessionConnecting) return null;

  const disconnectedSocket = socket;
  socket = null;
  sessionConnected = false;
  sessionConnecting = false;
  coreReady = false;
  performanceSnapshotInFlight = false;
  outputMeterInFlight = false;
  if (performanceTimer !== null) window.clearInterval(performanceTimer);
  if (outputMeterTimer !== null) window.clearInterval(outputMeterTimer);
  performanceTimer = null;
  outputMeterTimer = null;
  pendingPerformanceEdit?.reject(new Error(CONNECTION_INTERRUPTED_MESSAGE));
  pendingPerformanceEdit = null;
  if (pendingPresetRequest?.timeout !== undefined) {
    window.clearTimeout(pendingPresetRequest.timeout);
  }
  pendingPresetRequest?.reject(new Error(CONNECTION_INTERRUPTED_MESSAGE));
  pendingPresetRequest = null;
  rejectPendingCommands(new Error(CONNECTION_INTERRUPTED_MESSAGE));
  rejectSnapshotRefreshes(new Error(CONNECTION_INTERRUPTED_MESSAGE));
  for (const queued of presetRequestQueue.splice(0)) {
    queued.reject(new Error(CONNECTION_INTERRUPTED_MESSAGE));
  }
  store.dispatch(connectionChanged("offline"));
  connectionOutage.begin();
  scheduleReconnect();
  return disconnectedSocket;
}

function sendPerformanceSnapshotRequest() {
  if (
    socket &&
    sessionConnected &&
    coreReady &&
    !performanceSnapshotInFlight &&
    !pendingPerformanceEdit &&
    !store.getState().rackforge.performancePending
  ) {
    performanceSnapshotInFlight = true;
    socket.send(JSON.stringify({ op: "performance_snapshot" }));
  }
}

function sendPendingPerformanceEdit() {
  if (
    !socket ||
    !sessionConnected ||
    performanceSnapshotInFlight ||
    !pendingPerformanceEdit?.request
  ) return;
  const request = pendingPerformanceEdit.request;
  pendingPerformanceEdit.request = null;
  socket.send(JSON.stringify({ op: "edit_performance", ...request }));
}

export function connectGateway() {
  if (sessionConnected || sessionConnecting) return;

  intentionallyStopped = false;
  sessionConnecting = true;
  const generation = ++gatewayGeneration;
  store.dispatch(connectionChanged("connecting"));
  socket = openSessionChannel({
    onOpen: () => {
      if (generation !== gatewayGeneration) return;
      connectionOutage.recover();
      sessionConnecting = false;
      sessionConnected = true;
      performanceSnapshotInFlight = false;
      outputMeterInFlight = false;
      store.dispatch(connectionChanged("online"));
      void invalidatePluginCatalog().catch(() => undefined);
      if (performanceTimer !== null) window.clearInterval(performanceTimer);
      performanceTimer = window.setInterval(
        sendPerformanceSnapshotRequest,
        PERFORMANCE_REFRESH_MS,
      );
      if (outputMeterTimer !== null) window.clearInterval(outputMeterTimer);
      midiActivityInFlight = false;
      midiActivityCursor = null;
      outputMeterTimer = window.setInterval(
        () => {
          sendOutputMeterRequest();
          sendAudioHealthRequest();
          sendMidiActivityRequest();
        },
        OUTPUT_METER_REFRESH_MS,
      );
      if (sequencerStatusTimer !== null) window.clearInterval(sequencerStatusTimer);
      sequencerStatusTimer = window.setInterval(
        sendSequencerStatusRequest,
        SEQUENCER_STATUS_REFRESH_MS,
      );
    },
    onMessage: (payload) => {
      if (generation !== gatewayGeneration) return;
      try {
        const message = JSON.parse(payload) as Record<string, unknown>;
        if (
          message.status === "command_applied" &&
          message.client_id === CLIENT_ID &&
          typeof message.command_id === "number"
        ) {
          const pending = pendingCommands.get(message.command_id);
          if (pending) {
            pending.applied = message as unknown as CoreCommandAppliedMessage;
            const snapshotRevision = store.getState().rackforge.snapshot?.revision;
            if (
              typeof snapshotRevision === "number" &&
              snapshotRevision >= pending.applied.revision
            ) {
              resolvePendingCommandsThrough(snapshotRevision);
            }
          }
        } else if (message.status === "snapshot" && "snapshot" in message) {
          coreReady = true;
          const snapshotMessage = message as unknown as CoreSnapshotMessage;
          store.dispatch(snapshotReceived(snapshotMessage.snapshot));
          resolveSnapshotRefreshes(snapshotMessage.snapshot);
          resolvePendingCommandsThrough(snapshotMessage.snapshot.revision);
          sendPerformanceSnapshotRequest();
        } else if (message.status === "host_idle") {
          coreReady = false;
          store.dispatch(hostIdleReceived());
        } else if (message.status === "sequencer_status" && "sequencer" in message) {
          sequencerStatusInFlight = false;
          sequencerStatusSentAt = 0;
          const status = (message as unknown as { sequencer: SequencerStatus }).sequencer;
          for (const listener of sequencerStatusListeners) listener(status);
        } else if (message.status === "audio_health" && "health" in message) {
          audioHealthInFlight = false;
          audioHealthSentAt = 0;
          const healthMessage = message as unknown as AudioHealthMessage;
          for (const listener of audioHealthListeners) listener(healthMessage.health);
        } else if (message.status === "output_meter" && "meter" in message) {
          outputMeterInFlight = false;
          outputMeterSentAt = 0;
          const meterMessage = message as unknown as OutputMeterMessage;
          for (const listener of outputMeterListeners) listener(meterMessage.meter);
        } else if (message.status === "midi_activity" && "cursor" in message) {
          midiActivityInFlight = false;
          midiActivitySentAt = 0;
          const baseline = midiActivityCursor === null;
          midiActivityCursor = Number(message.cursor) || 0;
          const events = (message.events ?? []) as MidiActivityEvent[];
          if (!baseline && events.length > 0) {
            for (const listener of midiActivityListeners) listener(events);
          }
        } else if (message.status === "core_restarting") {
          coreReady = false;
          store.dispatch(connectionChanged("connecting"));
        } else if (message.status === "plugin_catalog_changed") {
          void invalidatePluginCatalog().catch(() => undefined);
        } else if (
          typeof message.status === "string" &&
          pendingPresetRequest?.expected === message.status
        ) {
          if (pendingPresetRequest.timeout !== undefined) {
            window.clearTimeout(pendingPresetRequest.timeout);
          }
          pendingPresetRequest.resolve(message);
          pendingPresetRequest = null;
          pumpPresetRequests();
        } else if (
          (message.status === "performance_snapshot" ||
            message.status === "performance_edited") &&
          "snapshot" in message
        ) {
          const performanceMessage =
            message as unknown as PerformanceSnapshotMessage;
          if (message.status === "performance_snapshot") {
            performanceSnapshotInFlight = false;
          }
          store.dispatch(
            performanceReceived({
              snapshot: performanceMessage.snapshot,
              edited: message.status === "performance_edited",
            }),
          );
          if (message.status === "performance_edited") {
            pendingPerformanceEdit?.resolve(performanceMessage.snapshot);
            pendingPerformanceEdit = null;
          } else {
            sendPendingPerformanceEdit();
          }
        } else if (
          (message.status === "error" || message.status === "gateway_error") &&
          "message" in message
        ) {
          const errorMessage = message as unknown as CoreErrorMessage;
          store.dispatch(errorReceived(errorMessage.message));
          rejectPendingCommands(new Error(errorMessage.message));
          pendingPerformanceEdit?.reject(new Error(errorMessage.message));
          pendingPerformanceEdit = null;
          // Every poller's in-flight latch has to clear here, not just the
          // performance snapshot's. The Desktop answers a request it could
          // not serve with {"status":"error"}, which matches none of the
          // per-poller branches above, so a latch left set here never clears
          // and that poller stops for the rest of the session. The output
          // meter latching was visible as the OUT bar freezing after a
          // hiccup and never moving again.
          performanceSnapshotInFlight = false;
          outputMeterInFlight = false;
          outputMeterSentAt = 0;
          audioHealthInFlight = false;
          audioHealthSentAt = 0;
          sequencerStatusInFlight = false;
          sequencerStatusSentAt = 0;
          midiActivityInFlight = false;
          midiActivitySentAt = 0;
          if (pendingPresetRequest?.timeout !== undefined) {
            window.clearTimeout(pendingPresetRequest.timeout);
          }
          pendingPresetRequest?.reject(new Error(errorMessage.message));
          pendingPresetRequest = null;
          pumpPresetRequests();
          sendPerformanceSnapshotRequest();
        }
      } catch {
        store.dispatch(errorReceived("RackForge returned an unreadable response."));
      }
    },
    onClose: () => {
      disconnectGateway(generation);
    },
    onError: () => {
      // Native bridges do not necessarily emit a second close event when a
      // session request fails. Normalize both transports through the same
      // teardown/reconnect path and let the grace period decide whether the
      // interruption deserves a user-facing banner.
      disconnectGateway(generation)?.close();
    },
  });
}

function requestPresetOperation<T>(
  request: Record<string, unknown>,
  expected: string,
  decode: (message: Record<string, unknown>) => T,
): Promise<T> {
  if (!socket || !sessionConnected) {
    return Promise.reject(new Error("RackForge Core is not connected."));
  }
  return new Promise((resolve, reject) => {
    presetRequestQueue.push({
      request,
      expected,
      resolve: (message) => resolve(decode(message)),
      reject,
    });
    pumpPresetRequests();
  });
}

export function requestPluginPresets(pluginId: string): Promise<HostPresetSummary[]> {
  return requestPresetOperation(
    { op: "plugin_presets", plugin_id: pluginId },
    "plugin_presets",
    (message) => (message.presets ?? []) as HostPresetSummary[],
  );
}

export function requestPluginPreset(
  pluginId: string,
  presetId: string,
): Promise<HostPreset> {
  return requestPresetOperation(
    { op: "plugin_preset", plugin_id: pluginId, preset_id: presetId },
    "plugin_preset",
    (message) => message.preset as HostPreset,
  );
}

export function materializePluginState(
  pluginId: string,
  soundId?: string,
): Promise<PluginStateReference> {
  return requestPresetOperation(
    {
      op: "materialize_plugin_state",
      plugin_id: pluginId,
      ...(soundId ? { sound_id: soundId } : {}),
    },
    "plugin_state_materialized",
    (message) => message.state as PluginStateReference,
  );
}

export function savePluginPreset(instanceId: string, name: string): Promise<HostPreset> {
  return requestPresetOperation(
    { op: "save_plugin_preset", instance_id: instanceId, name },
    "plugin_preset_saved",
    (message) => message.preset as HostPreset,
  );
}

export function loadPluginPreset(instanceId: string, presetId: string): Promise<HostPreset> {
  return requestPresetOperation(
    { op: "load_plugin_preset", instance_id: instanceId, preset_id: presetId },
    "plugin_preset_loaded",
    (message) => message.preset as HostPreset,
  );
}

export function renamePluginPreset(
  pluginId: string,
  presetId: string,
  name: string,
): Promise<HostPreset> {
  return requestPresetOperation(
    { op: "rename_plugin_preset", plugin_id: pluginId, preset_id: presetId, name },
    "plugin_preset_renamed",
    (message) => message.preset as HostPreset,
  );
}

export function deletePluginPreset(
  pluginId: string,
  presetId: string,
): Promise<string> {
  return requestPresetOperation(
    { op: "delete_plugin_preset", plugin_id: pluginId, preset_id: presetId },
    "plugin_preset_deleted",
    (message) => String(message.preset_id),
  );
}

export function requestSequencerCaptureTake(
  lane: number,
): Promise<{ notes: import("./sequencer").CapturedNote[] }> {
  return requestPresetOperation(
    { op: "sequencer_capture_take", lane },
    "sequencer_capture",
    (message) => message as unknown as { notes: import("./sequencer").CapturedNote[] },
  );
}

export function requestPluginParameters(
  instanceId: string,
): Promise<PluginParameterSnapshot> {
  return requestPresetOperation(
    { op: "plugin_parameters", instance_id: instanceId },
    "plugin_parameters",
    (message) => message as unknown as PluginParameterSnapshot,
  );
}

export function setPluginParameter(
  instanceId: string,
  parameterIndex: number,
  value: number,
): Promise<number> {
  return requestPresetOperation(
    {
      op: "set_plugin_parameter",
      instance_id: instanceId,
      parameter_index: parameterIndex,
      value,
    },
    "plugin_parameter_set",
    (message) => Number(message.value),
  );
}

export function requestPluginStateParameters(
  state: PluginStateReference,
): Promise<PluginStateParameterSnapshot> {
  return requestPresetOperation(
    { op: "plugin_state_parameters", state },
    "plugin_state_parameters",
    (message) => message as unknown as PluginStateParameterSnapshot,
  );
}

export function setPluginStateParameter(
  state: PluginStateReference,
  parameterIndex: number,
  value: number,
): Promise<PluginStateParameterResult> {
  return requestPresetOperation(
    {
      op: "set_plugin_state_parameter",
      state,
      parameter_index: parameterIndex,
      value,
    },
    "plugin_state_parameter_set",
    (message) => message as unknown as PluginStateParameterResult,
  );
}

export function requestSessionSnapshot(): Promise<SessionSnapshot> {
  if (!socket || !sessionConnected) {
    return Promise.reject(new Error("RackForge Core is not connected."));
  }
  return new Promise((resolve, reject) => {
    const pending = {
      resolve,
      reject,
      timeout: window.setTimeout(() => {
        pendingSnapshotRefreshes.delete(pending);
        reject(new Error("RackForge did not refresh the session in time."));
      }, COMMAND_TIMEOUT_MS),
    };
    pendingSnapshotRefreshes.add(pending);
    try {
      socket?.send(JSON.stringify({ op: "snapshot" }));
    } catch (error) {
      pendingSnapshotRefreshes.delete(pending);
      window.clearTimeout(pending.timeout);
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

export function stopGateway() {
  intentionallyStopped = true;
  gatewayGeneration += 1;
  connectionOutage.recover();
  if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
  if (performanceTimer !== null) window.clearInterval(performanceTimer);
  if (outputMeterTimer !== null) window.clearInterval(outputMeterTimer);
  if (sequencerStatusTimer !== null) window.clearInterval(sequencerStatusTimer);
  reconnectTimer = null;
  performanceTimer = null;
  outputMeterTimer = null;
  sequencerStatusTimer = null;
  sequencerStatusInFlight = false;
  releaseVirtualMidi();
  const closingSocket = socket;
  socket = null;
  sessionConnected = false;
  sessionConnecting = false;
  coreReady = false;
  performanceSnapshotInFlight = false;
  outputMeterInFlight = false;
  for (const listener of outputMeterListeners) {
    listener({ left_peak: 0, right_peak: 0 });
  }
  const interruption = new Error(CONNECTION_INTERRUPTED_MESSAGE);
  pendingPerformanceEdit?.reject(interruption);
  pendingPerformanceEdit = null;
  if (pendingPresetRequest?.timeout !== undefined) {
    window.clearTimeout(pendingPresetRequest.timeout);
  }
  pendingPresetRequest?.reject(interruption);
  pendingPresetRequest = null;
  for (const queued of presetRequestQueue.splice(0)) queued.reject(interruption);
  rejectPendingCommands(interruption);
  rejectSnapshotRefreshes(interruption);
  closingSocket?.close();
}

export function sendVirtualMidi(status: number, data1: number, data2: number) {
  if (!socket || !sessionConnected) return false;
  if (
    !Number.isInteger(status)
    || !Number.isInteger(data1)
    || !Number.isInteger(data2)
    || status < 0x80
    || status > 0xbf
    || data1 < 0
    || data1 > 127
    || data2 < 0
    || data2 > 127
  ) {
    return false;
  }
  socket.send(JSON.stringify({
    op: "virtual_midi",
    client_id: CLIENT_ID,
    message: { status, data1, data2 },
  }));
  return true;
}

export function releaseVirtualMidi() {
  if (!socket || !sessionConnected) return;
  socket.send(JSON.stringify({
    op: "release_virtual_midi",
    client_id: CLIENT_ID,
  }));
}

/// Dispatches an edit against the revision the store holds RIGHT NOW,
/// waiting out any in-flight edit and retrying once on a conflict. For
/// last-writer-wins documents (sequencer tabs); revisioned callers that
/// must detect races keep using dispatchPerformanceEdit directly.
export async function dispatchPerformanceEditLatest(
  edit: PerformanceEdit,
): Promise<PerformanceSnapshot> {
  for (let attempt = 0; ; attempt += 1) {
    // An edit already in flight resolves quickly; give it room instead of
    // being refused by the pending gate.
    for (let waited = 0; store.getState().rackforge.performancePending && waited < 20; waited += 1) {
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    const revision = store.getState().rackforge.performance?.revision;
    if (!revision) throw new Error("The performance library is not loaded yet.");
    try {
      return await dispatchPerformanceEdit(revision, edit);
    } catch (error) {
      if (attempt >= 2) throw error;
    }
  }
}

export function dispatchPerformanceEdit(
  expectedRevision: string,
  edit: PerformanceEdit,
): Promise<PerformanceSnapshot> {
  if (!socket || !sessionConnected) {
    const message = "RackForge Core is not connected.";
    store.dispatch(errorReceived(message));
    return Promise.reject(new Error(message));
  }
  if (store.getState().rackforge.performancePending) {
    return Promise.reject(new Error("Another performance edit is still saving."));
  }
  store.dispatch(performanceEditStarted());
  return new Promise((resolve, reject) => {
    pendingPerformanceEdit = {
      resolve,
      reject,
      request: {
        expected_revision: expectedRevision,
        edit,
      },
    };
    sendPendingPerformanceEdit();
  });
}

export function dispatchCommand(command: SessionCommand) {
  if (!socket || !sessionConnected) {
    store.dispatch(errorReceived("RackForge Core is not connected."));
    return;
  }
  commandId += 1;
  try {
    socket.send(commandPayload(commandId, command));
  } catch (reason) {
    store.dispatch(errorReceived(
      reason instanceof Error ? reason.message : "Could not send the RackForge command.",
    ));
  }
}

function commandPayload(id: number, command: SessionCommand) {
  return serializeSessionCommand(CLIENT_ID, id, command);
}

function sendAudioHealthRequest() {
  // The browser host has no audio driver to report on and answers
  // audio_health as unavailable. Session errors carry no request id, so that
  // answer rejected whatever command was waiting beside it: switching
  // instrument in the browser failed with "does not implement this request"
  // though the switch itself had worked.
  if (
    !isVstHost()
    && !IS_BROWSER_HOST
    && socket
    && sessionConnected
    && coreReady
    && (!audioHealthInFlight || latchIsStale(audioHealthSentAt))
    && audioHealthListeners.size > 0
  ) {
    audioHealthInFlight = true;
    audioHealthSentAt = Date.now();
    socket.send(JSON.stringify({ op: "audio_health" }));
  }
}

function sendOutputMeterRequest() {
  if (
    !isVstHost()
    && socket
    && sessionConnected
    && coreReady
    && (!outputMeterInFlight || latchIsStale(outputMeterSentAt))
  ) {
    outputMeterInFlight = true;
    outputMeterSentAt = Date.now();
    socket.send(JSON.stringify({ op: "output_meter" }));
  }
}

function sendMidiActivityRequest() {
  if (
    !isVstHost()
    && socket
    && sessionConnected
    && coreReady
    && midiActivityListeners.size > 0
    && (!midiActivityInFlight || latchIsStale(midiActivitySentAt))
  ) {
    midiActivityInFlight = true;
    midiActivitySentAt = Date.now();
    socket.send(JSON.stringify({
      op: "midi_activity",
      // The first ask only finds where the log stands.
      after: midiActivityCursor ?? Number.MAX_SAFE_INTEGER,
    }));
  }
}

/**
 * Every channel message the host receives from now on, in batches as they
 * are polled. Watching starts afresh: what the host kept from before is not
 * delivered.
 */
export function subscribeMidiActivity(listener: (events: MidiActivityEvent[]) => void) {
  if (midiActivityListeners.size === 0) midiActivityCursor = null;
  midiActivityListeners.add(listener);
  return () => {
    midiActivityListeners.delete(listener);
  };
}

function sendSequencerStatusRequest() {
  if (
    !isVstHost()
    && socket
    && sessionConnected
    && coreReady
    && (!sequencerStatusInFlight || latchIsStale(sequencerStatusSentAt))
    && sequencerStatusListeners.size > 0
  ) {
    sequencerStatusInFlight = true;
    sequencerStatusSentAt = Date.now();
    socket.send(JSON.stringify({ op: "sequencer_status" }));
  }
}

/** Sends one sequencer instruction. Fire-and-forget by design: quantise
 * boundaries are resolved by the host transport, so nothing here waits. */
export function sendSequencerCommand(command: SequencerCommand): boolean {
  if (!socket || !sessionConnected || !coreReady) return false;
  socket.send(JSON.stringify({ op: "sequencer", command }));
  return true;
}

export function subscribeSequencerStatus(listener: (status: SequencerStatus) => void) {
  sequencerStatusListeners.add(listener);
  return () => {
    sequencerStatusListeners.delete(listener);
  };
}

export function subscribeAudioHealth(listener: (health: AudioHealthSnapshot) => void) {
  audioHealthListeners.add(listener);
  return () => {
    audioHealthListeners.delete(listener);
  };
}

export function subscribeOutputMeter(listener: (meter: OutputMeterSnapshot) => void) {
  outputMeterListeners.add(listener);
  return () => {
    outputMeterListeners.delete(listener);
  };
}

export function exportPluginPreset(
  pluginId: string,
  presetId: string,
): Promise<{ file_name: string; file: import("./types").RfPresetFile }> {
  return requestPresetOperation(
    { op: "export_plugin_preset", plugin_id: pluginId, preset_id: presetId },
    "plugin_preset_exported",
    (message) => ({
      file_name: String(message.file_name),
      file: message.file as import("./types").RfPresetFile,
    }),
  );
}

export function inspectPluginPreset(
  targetPluginId: string,
  file: import("./types").RfPresetFile,
): Promise<import("./types").RfPresetImportPreview> {
  return requestPresetOperation(
    { op: "inspect_plugin_preset", target_plugin_id: targetPluginId, file },
    "plugin_preset_inspected",
    (message) => message.preview as import("./types").RfPresetImportPreview,
  );
}

export function importPluginPreset(
  targetPluginId: string,
  file: import("./types").RfPresetFile,
  conflictPolicy: import("./types").PresetImportConflictPolicy,
): Promise<HostPreset> {
  return requestPresetOperation(
    {
      op: "import_plugin_preset",
      target_plugin_id: targetPluginId,
      file,
      conflict_policy: conflictPolicy,
    },
    "plugin_preset_imported",
    (message) => message.preset as HostPreset,
  );
}

export function exportLiveShow(
  name: string,
): Promise<{ file_name: string; file: import("./types").RfLiveFile }> {
  return requestPresetOperation(
    { op: "export_live_show", name },
    "live_show_exported",
    (message) => ({
      file_name: String(message.file_name),
      file: message.file as import("./types").RfLiveFile,
    }),
  );
}

export function inspectLiveShow(
  file: import("./types").RfLiveFile,
): Promise<import("./types").RfLiveImportPreview> {
  return requestPresetOperation(
    { op: "inspect_live_show", file },
    "live_show_inspected",
    (message) => message.preview as import("./types").RfLiveImportPreview,
  );
}

export function importLiveShow(
  file: import("./types").RfLiveFile,
): Promise<import("./types").RfLiveImportPreview> {
  return requestPresetOperation(
    { op: "import_live_show", file },
    "live_show_imported",
    (message) => {
      // The import already carries the resulting library; feed it to the
      // same store the edit path uses so every surface refreshes at once.
      store.dispatch(
        performanceReceived({
          snapshot: message.snapshot as PerformanceSnapshot,
          edited: false,
        }),
      );
      return message.preview as import("./types").RfLiveImportPreview;
    },
  );
}

export interface OutputCapture {
  /** Where the host saved it, on the host's own disk. */
  path: string;
  seconds: number;
  midi_messages: number;
}

/** Saves the host's flight recorder -- the last seconds of what went to the
    audio device and the MIDI that played them -- for a click heard once. */
export function saveOutputCapture(): Promise<OutputCapture> {
  return requestPresetOperation(
    { op: "save_output_capture" },
    "output_capture_saved",
    (message) => ({
      path: String(message.path ?? ""),
      seconds: Number(message.seconds ?? 0),
      midi_messages: Number(message.midi_messages ?? 0),
    }),
  );
}

/** Asks the host to show the audio driver's settings window. Resolves as
    soon as the host has accepted: the window may be modal, and what is
    changed in it comes back as the driver reopening, not as this reply. */
export function openAudioDriverPanel(): Promise<void> {
  return requestPresetOperation(
    { op: "open_audio_driver_panel" },
    "audio_driver_panel_opening",
    () => undefined,
  );
}

/** What the host captures, and the peaks of those inputs since the last
 *  request. A host that predates the question answers with an error, which
 *  is the caller's to treat as "unknown". */
export function requestAudioInput(): Promise<AudioInputStatus> {
  return requestPresetOperation(
    { op: "audio_input" },
    "audio_input",
    (message) => message.input as AudioInputStatus,
  );
}

export function requestMidiSources(): Promise<MidiSourceStatus[]> {
  return requestPresetOperation(
    { op: "midi_sources" },
    "midi_sources",
    (message) => (message.sources ?? []) as MidiSourceStatus[],
  );
}

export function beginMidiLearn(instanceId: string, parameterIndex: number): Promise<number> {
  return requestPresetOperation(
    { op: "begin_midi_learn", instance_id: instanceId, parameter_index: parameterIndex },
    "midi_learn_started",
    (message) => Number(message.learn_id),
  );
}

export function requestMidiLearnStatus(
  learnId: number,
): Promise<MidiLearnCandidate | null> {
  return requestPresetOperation(
    { op: "midi_learn_status", learn_id: learnId },
    "midi_learn_status",
    (message) => (message.candidate ?? null) as MidiLearnCandidate | null,
  );
}

export function cancelMidiLearn(learnId: number): Promise<void> {
  return requestPresetOperation(
    { op: "cancel_midi_learn", learn_id: learnId },
    "midi_learn_cancelled",
    () => undefined,
  );
}

export function requestControllerMaps(): Promise<{
  controllers: RegisteredController[];
  maps: ControllerMap[];
}> {
  return requestPresetOperation(
    { op: "controller_maps" },
    "controller_maps",
    (message) => ({
      controllers: (message.controllers ?? []) as RegisteredController[],
      maps: (message.maps ?? []) as ControllerMap[],
    }),
  );
}

/** Replaces one controller's whole map; the host applies it at once. */
export function saveControllerMap(map: ControllerMap): Promise<ControllerMap> {
  return requestPresetOperation(
    { op: "save_controller_map", map },
    "controller_map_saved",
    (message) => message.map as ControllerMap,
  );
}

/** Makes, or saves again, a controller package from the controls a player named. */
export function saveUserController(controller: {
  controller_id?: string;
  name: string;
  vendor?: string;
  endpoint_name: string;
  inputs: unknown[];
}): Promise<{ controller_id: string; version: string }> {
  return requestPresetOperation(
    { op: "save_user_controller", controller },
    "user_controller_saved",
    (message) => ({ controller_id: String(message.controller_id), version: String(message.version) }),
  );
}

export function exportControllerMap(
  controllerId: string,
): Promise<{ file_name: string; file: RfMapFile }> {
  return requestPresetOperation(
    { op: "export_controller_map", controller_id: controllerId },
    "controller_map_exported",
    (message) => ({
      file_name: String(message.file_name),
      file: message.file as RfMapFile,
    }),
  );
}

export function importControllerMap(file: RfMapFile): Promise<ControllerMap> {
  return requestPresetOperation(
    { op: "import_controller_map", file },
    "controller_map_imported",
    (message) => message.map as ControllerMap,
  );
}

export async function upsertParameterLink(link: ParameterLink): Promise<void> {
  await dispatchCommandAwait({ type: "upsert_parameter_link", link });
}

export async function removeParameterLink(linkId: string): Promise<void> {
  await dispatchCommandAwait({ type: "remove_parameter_link", link_id: linkId });
}

export function dispatchCommandAwait(
  command: SessionCommand,
): Promise<CoreCommandAppliedMessage> {
  if (!socket || !sessionConnected) {
    return Promise.reject(new Error("RackForge Core is not connected."));
  }
  commandId += 1;
  const id = commandId;
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      pendingCommands.delete(id);
      reject(new Error("RackForge Core did not confirm the command in time."));
    }, COMMAND_TIMEOUT_MS);
    pendingCommands.set(id, { resolve, reject, timeout });
    try {
      socket!.send(commandPayload(id, command));
    } catch (reason) {
      window.clearTimeout(timeout);
      pendingCommands.delete(id);
      reject(reason instanceof Error ? reason : new Error(String(reason)));
    }
  });
}
