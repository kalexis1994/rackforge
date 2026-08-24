import type { ResourceGrant, ResourceSelection } from "./types";

export const HOST_PROTOCOL = "rackforge.host@1";

/**
 * True when this build carries its own RackForge inside the page rather than
 * talking to one over the network. The published demo is built this way; a
 * RackForge that serves its own interface is not.
 */
export const IS_BROWSER_HOST = import.meta.env.VITE_RACKFORGE_BROWSER_HOST === "1";

type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

interface NativeHostBridge {
  postMessage(payload: string): void;
}

interface NativeHostMessage {
  protocol?: string;
  kind?: string;
  request_id?: string;
  ok?: boolean;
  status?: number;
  error?: string;
  result?: JsonValue;
  channel?: string;
  event?: string;
  payload?: string;
}

declare global {
  interface Window {
    RackForgeNativeHost?: NativeHostBridge;
    __RACKFORGE_NATIVE_HOST_TOKEN__?: string;
    __RACKFORGE_HOST_SHELL__?: "desktop";
  }
}

export class HostRequestError extends Error {
  readonly status?: number;
  readonly payload?: JsonValue;

  constructor(message: string, status?: number, payload?: JsonValue) {
    super(message);
    this.name = "HostRequestError";
    this.status = status;
    this.payload = payload;
  }
}

interface PendingNativeRequest {
  resolve: (value: JsonValue | undefined) => void;
  reject: (error: Error) => void;
}

export interface SessionChannelCallbacks {
  onOpen(): void;
  onMessage(payload: string): void;
  onClose(): void;
  onError(): void;
}

export interface SessionChannel {
  send(payload: string): void;
  close(): void;
}

let nativeRequestId = 0;
let nativeListenerInstalled = false;
const pendingNativeRequests = new Map<string, PendingNativeRequest>();
const nativeSessionListeners = new Set<SessionChannelCallbacks>();
const NATIVE_REQUEST_TIMEOUT_MS = 15_000;
let lastHapticAt = 0;

function nativeBridge() {
  return window.RackForgeNativeHost;
}

export function isNativeHost() {
  return Boolean(nativeBridge());
}

export function isDesktopHost() {
  return window.__RACKFORGE_HOST_SHELL__ === "desktop";
}

export function isRemoteWebClient() {
  return !isNativeHost() && !isDesktopHost() && !IS_BROWSER_HOST;
}

function installNativeListener() {
  if (nativeListenerInstalled) return;
  nativeListenerInstalled = true;
  window.addEventListener("message", (event: MessageEvent<unknown>) => {
    if (event.source !== window || !event.data || typeof event.data !== "object") {
      return;
    }
    const message = event.data as NativeHostMessage;
    if (message.protocol !== HOST_PROTOCOL) return;
    if (message.kind === "response" && typeof message.request_id === "string") {
      const pending = pendingNativeRequests.get(message.request_id);
      if (!pending) return;
      pendingNativeRequests.delete(message.request_id);
      if (message.ok) {
        pending.resolve(message.result);
      } else {
        pending.reject(
          new HostRequestError(
            typeof message.error === "string"
              ? message.error
              : "The RackForge host rejected the request.",
            message.status,
          ),
        );
      }
      return;
    }
    if (
      message.kind === "event" &&
      message.channel === "session" &&
      typeof message.event === "string"
    ) {
      for (const listener of nativeSessionListeners) {
        switch (message.event) {
          case "open":
            listener.onOpen();
            break;
          case "message":
            if (typeof message.payload === "string") {
              listener.onMessage(message.payload);
            }
            break;
          case "close":
            listener.onClose();
            break;
          case "error":
            listener.onError();
            break;
        }
      }
    }
  });
}

function requestNative<T>(
  method: string,
  params: JsonValue,
  timeoutMs = NATIVE_REQUEST_TIMEOUT_MS,
): Promise<T> {
  const bridge = nativeBridge();
  if (!bridge) {
    return Promise.reject(new HostRequestError("RackForge native host is unavailable."));
  }
  installNativeListener();
  nativeRequestId += 1;
  const requestId = `web-${nativeRequestId}`;
  return new Promise<T>((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      if (!pendingNativeRequests.delete(requestId)) return;
      reject(
        new HostRequestError(
          `RackForge native host timed out while handling ${method}.`,
          408,
        ),
      );
    }, timeoutMs);
    pendingNativeRequests.set(requestId, {
      resolve: (value) => {
        window.clearTimeout(timeout);
        resolve(value as T);
      },
      reject: (error) => {
        window.clearTimeout(timeout);
        reject(error);
      },
    });
    try {
      bridge.postMessage(
        JSON.stringify({
          protocol: HOST_PROTOCOL,
          kind: "request",
          request_id: requestId,
          token: window.__RACKFORGE_NATIVE_HOST_TOKEN__,
          method,
          params,
        }),
      );
    } catch (error) {
      pendingNativeRequests.delete(requestId);
      window.clearTimeout(timeout);
      reject(error instanceof Error ? error : new Error(String(error)));
    }
  });
}

async function responseError(response: Response) {
  const payload = (await response.json().catch(() => null)) as {
    message?: string;
  } | null;
  return new HostRequestError(
    payload?.message ?? `RackForge host returned HTTP ${response.status}.`,
    response.status,
    payload as JsonValue | null,
  );
}

export async function hostJson<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  if (IS_BROWSER_HOST) {
    const { browserHostJson } = await import("./browser/client");
    return browserHostJson<T>(path, init);
  }
  const bridge = nativeBridge();
  if (bridge) {
    const headers = Object.fromEntries(new Headers(init.headers).entries());
    const method = init.method ?? "GET";
    const timeoutMs = method.toUpperCase() === "DELETE" && path.startsWith("/api/v1/plugins/")
      ? 2 * 60_000
      : NATIVE_REQUEST_TIMEOUT_MS;
    return requestNative<T>("http.request", {
      path,
      method,
      headers,
      body: typeof init.body === "string" ? init.body : null,
    }, timeoutMs);
  }
  const response = await fetch(path, {
    ...init,
    cache: init.cache ?? (path.startsWith("/api/v1/") ? "no-store" : undefined),
  });
  if (!response.ok) throw await responseError(response);
  return (await response.json()) as T;
}

export function hostHaptic(style: "tap" | "confirm" = "tap") {
  if (!nativeBridge()) return;
  const now = performance.now();
  if (now - lastHapticAt < 35) return;
  lastHapticAt = now;
  void requestNative<JsonValue>("ui.haptic", { style }).catch(() => undefined);
}

export function syncNativeRoute(path: string) {
  if (!nativeBridge()) return;
  void requestNative<JsonValue>("ui.route", { path }).catch(() => undefined);
}

export function selectNativeResource(options: {
  kind: "file" | "directory";
  extensions?: string[];
}): Promise<ResourceSelection> {
  if (nativeBridge()) {
    return requestNative("resource.pick", options, 10 * 60_000);
  }
  if (isDesktopHost()) {
    return hostJson<ResourceSelection>("/api/v1/resources/native-pick", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(options),
    });
  }
  return Promise.reject(
    new HostRequestError("The native resource picker is unavailable."),
  );
}

export function readNativeTextFile(options: {
  extensions?: string[];
  maximum_bytes?: number;
}): Promise<{ file_name: string; text: string }> {
  if (nativeBridge()) {
    return requestNative("resource.read_text", options, 10 * 60_000);
  }
  if (isDesktopHost()) {
    return hostJson("/api/v1/resources/native-read-text", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(options),
    });
  }
  return Promise.reject(
    new HostRequestError("The native text-file picker is unavailable."),
  );
}

export async function savePortableTextFile(options: {
  file_name: string;
  mime_type: string;
  text: string;
}): Promise<void> {
  if (nativeBridge()) {
    await requestNative("resource.write_text", options, 10 * 60_000);
    return;
  }
  if (isDesktopHost()) {
    await hostJson("/api/v1/resources/native-write-text", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(options),
    });
    return;
  }
  const blob = new Blob([options.text], { type: options.mime_type });
  const url = URL.createObjectURL(blob);
  try {
    const link = document.createElement("a");
    link.href = url;
    link.download = options.file_name;
    link.style.display = "none";
    document.body.append(link);
    link.click();
    link.remove();
  } finally {
    window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
  }
}

export function bindNativePluginResource(options: {
  plugin_id: string;
  resource_id: string;
  kind: "file" | "directory";
  extensions?: string[];
}): Promise<ResourceGrant> {
  if (nativeBridge()) {
    return requestNative("resource.bind", options, 10 * 60_000);
  }
  if (isDesktopHost()) {
    return selectNativeResource({
      kind: options.kind,
      extensions: options.extensions,
    }).then((selection) =>
      hostJson<ResourceGrant>("/api/v1/resources/bind-selection", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          plugin_id: options.plugin_id,
          resource_id: options.resource_id,
          selection_id: selection.selection_id,
        }),
      }),
    );
  }
  return Promise.reject(
    new HostRequestError("The native plugin resource picker is unavailable."),
  );
}

export function selectNativePluginSound(options: {
  instance_id: string;
  sound_id: string;
}): Promise<{ sound_id: string }> {
  if (!nativeBridge()) {
    return Promise.reject(
      new HostRequestError("The native plugin runtime is unavailable."),
    );
  }
  return requestNative("plugin.select_sound", options);
}

function browserSessionUrl() {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/ws/v1/session`;
}

export function openSessionChannel(
  callbacks: SessionChannelCallbacks,
): SessionChannel {
  if (IS_BROWSER_HOST) {
    // Deferred so a networked build never loads the engine, and the page can
    // start rendering before the audio worklet is ready.
    let channel: SessionChannel | null = null;
    let closed = false;
    const queued: string[] = [];
    void import("./browser/client").then(({ openBrowserSessionChannel }) => {
      if (closed) return;
      channel = openBrowserSessionChannel(callbacks);
      for (const payload of queued.splice(0)) channel.send(payload);
    });
    return {
      send: (payload) => (channel ? channel.send(payload) : queued.push(payload)),
      close: () => {
        closed = true;
        channel?.close();
      },
    };
  }
  if (!nativeBridge()) {
    const socket = new WebSocket(browserSessionUrl());
    socket.addEventListener("open", callbacks.onOpen);
    socket.addEventListener("message", (event) =>
      callbacks.onMessage(String(event.data)),
    );
    socket.addEventListener("close", callbacks.onClose);
    socket.addEventListener("error", callbacks.onError);
    return {
      send: (payload) => socket.send(payload),
      close: () => socket.close(),
    };
  }

  installNativeListener();
  nativeSessionListeners.add(callbacks);
  void requestNative<JsonValue>("session.connect", {}).catch(() =>
    callbacks.onError(),
  );
  return {
    send: (payload) => {
      void requestNative<JsonValue>("session.send", { payload }).catch(() =>
        callbacks.onError(),
      );
    },
    close: () => {
      nativeSessionListeners.delete(callbacks);
      void requestNative<JsonValue>("session.close", {}).catch(() => undefined);
    },
  };
}
