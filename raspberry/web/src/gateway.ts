import {
  connectionChanged,
  errorReceived,
  snapshotReceived,
  store,
} from "./store";
import type {
  CoreErrorMessage,
  CoreSnapshotMessage,
  SessionCommand,
} from "./types";

const CLIENT_ID = "web.main";
const SESSION_SCHEMA_VERSION = 9;
const RECONNECT_DELAY_MS = 1200;

let socket: WebSocket | null = null;
let commandId = 0;
let reconnectTimer: number | null = null;
let intentionallyStopped = false;

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
        (message.status === "error" ||
          message.status === "gateway_error") &&
        "message" in message
      ) {
        const errorMessage = message as unknown as CoreErrorMessage;
        store.dispatch(errorReceived(errorMessage.message));
      }
    } catch {
      store.dispatch(errorReceived("RackForge returned an unreadable response."));
    }
  });
  socket.addEventListener("close", () => {
    socket = null;
    store.dispatch(connectionChanged("offline"));
    scheduleReconnect();
  });
  socket.addEventListener("error", () => {
    store.dispatch(errorReceived("The RackForge Core connection was interrupted."));
  });
}

export function stopGateway() {
  intentionallyStopped = true;
  if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
  reconnectTimer = null;
  socket?.close();
  socket = null;
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
