/**
 * Messages between the page and the RackForge engine running in its audio
 * worklet.
 *
 * The engine is the host: it owns the session, the performance library and the
 * plugins. The page owns the screen. They exchange exactly what a networked
 * RackForge exchanges over its socket — control requests and responses — plus
 * the few things a page must supply that an appliance gets from hardware:
 * storage contents at boot and MIDI as it arrives.
 */

/** One file the engine should find in its storage when it boots. */
export interface SeedFile {
  /** Path below the RackForge data root, using forward slashes. */
  path: string;
  bytes: Uint8Array;
}

export interface BootMessage {
  kind: "boot";
  /**
   * The host component, as bytes.
   *
   * A compiled `WebAssembly.Module` cannot cross into an audio worklet: the
   * worklet is a separate agent cluster, and the message is dropped rather
   * than refused. The worklet compiles these itself, which it may do
   * synchronously because it is not the main thread.
   */
  wasm: Uint8Array;
  files: SeedFile[];
  maximumFrames: number;
  channels: number;
}

export interface RequestMessage {
  kind: "request";
  id: number;
  /** A `ControlRequest`, already encoded as JSON. */
  request: string;
}

/**
 * Validates or installs a `.rfplugin`. The engine owns the plugin store, so
 * the page hands the archive over rather than unpacking anything itself.
 */
export interface PackageMessage {
  kind: "package";
  id: number;
  action: "inspect" | "install";
  archive: Uint8Array;
}

export interface MidiMessage {
  kind: "midi";
  data: [number, number, number];
  length: number;
}

export type EngineCommand =
  | BootMessage
  | RequestMessage
  | PackageMessage
  | MidiMessage;

/**
 * Sent as soon as the processor exists. A port message posted before that is
 * not delivered, so the page waits for this before it boots the engine.
 */
export interface ReadyMessage {
  kind: "ready";
}

export interface BootedMessage {
  kind: "booted";
  ok: boolean;
  error?: string;
  warnings: string[];
}

export interface ResponseMessage {
  kind: "response";
  id: number;
  /** A `ControlResponse`, encoded as JSON. */
  response: string;
}

/**
 * Everything the host has written, reported after it writes. The page files it
 * so the next visit starts where this one left off.
 */
export interface StorageMessage {
  kind: "storage";
  files: SeedFile[];
}

/**
 * The engine publishes a revision whenever it changes, so the page can refresh
 * without polling the session on a timer.
 */
export interface RevisionMessage {
  kind: "revision";
  revision: number;
}

export type EngineEvent =
  | ReadyMessage
  | BootedMessage
  | ResponseMessage
  | StorageMessage
  | RevisionMessage;

/**
 * Storage the host does not own: RackForge ships these with the site, and a
 * stored copy would go stale as soon as the site was rebuilt.
 */
export const PACKAGED_STORAGE_PREFIX = "plugins/";

/** Name the page registers the engine processor under. */
export const ENGINE_PROCESSOR = "rackforge-engine";
