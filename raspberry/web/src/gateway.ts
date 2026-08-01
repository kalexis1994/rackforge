import {
  connectionChanged,
  errorReceived,
  performanceEditStarted,
  performanceReceived,
  snapshotReceived,
  store,
} from "./store";
import type {
  CoreErrorMessage,
  CoreSnapshotMessage,
  HostPreset,
  HostPresetSummary,
  PerformanceEdit,
  PerformanceSnapshot,
  PerformanceSnapshotMessage,
  SessionCommand,
} from "./types";

const CLIENT_ID = "web.main";
const SESSION_SCHEMA_VERSION = 10;
const RECONNECT_DELAY_MS = 1200;
const PERFORMANCE_REFRESH_MS = 2000;

let socket: WebSocket | null = null;
let commandId = 0;
let reconnectTimer: number | null = null;
let performanceTimer: number | null = null;
let intentionallyStopped = false;
let pendingPerformanceEdit:
  | {
      resolve: (snapshot: PerformanceSnapshot) => void;
      reject: (error: Error) => void;
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
  if (pendingPresetRequest || socket?.readyState !== WebSocket.OPEN) return;
  const next = presetRequestQueue.shift();
  if (!next) return;
  pendingPresetRequest = next;
  socket.send(JSON.stringify(next.request));
}

function socketUrl() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws/v1/session`;
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
    socket?.readyState === WebSocket.OPEN &&
    !store.getState().rackforge.performancePending
  ) {
    socket.send(JSON.stringify({ op: "performance_snapshot" }));
  }
}

export function connectGateway() {
  if (
    socket?.readyState === WebSocket.OPEN ||
    socket?.readyState === WebSocket.CONNECTING
  ) {
    return;
  }

  intentionallyStopped = false;
  store.dispatch(connectionChanged("connecting"));
  socket = new WebSocket(socketUrl());

  socket.addEventListener("open", () => {
    store.dispatch(connectionChanged("online"));
    sendPerformanceSnapshotRequest();
    if (performanceTimer !== null) window.clearInterval(performanceTimer);
    performanceTimer = window.setInterval(
      sendPerformanceSnapshotRequest,
      PERFORMANCE_REFRESH_MS,
    );
  });
  socket.addEventListener("message", (event) => {
    try {
      const message = JSON.parse(event.data) as Record<string, unknown>;
      if (
        message.status === "snapshot" &&
        "snapshot" in message
      ) {
        const snapshotMessage = message as unknown as CoreSnapshotMessage;
        store.dispatch(snapshotReceived(snapshotMessage.snapshot));
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
        store.dispatch(
          performanceReceived({
            snapshot: performanceMessage.snapshot,
            edited: message.status === "performance_edited",
          }),
        );
        if (message.status === "performance_edited") {
          pendingPerformanceEdit?.resolve(performanceMessage.snapshot);
          pendingPerformanceEdit = null;
        }
      } else if (
        (message.status === "error" ||
          message.status === "gateway_error") &&
        "message" in message
      ) {
        const errorMessage = message as unknown as CoreErrorMessage;
        store.dispatch(errorReceived(errorMessage.message));
        pendingPerformanceEdit?.reject(new Error(errorMessage.message));
        pendingPerformanceEdit = null;
        pendingPresetRequest?.reject(new Error(errorMessage.message));
        pendingPresetRequest = null;
        pumpPresetRequests();
      }
    } catch {
      store.dispatch(errorReceived("RackForge returned an unreadable response."));
    }
  });
  socket.addEventListener("close", () => {
    socket = null;
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
  });
  socket.addEventListener("error", () => {
    store.dispatch(errorReceived("The RackForge Core connection was interrupted."));
  });
}

function requestPresetOperation<T>(
  request: Record<string, unknown>,
  expected: string,
  decode: (message: Record<string, unknown>) => T,
): Promise<T> {
  if (socket?.readyState !== WebSocket.OPEN) {
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

export function stopGateway() {
  intentionallyStopped = true;
  if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
  if (performanceTimer !== null) window.clearInterval(performanceTimer);
  reconnectTimer = null;
  performanceTimer = null;
  socket?.close();
  socket = null;
}

export function dispatchPerformanceEdit(
  expectedRevision: string,
  edit: PerformanceEdit,
): Promise<PerformanceSnapshot> {
  if (socket?.readyState !== WebSocket.OPEN) {
    const message = "RackForge Core is not connected.";
    store.dispatch(errorReceived(message));
    return Promise.reject(new Error(message));
  }
  if (store.getState().rackforge.performancePending) {
    return Promise.reject(new Error("Another performance edit is still saving."));
  }
  store.dispatch(performanceEditStarted());
  return new Promise((resolve, reject) => {
    pendingPerformanceEdit = { resolve, reject };
    socket?.send(
      JSON.stringify({
        op: "edit_performance",
        expected_revision: expectedRevision,
        edit,
      }),
    );
  });
}

export function dispatchCommand(command: SessionCommand) {
  if (socket?.readyState !== WebSocket.OPEN) {
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
