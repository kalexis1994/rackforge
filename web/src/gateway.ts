import {
  connectionChanged,
  errorReceived,
  hostIdleReceived,
  performanceEditStarted,
  performanceReceived,
  snapshotReceived,
  store,
} from "./store";
import { isVstHost, openSessionChannel, type SessionChannel } from "./host";
import { randomIdToken } from "./ids";
import { invalidatePluginCatalog } from "./pluginCatalog";
import { serializeSessionCommand } from "./sessionCommandProtocol";
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
  OutputMeterMessage,
  OutputMeterSnapshot,
  ParameterLink,
  SessionSnapshot,
  SessionCommand,
} from "./types";

function createClientId() {
  return `web.touch.${randomIdToken()}`;
}

const CLIENT_ID = createClientId();
const RECONNECT_DELAY_MS = 1200;
const PERFORMANCE_REFRESH_MS = 2000;
const OUTPUT_METER_REFRESH_MS = 50;
const COMMAND_TIMEOUT_MS = 8_000;

let socket: SessionChannel | null = null;
let sessionConnected = false;
let sessionConnecting = false;
let coreReady = false;
let commandId = 0;
let reconnectTimer: number | null = null;
let performanceTimer: number | null = null;
let outputMeterTimer: number | null = null;
let gatewayGeneration = 0;
let performanceSnapshotInFlight = false;
let outputMeterInFlight = false;
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
      outputMeterTimer = window.setInterval(
        sendOutputMeterRequest,
        OUTPUT_METER_REFRESH_MS,
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
        } else if (message.status === "output_meter" && "meter" in message) {
          outputMeterInFlight = false;
          const meterMessage = message as unknown as OutputMeterMessage;
          for (const listener of outputMeterListeners) listener(meterMessage.meter);
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
          performanceSnapshotInFlight = false;
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
      if (generation !== gatewayGeneration) return;
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
      pendingPerformanceEdit?.reject(
        new Error("The RackForge Core connection was interrupted."),
      );
      pendingPerformanceEdit = null;
      if (pendingPresetRequest?.timeout !== undefined) {
        window.clearTimeout(pendingPresetRequest.timeout);
      }
      pendingPresetRequest?.reject(
        new Error("The RackForge Core connection was interrupted."),
      );
      pendingPresetRequest = null;
      rejectPendingCommands(
        new Error("The RackForge Core connection was interrupted."),
      );
      rejectSnapshotRefreshes(
        new Error("The RackForge Core connection was interrupted."),
      );
      for (const queued of presetRequestQueue.splice(0)) {
        queued.reject(new Error("The RackForge Core connection was interrupted."));
      }
      store.dispatch(connectionChanged("offline"));
      scheduleReconnect();
    },
    onError: () => {
      if (generation !== gatewayGeneration) return;
      store.dispatch(errorReceived("The RackForge Core connection was interrupted."));
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
  if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
  if (performanceTimer !== null) window.clearInterval(performanceTimer);
  if (outputMeterTimer !== null) window.clearInterval(outputMeterTimer);
  reconnectTimer = null;
  performanceTimer = null;
  outputMeterTimer = null;
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
  const interruption = new Error("The RackForge Core connection was interrupted.");
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

function sendOutputMeterRequest() {
  if (!isVstHost() && socket && sessionConnected && coreReady && !outputMeterInFlight) {
    outputMeterInFlight = true;
    socket.send(JSON.stringify({ op: "output_meter" }));
  }
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
