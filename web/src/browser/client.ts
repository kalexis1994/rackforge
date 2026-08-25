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
  canServePluginAssets,
  pluginAssetUrl,
  publishPluginAssets,
  whenServing,
} from "./pluginAssets";
import {
  readStoredFiles,
  requestPersistentStorage,
  writeStoredFiles,
} from "./storage";
import { assetUrl } from "../assets";
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

/**
 * Gives every file that defines the bundled browser host a deployment-stable
 * cache key.
 *
 * The offline worker intentionally caches ordinary public files. Without a
 * revision on the storage manifest, the first visit after a deployment boots
 * from yesterday's plugin list while the worker refreshes it in the
 * background. The UI and host module are already tied to this revision; the
 * packaged filesystem must be tied to it as well.
 */
export function versionedBrowserAsset(path: string): string {
  const separator = path.includes("?") ? "&" : "?";
  return `${path}${separator}v=${encodeURIComponent(__UI_REVISION__)}`;
}

interface StorageManifest {
  /** Paths below the RackForge data root, relative to the storage image. */
  files: string[];
}

interface Pending {
  resolve: (response: string) => void;
  reject: (error: Error) => void;
  timeout: number;
}

let context: AudioContext | null = null;
let engine: AudioWorkletNode | null = null;
let booting: Promise<void> | null = null;
let bootError: string | null = null;
let bootWarnings: string[] = [];
let nextRequestId = 1;
const pending = new Map<number, Pending>();
const listeners = new Set<SessionChannelCallbacks>();

export interface BrowserMidiEndpoint {
  id: string;
  name: string | null;
  manufacturer?: string | null;
  state: string;
}

interface BrowserMidiInput extends BrowserMidiEndpoint {
  onmidimessage: ((event: { data?: Uint8Array }) => void) | null;
}

interface BrowserMidiOutput extends BrowserMidiEndpoint {
  send(data: Uint8Array, timestamp?: number): void;
  clear?(): void;
}

interface BrowserMidiAccess {
  inputs: Map<string, BrowserMidiInput>;
  outputs: Map<string, BrowserMidiOutput>;
  onstatechange: (() => void) | null;
}

let webMidiSupported = false;
let webMidiSysex = false;
let midiInputNames: string[] = [];
let keyLabConnected = false;
let keyLabInputId: string | null = null;
let keyLabDisplayConnected = false;
let keyLabOutput: BrowserMidiOutput | null = null;
let keyLabRestorePlan: Array<{ bytes: number[]; settle_after_ms: number }> = [];
let keyLabReleaseSent = false;
let webMidiReconcile: (() => void) | null = null;
let controllerLifecycleInstalled = false;
const CONTROLLER_COLOR_KEY = "rackforge.controller.arturia-keylab-essential-mk3.color";
const DEFAULT_CONTROLLER_COLOR = "#145080";

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
/**
 * The packages the site ships, kept for as long as the page lives.
 *
 * The host writes back only what it owns — sessions, presets, the performance
 * library — so a storage snapshot never mentions a packaged plugin. Publishing
 * from that snapshot alone therefore looked like every package had been
 * uninstalled, and took the published files with it: the instrument kept
 * sounding, since audio needs no URLs, while its interface answered 404.
 */
let packagedFiles: SeedFile[] = [];

async function loadStorage(): Promise<SeedFile[]> {
  const manifest = (await (
    await fetch(assetUrl(versionedBrowserAsset(STORAGE_MANIFEST)))
  ).json()) as StorageManifest;
  const [packaged, stored] = await Promise.all([
    Promise.all(
      manifest.files.map(async (path) => ({
        path,
        bytes: await fetchBytes(versionedBrowserAsset(`demo/rackforge/${path}`)),
      })),
    ),
    readStoredFiles(),
  ]);
  const files = [
    ...packaged,
    ...stored.filter((file) => !file.path.startsWith(PACKAGED_STORAGE_PREFIX)),
  ];
  // Copies, because booting the engine transfers every file's buffer to the
  // worklet and leaves these detached. Republishing then threw on the first
  // storage write, and a plugin's interface quietly stopped following its
  // package for the rest of the session.
  packagedFiles = packaged.map((file) => ({ path: file.path, bytes: file.bytes.slice() }));
  await publishPluginAssets(files).catch((error: unknown) => {
    console.warn("RackForge could not publish a plugin's files", error);
  });
  return files;
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
  // A plugin's own interface is served from what the host holds, so the
  // published files follow every install and removal — but the packages the
  // site ships are not in this snapshot and must not be dropped with it.
  void publishPluginAssets([
    ...packagedFiles.filter(
      (packaged) => !files.some((file) => file.path === packaged.path),
    ),
    ...files,
  ]).catch((error: unknown) => {
    console.warn("RackForge could not publish a plugin's files", error);
  });
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
      window.clearTimeout(waiting.timeout);
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
      void publishSnapshot();
      break;
    case "controller_output": {
      const output = keyLabOutput;
      if (!output || output.state !== "connected") break;
      let timestamp = performance.now();
      for (const message of event.messages) {
        try {
          output.send(new Uint8Array(message.bytes), timestamp);
        } catch (error) {
          console.warn("RackForge could not write the Arturia display", error);
          break;
        }
        timestamp += message.settle_after_ms;
      }
      break;
    }
  }
}

/**
 * Starts the audio context and the host inside it.
 *
 * Browsers only let a page make sound after someone has interacted with it, so
 * this is called from the first gesture and is safe to call again afterwards.
 */
export async function startBrowserHost(): Promise<void> {
  // Inside the desktop shell the native engine IS the instrument. This
  // module only exists there when the embedded dist was built with the
  // browser-host flag by mistake -- which happened: the desktop shipped a
  // demo build, its WebView asked for WebMIDI permission at startup, and
  // granting it layered a second complete piano (its own AudioContext,
  // worklet and wasm) over the native one. Every fader then edited only one
  // of the two, which is unfixable from a panel. Refuse to boot, whatever
  // the build flags say.
  if (window.__RACKFORGE_HOST_SHELL__ === "desktop") {
    throw new Error("the browser engine must not run inside the desktop shell");
  }
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
    // The worker keeps non-hashed files available offline. Tie the host ABI to
    // the hashed UI build so a deployment can never pair a new worklet with
    // yesterday's cached WebAssembly exports.
    const [wasm, files] = await Promise.all([
      fetchBytes(versionedBrowserAsset(HOST_MODULE)),
      loadStorage(),
    ]);

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
      rejectPending(new Error(bootError));
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
let snapshotPublishing: Promise<void> | null = null;
let snapshotQueued = false;

async function publishSnapshot(): Promise<void> {
  if (!engine || listeners.size === 0) return;
  if (snapshotPublishing) {
    snapshotQueued = true;
    return snapshotPublishing;
  }
  snapshotPublishing = (async () => {
    do {
      snapshotQueued = false;
      const snapshot = await send(JSON.stringify({ op: "snapshot" }));
      for (const listener of listeners) {
        listener.onMessage(snapshot);
      }
    } while (snapshotQueued);
  })().finally(() => {
    snapshotPublishing = null;
  });
  return snapshotPublishing;
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
    // The clock only runs while the browser is allowing sound. A page nobody
    // has touched yet is not a page that is failing: its worklet cannot run,
    // so it cannot reach a milestone, and timing it out killed the host for
    // the rest of the visit — after which the gesture that would have started
    // it arrived to nothing.
    let timeout: number | null = null;
    const arm = () => {
      if (timeout !== null || audio.state !== "running") return;
      timeout = window.setTimeout(() => {
        milestones[name] = null;
        audio.removeEventListener("statechange", arm);
        reject(new Error(timedOut));
      }, 15_000);
    };
    audio.addEventListener("statechange", arm);
    arm();
    const settle = (error?: string | null) => {
      if (timeout !== null) window.clearTimeout(timeout);
      audio.removeEventListener("statechange", arm);
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
    const timeout = window.setTimeout(() => {
      pending.delete(id);
      reject(new Error("RackForge did not answer the control request in time."));
    }, 30_000);
    pending.set(id, { resolve, reject, timeout });
    node.port.postMessage({ kind: "request", id, request });
  });
}

function rejectPending(error: Error) {
  for (const waiting of pending.values()) {
    window.clearTimeout(waiting.timeout);
    waiting.reject(error);
  }
  pending.clear();
}

export function sendControllerRestorePlan(
  output: Pick<BrowserMidiOutput, "send" | "clear">,
  plan: ReadonlyArray<{ bytes: number[] }>,
): boolean {
  try {
    output.clear?.();
    // Unload handlers cannot wait for cosmetic settle delays. Immediate
    // writes preserve packet order and maximize the chance that the browser
    // hands the complete reset to its MIDI stack before page teardown.
    for (const message of plan) {
      output.send(new Uint8Array(message.bytes));
    }
    return true;
  } catch (error) {
    console.warn("RackForge could not restore the Arturia controller", error);
    return false;
  }
}

function releaseKeyLabForPageExit() {
  if (!keyLabConnected || keyLabReleaseSent) return;
  keyLabReleaseSent = true;
  // Keep the worklet coherent when it receives a final timeslice, but do not
  // depend on it: the direct Web MIDI path owns page teardown.
  engine?.port.postMessage({ kind: "controller_connection", connected: false });
  if (keyLabOutput && keyLabRestorePlan.length > 0) {
    sendControllerRestorePlan(keyLabOutput, keyLabRestorePlan);
  }
  keyLabConnected = false;
  keyLabDisplayConnected = false;
}

function installControllerLifecycleRelease() {
  if (controllerLifecycleInstalled) return;
  controllerLifecycleInstalled = true;
  window.addEventListener("pagehide", releaseKeyLabForPageExit, { capture: true });
  window.addEventListener("beforeunload", releaseKeyLabForPageExit, { capture: true });
  window.addEventListener("pageshow", () => {
    keyLabReleaseSent = false;
    webMidiReconcile?.();
  });
}

function requestControllerRestorePlan(
  node: AudioWorkletNode,
): Promise<Array<{ bytes: number[]; settle_after_ms: number }>> {
  const id = nextRequestId++;
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      pending.delete(id);
      reject(new Error("RackForge did not provide the controller release plan."));
    }, 10_000);
    pending.set(id, {
      resolve: (response) => resolve(JSON.parse(response)),
      reject,
      timeout,
    });
    node.port.postMessage({ kind: "controller_restore_plan", id });
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
      requestMIDIAccess?: (options?: { sysex?: boolean }) => Promise<BrowserMidiAccess>;
    }
  ).requestMIDIAccess;
  webMidiSupported = Boolean(midi);
  if (!midi) return;
  installControllerLifecycleRelease();
  void requestControllerRestorePlan(node)
    .then((plan) => {
      keyLabRestorePlan = plan;
    })
    .catch((error) => console.warn(error));

  const connect = (access: BrowserMidiAccess, sysex: boolean) => {
    webMidiSysex = sysex;
    const reconcile = () => {
      midiInputNames = [...access.inputs.values()]
        .filter((input) => input.state === "connected")
        .map((input) => input.name?.trim() || "MIDI input");
      const { input: controllerInput, output: controllerOutput } = resolveKeyLabTransport(
        access.inputs.values(),
        access.outputs.values(),
        sysex,
      );
      // The semantic controller profile uses ordinary channel messages and
      // remains useful without SysEx. Only LITTLE's OLED and LEDs require the
      // output plus the stronger browser permission.
      const nextController = Boolean(controllerInput);
      const pairChanged =
        keyLabInputId !== (controllerInput?.id ?? null) ||
        keyLabOutput?.id !== controllerOutput?.id ||
        keyLabConnected !== nextController;

      for (const input of access.inputs.values()) {
        // Assignment makes hotplug reconciliation idempotent.
        input.onmidimessage = (event) => {
          const data = event.data;
          if (!data || data.length === 0) return;
          const isController =
            nextController && controllerInput?.id === input.id && data[0] < 0xf0;
          if (data[0] >= 0xf0) return;
          node.port.postMessage({
            kind: isController ? "controller_midi" : "midi",
            data: [data[0], data[1] ?? 0, data[2] ?? 0],
            length: Math.min(data.length, 3),
          });
        };
      }

      if (pairChanged && keyLabConnected) {
        node.port.postMessage({ kind: "controller_connection", connected: false });
      }
      keyLabInputId = controllerInput?.id ?? null;
      keyLabOutput = controllerOutput ?? null;
      keyLabConnected = nextController;
      keyLabDisplayConnected = Boolean(controllerInput && controllerOutput && sysex);
      if (pairChanged && nextController) {
        keyLabReleaseSent = false;
        node.port.postMessage({
          kind: "controller_setting",
          color: parseControllerColor(savedControllerColor()),
        });
        node.port.postMessage({ kind: "controller_connection", connected: true });
      }
    };
    webMidiReconcile = reconcile;
    reconcile();
    access.onstatechange = reconcile;
  };

  // LITTLE needs SysEx for the OLED and LEDs. If that permission is denied,
  // retry ordinary Web MIDI so keys and controls keep working as instruments.
  void midi
    .call(navigator, { sysex: true })
    .then((access) => connect(access, true))
    .catch(() =>
      midi
        .call(navigator, { sysex: false })
        .then((access) => connect(access, false))
        .catch(() => undefined),
    );
}

function requestControllerCatalog(): Promise<{
  controllers: Array<{
    id: string;
    name: string;
    version: string;
    enabled: boolean;
    trust: string;
    runtime: string;
    devices: number;
    settings: Array<{
      id: string;
      name: string;
      kind: string;
      default: string;
      page: string | null;
    }>;
  }>;
  error?: string;
}> {
  const node = engine;
  if (!node) return Promise.reject(new Error("The RackForge engine is not running."));
  const id = nextRequestId++;
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      pending.delete(id);
      reject(new Error("RackForge did not answer the controller catalog request."));
    }, 10_000);
    pending.set(id, {
      resolve: (response) => resolve(JSON.parse(response)),
      reject,
      timeout,
    });
    node.port.postMessage({ kind: "controller_catalog", id });
  });
}

export function isKeyLabMainEndpoint(
  name: string | null,
  manufacturer: string | null = null,
): boolean {
  const folded = (name ?? "").trim().toLowerCase();
  const maker = (manufacturer ?? "").trim().toLowerCase();
  if (
    !folded ||
    !["keylab", "kl essential"].some((part) => folded.includes(part)) ||
    ["mcu", "hui", "dinthru", "alv"].some((part) => folded.includes(part))
  ) {
    return false;
  }
  if (folded.endsWith("midi")) return true;

  // Chromium on Android assigns every Web MIDI port the USB product name,
  // not the desktop port label. The KeyLab's main endpoint therefore appears
  // simply as "KeyLab Essential 61 mk3". Android preserves port ordering, so
  // resolveKeyLabTransport pairs the first matching input and output.
  const androidProductNames = new Set([
    "keylab essential 61 mk3",
    "arturia keylab essential 61 mk3",
    "kl essential 61 mk3",
  ]);
  return androidProductNames.has(folded) || maker.includes("arturia");
}

export function resolveKeyLabTransport<
  Input extends BrowserMidiEndpoint,
  Output extends BrowserMidiEndpoint,
>(
  inputs: Iterable<Input>,
  outputs: Iterable<Output>,
  sysex: boolean,
): { input?: Input; output?: Output } {
  const input = [...inputs].find(
    (candidate) =>
      candidate.state === "connected" &&
      isKeyLabMainEndpoint(candidate.name, candidate.manufacturer),
  );
  const output = sysex
    ? [...outputs].find(
        (candidate) =>
          candidate.state === "connected" &&
          isKeyLabMainEndpoint(candidate.name, candidate.manufacturer),
      )
    : undefined;
  return { input, output };
}

function savedControllerColor(): string {
  try {
    return localStorage.getItem(CONTROLLER_COLOR_KEY) ?? DEFAULT_CONTROLLER_COLOR;
  } catch {
    return DEFAULT_CONTROLLER_COLOR;
  }
}

export function parseControllerColor(value: string): [number, number, number] {
  const normalized = /^#[0-9a-f]{6}$/i.test(value) ? value : DEFAULT_CONTROLLER_COLOR;
  return [
    Number.parseInt(normalized.slice(1, 3), 16),
    Number.parseInt(normalized.slice(3, 5), 16),
    Number.parseInt(normalized.slice(5, 7), 16),
  ];
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
/**
 * Uploads a plugin has been given permission to use, held until it asks for
 * them to be installed. A networked host keeps the same handles; here they
 * live for as long as the page does.
 */
const grants = new Map<
  string,
  { pluginId: string; resourceId: string; name: string; archive: Uint8Array }
>();
let nextSelectionId = 1;

/** Sends one plugin-store operation to the engine and returns its answer. */
function sendPackage(
  action:
    | "inspect"
    | "install"
    | "catalog"
    | "activate"
    | "deactivate"
    | "uninstall"
    | "import_resource"
    | "resource_status",
  payload: Uint8Array = new Uint8Array(),
): Promise<{
  ok: boolean;
  error?: string;
  preview?: PackagePreview;
  installed?: InstalledPackage;
  catalog?: CatalogEntry[];
  imported?: unknown;
  resources?: Array<{ resource_id: string; installed: boolean }>;
}> {
  const node = engine;
  if (!node) {
    return Promise.reject(new Error("The RackForge engine is not running."));
  }
  const id = nextRequestId++;
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => {
      pending.delete(id);
      reject(new Error("RackForge did not finish processing the plugin package in time."));
    }, 120_000);
    pending.set(id, {
      resolve: (response) => resolve(JSON.parse(response)),
      reject,
      timeout,
    });
    // Keep the selection for the later install, but transfer the worklet copy
    // instead of asking structured clone to duplicate another 56+ MB buffer.
    const transferred = payload.slice();
    node.port.postMessage(
      { kind: "package", id, action, payload: transferred },
      [transferred.buffer],
    );
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

/**
 * The host reports paths inside a package, since it has no idea where the page
 * publishes them. This turns them into the URLs the interface loads.
 */
function withAssetUrls(plugin: CatalogEntry, serving: boolean): PluginWebDescriptor {
  const asset = (entry: string) => pluginAssetUrl(plugin.package_root, entry, plugin.version);
  const assetsAvailable = canServePluginAssets(plugin.package_root, serving);
  return {
    ...plugin,
    branding: assetsAvailable && plugin.branding
      ? {
          icon_url: asset(plugin.branding.icon),
          banner_url: asset(plugin.branding.banner),
          splash_url: asset(plugin.branding.splash),
          background_color: plugin.branding.background_color ?? undefined,
          accent_color: plugin.branding.accent_color ?? undefined,
        }
      : null,
    // Without a worker to serve them, a plugin's own pages have no address,
    // and the interface says so rather than loading a broken frame.
    surfaces: assetsAvailable
      ? plugin.surfaces.map((surface) => ({
          kind: surface.kind,
          entry_url: asset(surface.entry),
        }))
      : [],
  };
}

function catalogNeedsAssetWorker(catalog: CatalogEntry[]): boolean {
  return catalog.some((plugin) =>
    !canServePluginAssets(plugin.package_root, false)
    && (plugin.branding !== null || plugin.surfaces.length > 0));
}

/** One plugin as the host describes it, before its files have an address. */
interface CatalogEntry extends Omit<PluginWebDescriptor, "branding" | "surfaces"> {
  package_root: string;
  branding: {
    icon: string;
    banner: string;
    splash: string;
    background_color?: string | null;
    accent_color?: string | null;
  } | null;
  surfaces: Array<{ kind: "play" | "config"; entry: string }>;
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
    const answer = await sendPackage("catalog");
    if (!answer.ok || !answer.catalog) {
      throw new HostRequestError(answer.error ?? "The plugin catalog is unavailable.", 503);
    }
    const serving = catalogNeedsAssetWorker(answer.catalog) ? await whenServing() : false;
    return answer.catalog.map((plugin) => withAssetUrls(plugin, serving)) as T;
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
        midi_inputs: [...midiInputNames],
      },
      preferences: {
        schema_version: 1,
        driver: "Web Audio",
        output_device: "Browser audio output",
        sample_rate_hz: rate,
        buffer_frames: RENDER_FRAMES,
        output_gain_db: 0,
        midi_inputs: [...midiInputNames],
      },
      runtime: {
        running: Boolean(engine),
        stream_health: engine ? "healthy" : "lost",
        sample_rate: rate,
        buffer_size_frames: RENDER_FRAMES,
      },
      runtime_status: engine ? "running" : "stopped",
    } satisfies HostAudioSettings as T;
  }
  if (path === "/api/v1/controllers" && method === "GET") {
    const answer = await requestControllerCatalog();
    if (answer.error) throw new HostRequestError(answer.error, 503);
    const runtime = !webMidiSupported
      ? "Browser · Web MIDI unavailable"
      : keyLabDisplayConnected
        ? "Browser · connected · LITTLE active"
        : keyLabConnected
          ? webMidiSysex
            ? "Browser · controls active · waiting for display output"
            : "Browser · controls active · SysEx required for LITTLE"
          : !webMidiSysex
            ? "Browser · SysEx permission required for LITTLE"
            : "Browser · waiting for device";
    return {
      controllers: answer.controllers.map((controller) => ({
        ...controller,
        runtime,
        settings: controller.settings.map((setting) => ({
          ...setting,
          value:
            setting.id === "key-light-color" ? savedControllerColor() : setting.default,
        })),
      })),
    } as T;
  }
  if (
    path === "/api/v1/controllers/org.rackforge.arturia-keylab-essential-mk3/settings" &&
    method === "PUT"
  ) {
    const request = JSON.parse(String(init.body ?? "{}")) as {
      values?: Record<string, string>;
    };
    const value = request.values?.["key-light-color"];
    if (value !== undefined) {
      if (!/^#[0-9a-f]{6}$/i.test(value)) {
        throw new HostRequestError("The controller color must use #rrggbb.", 400);
      }
      try {
        localStorage.setItem(CONTROLLER_COLOR_KEY, value);
      } catch {
        throw new HostRequestError("The browser could not save this setting.", 507);
      }
      engine?.port.postMessage({
        kind: "controller_setting",
        color: parseControllerColor(value),
      });
    }
    return { status: "ok" } as T;
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
    const pluginId = decodeURIComponent(
      path.slice("/api/v1/plugins/".length, -"/activate".length),
    );
    const answer = await sendPackage(
      "activate",
      new TextEncoder().encode(JSON.stringify({ plugin_id: pluginId, active: true })),
    );
    if (!answer.ok) {
      throw new HostRequestError(answer.error ?? "This plugin could not be activated.", 409);
    }
    await publishSnapshot();
    return { status: "active", plugin_id: pluginId } as T;
  }
  if (path.startsWith("/api/v1/plugins/") && path.endsWith("/deactivate") && method === "POST") {
    const pluginId = decodeURIComponent(
      path.slice("/api/v1/plugins/".length, -"/deactivate".length),
    );
    const answer = await sendPackage(
      "deactivate",
      new TextEncoder().encode(JSON.stringify({ plugin_id: pluginId, active: false })),
    );
    if (!answer.ok) {
      throw new HostRequestError(answer.error ?? "This plugin could not be deactivated.", 409);
    }
    await publishSnapshot();
    return { status: "inactive", plugin_id: pluginId } as T;
  }
  if (path === "/api/v1/resources/mounts" && method === "GET") {
    // There is no host storage to browse: a page can only be given a file.
    return [] as unknown as T;
  }
  if (path === "/api/v1/resources/bind-selection" && method === "POST") {
    const request = JSON.parse(String(init.body)) as {
      selection_id?: string;
      plugin_id?: string;
      resource_id?: string;
    };
    const selection = selectionOf(init.body);
    const grantId = `browser.grant.${nextSelectionId++}`;
    grants.set(grantId, {
      pluginId: request.plugin_id ?? "",
      resourceId: request.resource_id ?? "",
      name: selection.name,
      archive: selection.archive,
    });
    if (request.selection_id) selections.delete(request.selection_id);
    return {
      grant_id: grantId,
      resource_id: request.resource_id ?? "",
      display_name: selection.name,
      kind: "file",
      size: selection.archive.length,
    } as T;
  }
  if (path === "/api/v1/resources/grants" && method === "POST") {
    const { plugin_id: pluginId } = JSON.parse(String(init.body)) as { plugin_id?: string };
    return [...grants.entries()]
      .filter(([, grant]) => grant.pluginId === pluginId)
      .map(([grantId, grant]) => ({
        grant_id: grantId,
        resource_id: grant.resourceId,
        display_name: grant.name,
        kind: "file",
        size: grant.archive.length,
      })) as T;
  }
  if (path === "/api/v1/resources/status" && method === "POST") {
    const { plugin_id: pluginId } = JSON.parse(String(init.body)) as { plugin_id?: string };
    const answer = await sendPackage(
      "resource_status",
      new TextEncoder().encode(pluginId ?? ""),
    );
    if (!answer.ok) {
      throw new HostRequestError(answer.error ?? "Resource status is unavailable.", 404);
    }
    return answer.resources as T;
  }
  if (path === "/api/v1/resources/browse" && method === "POST") {
    // A granted upload is one file, so there is nothing inside it to list.
    return [] as unknown as T;
  }
  if (path === "/api/v1/resources/load" && method === "POST") {
    const request = JSON.parse(String(init.body)) as {
      plugin_id?: string;
      target_resource_id?: string;
      grant_id?: string;
      bundle?: string | null;
    };
    if (request.bundle) {
      throw new HostRequestError(
        "This browser received only the NKI file, without its sibling samples. Choose an .rfbank archive instead.",
        400,
      );
    }
    const grant = request.grant_id ? grants.get(request.grant_id) : undefined;
    if (!grant) {
      throw new HostRequestError("This file is no longer available; choose it again.", 404);
    }
    const header = new TextEncoder().encode(
      `${JSON.stringify({
        plugin_id: request.plugin_id ?? grant.pluginId,
        resource_id: request.target_resource_id ?? grant.resourceId,
      })}\n`,
    );
    const payload = new Uint8Array(header.length + grant.archive.length);
    payload.set(header);
    payload.set(grant.archive, header.length);
    const answer = await sendPackage("import_resource", payload);
    if (!answer.ok) {
      throw new HostRequestError(answer.error ?? "This file could not be installed.", 400);
    }
    grants.delete(request.grant_id!);
    await publishSnapshot();
    return answer.imported as T;
  }
  if (path.startsWith("/api/v1/plugins/") && method === "DELETE") {
    const pluginId = decodeURIComponent(
      path.slice("/api/v1/plugins/".length).split("?")[0],
    );
    const options = init.body ? (JSON.parse(String(init.body)) as object) : {};
    const answer = await sendPackage(
      "uninstall",
      new TextEncoder().encode(JSON.stringify({ plugin_id: pluginId, ...options })),
    );
    if (!answer.ok) {
      throw new HostRequestError(answer.error ?? "This plugin could not be removed.", 400);
    }
    await publishSnapshot();
    return (answer as { removed?: unknown }).removed as T;
  }
  if (path.startsWith("/api/v1/plugins/") && method === "GET") {
    const pluginId = decodeURIComponent(path.slice("/api/v1/plugins/".length));
    const answer = await sendPackage("catalog");
    const descriptor = answer.catalog?.find((plugin) => plugin.plugin_id === pluginId);
    if (!descriptor) {
      throw new HostRequestError("This plugin is not loaded.", 404);
    }
    const serving = catalogNeedsAssetWorker([descriptor]) ? await whenServing() : false;
    return withAssetUrls(descriptor, serving) as T;
  }
  throw new HostRequestError(
    "The browser host does not provide this; it is part of an installed RackForge.",
    501,
  );
}
