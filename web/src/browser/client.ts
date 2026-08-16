/**
 * Connects the RackForge SPA to a host running inside the page.
 *
 * The interface is unchanged: it opens a session channel and makes the same
 * requests it makes of an appliance over the network. Only the transport
 * differs — messages go to the audio worklet instead of a socket, and the few
 * endpoints that describe real hardware answer for a page instead.
 */

import type { SessionChannelCallbacks } from "../host";
import { HostRequestError } from "../host";
import engineWorkletUrl from "./engine.worklet.ts?worker&url";
import {
  ENGINE_PROCESSOR,
  PACKAGED_STORAGE_PREFIX,
  type EngineEvent,
  type SeedFile,
} from "./protocol";
import {
  readStoredFiles,
  requestPersistentStorage,
  writeStoredFiles,
} from "./storage";
import type {
  HostAudioSettings,
  PluginWebDescriptor,
  ResourceSelection,
  WebAuthStatus,
  WebPublicConfig,
} from "../types";

/** Frames per render. Web Audio always asks for 128. */
const RENDER_FRAMES = 128;
const OUTPUT_CHANNELS = 2;
/** Where the built site keeps the storage image the host boots against. */
const STORAGE_MANIFEST = "demo/storage.json";
const HOST_MODULE = "demo/rackforge-browser.wasm";

interface StorageManifest {
  /** Paths below the RackForge data root, relative to the storage image. */
  files: string[];
}

interface Pending {
  resolve: (response: string) => void;
  reject: (error: Error) => void;
}

let context: AudioContext | null = null;
let engine: AudioWorkletNode | null = null;
let booting: Promise<void> | null = null;
let bootError: string | null = null;
let bootWarnings: string[] = [];
let nextRequestId = 1;
const pending = new Map<number, Pending>();
const listeners = new Set<SessionChannelCallbacks>();

function assetUrl(path: string): string {
  return new URL(path, document.baseURI).toString();
}

async function fetchBytes(path: string): Promise<Uint8Array> {
  const response = await fetch(assetUrl(path), { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`could not read ${path} (${response.status})`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

/**
 * Assembles the filesystem the host boots against.
 *
 * Two sources: the packages RackForge ships with the site, which are read from
 * it every visit so they follow the deployed version, and everything the host
 * wrote on earlier visits — installed plugins, presets, the performance
 * library — which is read from this browser.
 */
async function loadStorage(): Promise<SeedFile[]> {
  const manifest = (await (await fetch(assetUrl(STORAGE_MANIFEST))).json()) as StorageManifest;
  const [packaged, stored] = await Promise.all([
    Promise.all(
      manifest.files.map(async (path) => ({
        path,
        bytes: await fetchBytes(`demo/rackforge/${path}`),
      })),
    ),
    readStoredFiles(),
  ]);
  return [
    ...packaged,
    ...stored.filter((file) => !file.path.startsWith(PACKAGED_STORAGE_PREFIX)),
  ];
}

/**
 * Files the host's storage after it changes.
 *
 * Writes are coalesced: a burst of edits — dragging a fader, say — should cost
 * one write rather than one per event.
 */
let storageWrite: number | null = null;
let pendingStorage: SeedFile[] = [];

function storeFiles(files: SeedFile[]) {
  pendingStorage = files;
  if (storageWrite !== null) return;
  storageWrite = window.setTimeout(() => {
    storageWrite = null;
    const files = pendingStorage;
    pendingStorage = [];
    void writeStoredFiles(files).catch((error: unknown) => {
      console.warn("RackForge could not keep its storage in this browser", error);
    });
  }, 400);
}

/** Boot milestones the engine announces, awaited by [`startBrowserHost`]. */
const milestones = {
  ready: null as (() => void) | null,
  booted: null as ((error: string | null) => void) | null,
};

function handleEngineEvent(event: EngineEvent) {
  switch (event.kind) {
    case "response": {
      const waiting = pending.get(event.id);
      if (!waiting) return;
      pending.delete(event.id);
      waiting.resolve(event.response);
      break;
    }
    case "booted":
      bootWarnings = event.warnings;
      bootError = event.ok
        ? null
        : (event.error ?? "the RackForge engine did not start");
      milestones.booted?.(bootError);
      break;
    case "storage":
      storeFiles(event.files);
      break;
    case "ready":
      milestones.ready?.();
      break;
    case "revision":
      break;
  }
}

/**
 * Starts the audio context and the host inside it.
 *
 * Browsers only let a page make sound after someone has interacted with it, so
 * this is called from the first gesture and is safe to call again afterwards.
 */
export async function startBrowserHost(): Promise<void> {
  if (booting) {
    await booting;
    if (context?.state === "suspended") {
      await context.resume();
    }
    return;
  }
  booting = (async () => {
    const audio = new AudioContext({ latencyHint: "interactive" });
    await audio.audioWorklet.addModule(engineWorkletUrl);
    const [wasm, files] = await Promise.all([fetchBytes(HOST_MODULE), loadStorage()]);

    const node = new AudioWorkletNode(audio, ENGINE_PROCESSOR, {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [OUTPUT_CHANNELS],
    });
    node.port.onmessage = (message: MessageEvent<EngineEvent>) =>
      handleEngineEvent(message.data);
    node.onprocessorerror = () => {
      bootError = "the RackForge engine stopped on the audio thread";
      console.error(bootError);
    };
    node.connect(audio.destination);

    const ready = milestone(
      "ready",
      audio,
      "the RackForge engine did not start on the audio thread",
    );
    const booted = milestone("booted", audio, "the RackForge engine did not answer");

    // The processor only exists once the context is running, and a suspended
    // context never delivers the boot message. Most browsers keep it suspended
    // until someone has interacted with the page, so this waits for that
    // rather than failing: the engine has done nothing wrong, and a person who
    // has not touched the page yet is not waiting for sound.
    resumeOnGesture(audio);
    await running(audio);
    await ready;

    node.port.postMessage(
      {
        kind: "boot",
        wasm,
        files,
        maximumFrames: RENDER_FRAMES,
        channels: OUTPUT_CHANNELS,
      },
      [wasm.buffer, ...files.map((file) => file.bytes.buffer)],
    );
    context = audio;
    await booted;

    engine = node;
    attachWebMidi(node);
    // Worth asking once the host is actually running: a browser is far more
    // likely to grant this to a page someone is using than to one that just
    // opened.
    void requestPersistentStorage();
  })();

  try {
    await booting;
  } catch (error) {
    booting = null;
    console.error("RackForge could not start in this page", error);
    throw error;
  }
}

/** Warnings the host reported while loading its packages. */
export function browserHostWarnings(): string[] {
  return bootWarnings;
}

export function browserHostError(): string | null {
  return bootError;
}

/**
 * Pushes the current session to every open channel.
 *
 * The engine answers questions; it does not volunteer. Whenever something
 * changes the session behind the interface's back — installing a plugin, for
 * one — the channel republishes it.
 */
async function publishSnapshot(): Promise<void> {
  if (!engine || listeners.size === 0) return;
  const snapshot = await send(JSON.stringify({ op: "snapshot" }));
  for (const listener of listeners) {
    listener.onMessage(snapshot);
  }
}

/** Resolves once the browser has let the context start. */
function running(audio: AudioContext): Promise<void> {
  return new Promise((resolve) => {
    const settle = () => {
      if (audio.state !== "running") return;
      audio.removeEventListener("statechange", settle);
      resolve();
    };
    audio.addEventListener("statechange", settle);
    void audio.resume().catch(() => undefined);
    settle();
  });
}

/**
 * Waits for one boot milestone, failing with a message that says whether the
 * browser had allowed audio to start yet.
 */
function milestone(
  name: "ready" | "booted",
  audio: AudioContext,
  timedOut: string,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      milestones[name] = null;
      reject(
        new Error(
          audio.state === "running"
            ? timedOut
            : "the browser has not allowed audio to start yet",
        ),
      );
    }, 15_000);
    const settle = (error?: string | null) => {
      window.clearTimeout(timeout);
      milestones[name] = null;
      if (error) {
        reject(new Error(error));
      } else {
        resolve();
      }
    };
    if (name === "ready") {
      milestones.ready = () => settle(null);
    } else {
      milestones.booted = (error) => settle(error);
    }
  });
}

/**
 * Resumes the audio context on the first interaction.
 *
 * Browsers refuse to make sound until someone has touched the page. RackForge
 * asks once per gesture kind and stops asking as soon as it is running.
 */
function resumeOnGesture(audio: AudioContext) {
  const resume = () => {
    void audio.resume().catch(() => undefined);
    if (audio.state === "running") {
      for (const event of ["pointerdown", "keydown", "touchstart"] as const) {
        window.removeEventListener(event, resume);
      }
    }
  };
  for (const event of ["pointerdown", "keydown", "touchstart"] as const) {
    window.addEventListener(event, resume);
  }
}

function send(request: string): Promise<string> {
  const node = engine;
  if (!node) {
    return Promise.reject(new Error("The RackForge engine is not running."));
  }
  const id = nextRequestId++;
  return new Promise<string>((resolve, reject) => {
    pending.set(id, { resolve, reject });
    node.port.postMessage({ kind: "request", id, request });
  });
}

/**
 * Forwards Web MIDI straight to the engine. Browsers that do not implement it,
 * or a person who declines the permission, simply play with the on-screen
 * controls instead.
 */
function attachWebMidi(node: AudioWorkletNode) {
  const midi = (
    navigator as Navigator & {
      requestMIDIAccess?: () => Promise<{
        inputs: Map<string, { onmidimessage: ((event: MIDIMessageEvent) => void) | null }>;
      }>;
    }
  ).requestMIDIAccess;
  if (!midi) return;
  void midi
    .call(navigator)
    .then((access) => {
      for (const input of access.inputs.values()) {
        input.onmidimessage = (event: MIDIMessageEvent) => {
          const data = event.data;
          if (!data || data.length === 0 || data[0] >= 0xf0) return;
          node.port.postMessage({
            kind: "midi",
            data: [data[0], data[1] ?? 0, data[2] ?? 0],
            length: Math.min(data.length, 3),
          });
        };
      }
    })
    .catch(() => undefined);
}

/**
 * A session channel backed by the engine.
 *
 * The appliance gateway pushes a fresh snapshot whenever the core advances, and
 * the SPA relies on that to settle its pending commands. The engine answers one
 * request at a time, so the channel follows any command that changed the
 * session with the snapshot the interface is waiting for.
 */
export function openBrowserSessionChannel(callbacks: SessionChannelCallbacks) {
  listeners.add(callbacks);
  let open = true;

  void startBrowserHost()
    .then(async () => {
      if (!open) return;
      callbacks.onOpen();
      // An appliance publishes the session as soon as a surface connects,
      // rather than waiting to be asked, and the interface is written for
      // that. The engine answers requests, so the channel asks on its behalf.
      const snapshot = await send(JSON.stringify({ op: "snapshot" }));
      if (open) callbacks.onMessage(snapshot);
    })
    .catch(() => {
      if (!open) return;
      callbacks.onError();
      callbacks.onClose();
    });

  return {
    send(payload: string) {
      if (!open) return;
      void send(payload)
        .then(async (response) => {
          if (!open) return;
          callbacks.onMessage(response);
          const decoded = JSON.parse(response) as { status?: string };
          if (
            decoded.status === "command_applied" ||
            decoded.status === "plugin_preset_loaded"
          ) {
            const snapshot = await send(JSON.stringify({ op: "snapshot" }));
            if (open) callbacks.onMessage(snapshot);
          }
        })
        .catch((error: Error) => {
          if (!open) return;
          callbacks.onMessage(
            JSON.stringify({ status: "error", code: "internal", message: error.message }),
          );
        });
    },
    close() {
      open = false;
      listeners.delete(callbacks);
      callbacks.onClose();
    },
  };
}

/**
 * Archives the interface has handed over but not yet installed.
 *
 * A networked RackForge stages an upload on the host and refers to it by a
 * selection id. The page does the same, in memory: the interface's install
 * flow is unchanged, and nothing is written until someone confirms it.
 */
const selections = new Map<string, { name: string; archive: Uint8Array }>();
let nextSelectionId = 1;

/** Sends one archive to the engine and returns its answer. */
function sendPackage(
  action: "inspect" | "install",
  archive: Uint8Array,
): Promise<{ ok: boolean; error?: string; preview?: PackagePreview; installed?: InstalledPackage }> {
  const node = engine;
  if (!node) {
    return Promise.reject(new Error("The RackForge engine is not running."));
  }
  const id = nextRequestId++;
  return new Promise((resolve, reject) => {
    pending.set(id, {
      resolve: (response) => resolve(JSON.parse(response)),
      reject,
    });
    node.port.postMessage({ kind: "package", id, action, archive });
  });
}

interface PackagePreview {
  plugin_id: string;
  plugin_name: string;
  vendor: string;
  version: string;
  description?: string | null;
  kind: string;
  platform: string;
  portable: boolean;
  archive_bytes: number;
}

interface InstalledPackage {
  plugin_id: string;
  version: string;
  already_installed: boolean;
  activation_required: boolean;
}

function selectionOf(body: RequestInit["body"]): { name: string; archive: Uint8Array } {
  const { selection_id: id } = JSON.parse(String(body)) as { selection_id?: string };
  const selection = id ? selections.get(id) : undefined;
  if (!selection) {
    throw new HostRequestError("This upload is no longer available; choose the file again.", 404);
  }
  return selection;
}

/**
 * Answers the host endpoints for a page.
 *
 * A browser has no PIN to enrol, no audio devices to choose between and no
 * plugin store to install from, so those endpoints report what is true here
 * rather than pretending to offer the appliance's choices.
 */
export async function browserHostJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const method = (init.method ?? "GET").toUpperCase();
  if (path === "/api/v1/auth/status" && method === "GET") {
    return {
      status: "ok",
      pin_managed: false,
      requires_pin: false,
      unlocked: true,
      pin_state: "unclaimed",
      pin_digits: 0,
      locked_for: 0,
    } satisfies WebAuthStatus as T;
  }
  if (path === "/api/v1/config" && method === "GET") {
    return {
      enabled: true,
      access: "local",
      port: 0,
      configurable: false,
    } satisfies WebPublicConfig as T;
  }
  if (path === "/api/v1/plugins" && method === "GET") {
    // The demo runs the instrument RackForge ships, which has no web surface
    // of its own to index.
    return [] as unknown as T;
  }
  if (path === "/api/v1/host/audio" && method === "GET") {
    const rate = context?.sampleRate ?? 48_000;
    return {
      status: "ok",
      host: "browser",
      inventory: {
        drivers: [{ name: "Web Audio", available: true, detail: "Provided by the browser" }],
        outputs: [
          {
            driver: "Web Audio",
            name: "Browser audio output",
            is_default: true,
            channels: OUTPUT_CHANNELS,
            default_sample_rate: rate,
            sample_rates: [rate],
            buffer_frames: [RENDER_FRAMES],
          },
        ],
        midi_inputs: [],
      },
      preferences: {
        schema_version: 1,
        driver: "Web Audio",
        output_device: "Browser audio output",
        sample_rate_hz: rate,
        buffer_frames: RENDER_FRAMES,
        output_gain_db: 0,
        midi_inputs: [],
      },
      runtime_status: engine ? "running" : "stopped",
    } satisfies HostAudioSettings as T;
  }
  if (path.startsWith("/api/v1/resources/uploads") && method === "POST") {
    const name = new URLSearchParams(path.split("?")[1] ?? "").get("name") ?? "package.rfplugin";
    const body = init.body;
    const archive = new Uint8Array(
      body instanceof Blob ? await body.arrayBuffer() : new ArrayBuffer(0),
    );
    if (archive.length === 0) {
      throw new HostRequestError("The package is empty.", 400);
    }
    const selectionId = `browser.upload.${nextSelectionId++}`;
    selections.set(selectionId, { name, archive });
    return {
      selection_id: selectionId,
      display_name: name,
      kind: "file",
      size: archive.length,
      source: "client_upload",
      // Held until the interface releases it or the page goes away, so there
      // is no deadline to report.
      expires_in_seconds: 0,
    } satisfies ResourceSelection as T;
  }
  if (path === "/api/v1/resources/selections/release" && method === "POST") {
    const { selection_id: id } = JSON.parse(String(init.body)) as { selection_id?: string };
    if (id) selections.delete(id);
    return { status: "ok" } as T;
  }
  if (path === "/api/v1/plugins/inspect" && method === "POST") {
    const { selection_id: id } = JSON.parse(String(init.body)) as { selection_id?: string };
    const selection = selectionOf(init.body);
    const answer = await sendPackage("inspect", selection.archive);
    if (!answer.ok || !answer.preview) {
      throw new HostRequestError(answer.error ?? "This package could not be read.", 400);
    }
    return { ...answer.preview, selection_id: id, branding: null } as T;
  }
  if (path === "/api/v1/plugins/install" && method === "POST") {
    const { selection_id: id } = JSON.parse(String(init.body)) as { selection_id?: string };
    const selection = selectionOf(init.body);
    const answer = await sendPackage("install", selection.archive);
    if (!answer.ok || !answer.installed) {
      throw new HostRequestError(answer.error ?? "This package could not be installed.", 400);
    }
    if (id) selections.delete(id);
    // The engine reloaded its session over the new package; the interface is
    // still showing the previous one.
    await publishSnapshot();
    return answer.installed as T;
  }
  if (path.startsWith("/api/v1/plugins/") && path.endsWith("/activate") && method === "POST") {
    // Installing already loaded and activated the package here, so there is
    // nothing left to start.
    return { status: "active" } as T;
  }
  if (path.startsWith("/api/v1/plugins/") && method === "GET") {
    const pluginId = decodeURIComponent(path.slice("/api/v1/plugins/".length));
    return {
      plugin_id: pluginId,
      plugin_name: pluginId,
      version: "",
      active: true,
      managed: true,
      api_version: 1,
      branding: null,
      surfaces: [],
      resources: [],
    } satisfies PluginWebDescriptor as T;
  }
  throw new HostRequestError(
    "The browser host does not provide this; it is part of an installed RackForge.",
    501,
  );
}
