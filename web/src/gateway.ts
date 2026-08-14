import {
  connectionChanged,
  errorReceived,
  hostIdleReceived,
  performanceEditStarted,
  performanceReceived,
  snapshotReceived,
  store,
} from "./store";
import { openSessionChannel, type SessionChannel } from "./host";
import { randomIdToken } from "./ids";
import type {
  CoreErrorMessage,
  CoreSnapshotMessage,
  HostPreset,
  HostPresetSummary,
  PerformanceEdit,
  PerformanceSnapshot,
  PerformanceSnapshotMessage,
  PluginParameterSnapshot,
  PluginStateReference,
  SessionCommand,
} from "./types";

function createClientId() {
  return `web.touch.${randomIdToken()}`;
}

const CLIENT_ID = createClientId();
const SESSION_SCHEMA_VERSION = 14;
const RECONNECT_DELAY_MS = 1200;
const PERFORMANCE_REFRESH_MS = 2000;

let socket: SessionChannel | null = null;
let sessionConnected = false;
let sessionConnecting = false;
let coreReady = false;
let commandId = 0;
let reconnectTimer: number | null = null;
let performanceTimer: number | null = null;
let gatewayGeneration = 0;
let performanceSnapshotInFlight = false;
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
    }
  | null = null;
const presetRequestQueue: Array<{
  request: Record<string, unknown>;
  expected: string;
  resolve: (message: Record<string, unknown>) => void;
  reject: (error: Error) => void;
}> = [];

function pumpPresetRequests() {
  if (pendingPresetRequest || !socket || !sessionConnected) return;
  const next = presetRequestQueue.shift();
  if (!next) return;
  pendingPresetRequest = next;
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
      store.dispatch(connectionChanged("online"));
      if (performanceTimer !== null) window.clearInterval(performanceTimer);
      performanceTimer = window.setInterval(
        sendPerformanceSnapshotRequest,
        PERFORMANCE_REFRESH_MS,
      );
    },
    onMessage: (payload) => {
      if (generation !== gatewayGeneration) return;
      try {
        const message = JSON.parse(payload) as Record<string, unknown>;
        if (message.status === "snapshot" && "snapshot" in message) {
          coreReady = true;
          const snapshotMessage = message as unknown as CoreSnapshotMessage;
          store.dispatch(snapshotReceived(snapshotMessage.snapshot));
          sendPerformanceSnapshotRequest();
        } else if (message.status === "host_idle") {
          coreReady = false;
          store.dispatch(hostIdleReceived());
        } else if (message.status === "core_restarting") {
          coreReady = false;
          store.dispatch(connectionChanged("connecting"));
        } else if (
          typeof message.status === "string" &&
          pendingPresetRequest?.expected === message.status
        ) {
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
          pendingPerformanceEdit?.reject(new Error(errorMessage.message));
          pendingPerformanceEdit = null;
          performanceSnapshotInFlight = false;
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
      if (performanceTimer !== null) window.clearInterval(performanceTimer);
      performanceTimer = null;
      pendingPerformanceEdit?.reject(
        new Error("The RackForge Core connection was interrupted."),
      );
      pendingPerformanceEdit = null;
      pendingPresetRequest?.reject(
        new Error("The RackForge Core connection was interrupted."),
      );
      pendingPresetRequest = null;
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

export function stopGateway() {
  intentionallyStopped = true;
  gatewayGeneration += 1;
  if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
  if (performanceTimer !== null) window.clearInterval(performanceTimer);
  reconnectTimer = null;
  performanceTimer = null;
  releaseVirtualMidi();
  const closingSocket = socket;
  socket = null;
  sessionConnected = false;
  sessionConnecting = false;
  coreReady = false;
  performanceSnapshotInFlight = false;
  const interruption = new Error("The RackForge Core connection was interrupted.");
  pendingPerformanceEdit?.reject(interruption);
  pendingPerformanceEdit = null;
  pendingPresetRequest?.reject(interruption);
  pendingPresetRequest = null;
  for (const queued of presetRequestQueue.splice(0)) queued.reject(interruption);
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
  socket.send(
    JSON.stringify({
      op: "dispatch",
      envelope: {
        schema_version: SESSION_SCHEMA_VERSION,
        client_id: CLIENT_ID,
        command_id: commandId,
        command,
      },
    }),
  );
}
