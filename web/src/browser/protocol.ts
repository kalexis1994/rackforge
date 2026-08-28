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
  action:
    | "inspect"
    | "install"
    | "catalog"
    | "activate"
    | "deactivate"
    | "uninstall"
    | "import_resource"
    | "resource_status";
  /** The archive to inspect or install, or the plugin id to remove. */
  payload: Uint8Array;
}

export interface MidiMessage {
  kind: "midi";
  data: [number, number, number];
  length: number;
}

export interface ControllerMidiMessage {
  kind: "controller_midi";
  data: [number, number, number];
  length: number;
}

export interface ControllerConnectionMessage {
  kind: "controller_connection";
  connected: boolean;
}

export interface ControllerSettingMessage {
  kind: "controller_setting";
  color: [number, number, number];
}

export interface ControllerCatalogMessage {
  kind: "controller_catalog";
  id: number;
}

export interface ControllerRestorePlanMessage {
  kind: "controller_restore_plan";
  id: number;
}

/** The page attaches a built render pool to the worklet. */
export interface PoolAttachCommand {
  kind: "pool_attach";
  buffer: SharedArrayBuffer;
  workerCount: number;
  epoch: number;
}

export type EngineCommand =
  | BootMessage
  | RequestMessage
  | PackageMessage
  | MidiMessage
  | ControllerMidiMessage
  | ControllerConnectionMessage
  | ControllerSettingMessage
  | ControllerCatalogMessage
  | PoolAttachCommand
  | ControllerRestorePlanMessage;

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
  /**
   * Identifies the storage snapshot that must be durably published before
   * this response is observable by its caller. Package lifecycle operations
   * use their request id, which keeps the protocol deterministic without a
   * timing assumption between two MessagePort events.
   */
  storage_operation_id?: number;
}

/**
 * Everything the host has written, reported after it writes. The page files it
 * so the next visit starts where this one left off.
 */
export interface StorageMessage {
  kind: "storage";
  files: SeedFile[];
  /** Package request whose response is blocked on this snapshot. */
  operation_id?: number;
  /** Whether this mutation can add, replace, or remove public package files. */
  publish_plugin_assets?: boolean;
}

/**
 * The engine publishes a revision whenever it changes, so the page can refresh
 * without polling the session on a timer.
 */
export interface RevisionMessage {
  kind: "revision";
  revision: number;
}

export interface ControllerOutputMessage {
  kind: "controller_output";
  messages: Array<{ bytes: number[]; settle_after_ms: number }>;
}

/**
 * The worklet asks the page to build a render pool: the worklet itself can
 * neither spawn workers nor allocate a SharedArrayBuffer it could share
 * forward. Carries everything a pool needs so the page stays policy-free.
 */
export interface PoolRequestEvent {
  kind: "pool_request";
  geometry: {
    maxUnits: number;
    dispatchStride: number;
    mixSlotSamples: number;
    sharedCapacity: number;
  };
  prepare: {
    sampleRate: number;
    maximumFrames: number;
    inputChannels: number;
    outputChannels: number;
  };
  component: ArrayBuffer;
  epoch: number;
}

export type EngineEvent =
  | ReadyMessage
  | BootedMessage
  | ResponseMessage
  | StorageMessage
  | RevisionMessage
  | PoolRequestEvent
  | ControllerOutputMessage;

/** Constructs the only valid event order for a successful package mutation. */
export function linkedPackageMutationEvents(
  operationId: number,
  response: string,
  files: SeedFile[],
  publishPluginAssets: boolean,
): [StorageMessage, ResponseMessage] {
  return [
    {
      kind: "storage",
      files,
      operation_id: operationId,
      ...(publishPluginAssets ? { publish_plugin_assets: true } : {}),
    },
    {
      kind: "response",
      id: operationId,
      response,
      storage_operation_id: operationId,
    },
  ];
}

/**
 * Waits for the storage publication explicitly linked by a response.
 *
 * Kept independent of the browser globals so the real ordering contract can
 * be tested without an AudioWorklet. A linked response without its snapshot
 * is a protocol violation, not a reason to guess or sleep.
 */
export async function waitForLinkedStoragePublication(
  event: ResponseMessage,
  publications: ReadonlyMap<number, Promise<void>>,
): Promise<void> {
  const operationId = event.storage_operation_id;
  if (operationId === undefined) return;
  const publication = publications.get(operationId);
  if (!publication) {
    throw new Error(
      `RackForge received package response ${event.id} before storage operation ${operationId}.`,
    );
  }
  await publication;
}

/**
 * Converts a worklet exception into the response the caller is waiting for.
 * A package failure used to masquerade as a second boot failure, leaving its
 * request promise unresolved and the interface on "Validating…" forever.
 */
export function engineFailureEvent(
  command: EngineCommand,
  message: string,
): EngineEvent | null {
  if (command.kind === "package") {
    return {
      kind: "response",
      id: command.id,
      response: JSON.stringify({ ok: false, error: message }),
    };
  }
  if (command.kind === "request") {
    return {
      kind: "response",
      id: command.id,
      response: JSON.stringify({ status: "error", code: "internal", message }),
    };
  }
  if (command.kind === "controller_catalog") {
    return {
      kind: "response",
      id: command.id,
      response: JSON.stringify({ controllers: [], error: message }),
    };
  }
  if (command.kind === "controller_restore_plan") {
    return {
      kind: "response",
      id: command.id,
      response: JSON.stringify([]),
    };
  }
  if (command.kind === "boot") {
    return { kind: "booted", ok: false, error: message, warnings: [] };
  }
  return null;
}

/**
 * Storage the host does not own: RackForge ships these with the site, and a
 * stored copy would go stale as soon as the site was rebuilt.
 */
export const PACKAGED_STORAGE_PREFIX = "plugins/";

/** Name the page registers the engine processor under. */
export const ENGINE_PROCESSOR = "rackforge-engine";
