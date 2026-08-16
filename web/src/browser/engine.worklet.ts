/**
 * The RackForge host, running on the page's audio thread.
 *
 * A page cannot share memory with its audio thread unless it is cross-origin
 * isolated, and a site served from GitHub Pages cannot set the headers that
 * would make it so. Rather than split the host in two and try to keep the
 * halves in step, the whole host lives here: session, performance library,
 * plugins and rendering, exactly as they live together in a native RackForge
 * process. The page above it is only a screen, and talks to this thread the
 * way a remote surface talks to an appliance — with control requests.
 */

import { installTextCoding } from "./textCoding";
import {
  ConsoleStdout,
  Directory,
  File,
  Inode,
  OpenFile,
  PreopenDirectory,
  WASI,
} from "@bjorn3/browser_wasi_shim";
import { PluginHost } from "./pluginHost";
import {
  ENGINE_PROCESSOR,
  PACKAGED_STORAGE_PREFIX,
  type EngineCommand,
  type EngineEvent,
  type SeedFile,
} from "./protocol";

// Installed before anything else runs: the WASI shim and the engine both
// decode UTF-8, and this scope has no built-in coder.
installTextCoding();

/** Where the host expects its private storage, matching `host::DATA_ROOT`. */
const DATA_ROOT = "rackforge";

interface HostExports {
  memory: WebAssembly.Memory;
  _initialize?: () => void;
  rf_alloc: (length: number) => number;
  rf_free: (pointer: number, length: number) => void;
  rf_open: (sampleRate: number, maximumFrames: number, channels: number) => number;
  rf_request: (pointer: number, length: number) => number;
  rf_response_ptr: () => number;
  rf_push_midi: (
    frame: number,
    status: number,
    data1: number,
    data2: number,
    length: number,
  ) => void;
  rf_render: (frames: number) => number;
  rf_inspect_plugin: (pointer: number, length: number) => number;
  rf_install_plugin: (pointer: number, length: number) => number;
}

/**
 * Control requests that only read. Everything else may have written to
 * storage, so the page is told to file a fresh copy afterwards.
 */
const READ_ONLY_OPERATIONS = new Set([
  "snapshot",
  "performance_snapshot",
  "events",
  "audio_snapshot",
  "plugin_presets",
  "plugin_preset",
  "plugin_parameters",
  "plugin_state_parameters",
]);

/** Builds the directory tree the host boots against from a flat file list. */
function seedDirectory(files: SeedFile[]): Map<string, Inode> {
  const root = new Map<string, Inode>();
  for (const file of files) {
    const segments = file.path.split("/").filter((segment) => segment.length > 0);
    const name = segments.pop();
    if (!name) {
      continue;
    }
    let directory = root;
    for (const segment of segments) {
      let child = directory.get(segment);
      if (!(child instanceof Directory)) {
        child = new Directory(new Map<string, Inode>());
        directory.set(segment, child);
      }
      directory = (child as Directory).contents;
    }
    directory.set(name, new File(file.bytes));
  }
  return root;
}

/** Reads the operation name out of a control request without validating it. */
function operationOf(request: string): string {
  try {
    return String((JSON.parse(request) as { op?: unknown }).op ?? "");
  } catch {
    return "";
  }
}

/** Flattens a directory tree into the path/bytes pairs the page stores. */
function collectFiles(contents: Map<string, Inode>, prefix: string, files: SeedFile[]) {
  for (const [name, inode] of contents) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (inode instanceof Directory) {
      collectFiles(inode.contents, path, files);
    } else if (inode instanceof File) {
      files.push({ path, bytes: inode.data });
    }
  }
}

class RackForgeEngine extends AudioWorkletProcessor {
  #host: HostExports | null = null;
  /** The root the host reads and writes through WASI. */
  #storage: PreopenDirectory | null = null;
  #pluginHost = new PluginHost();
  #decoder = new TextDecoder();
  #encoder = new TextEncoder();
  #channels = 2;
  #frames = 128;
  #failed = false;

  constructor() {
    super();
    this.port.onmessage = (event: MessageEvent<EngineCommand>) => {
      try {
        this.#handle(event.data);
      } catch (error) {
        this.#post({
          kind: "booted",
          ok: false,
          error: error instanceof Error ? error.message : String(error),
          warnings: [],
        });
      }
    };
    this.#post({ kind: "ready" });
  }

  #handle(command: EngineCommand) {
    switch (command.kind) {
      case "boot":
        this.#boot(command.wasm, command.files, command.maximumFrames, command.channels);
        break;
      case "request": {
        const response = this.#request(command.request);
        this.#post({ kind: "response", id: command.id, response });
        if (!READ_ONLY_OPERATIONS.has(operationOf(command.request))) {
          this.#publishStorage();
        }
        break;
      }
      case "package":
        this.#post({
          kind: "response",
          id: command.id,
          response: this.#package(command.action, command.archive),
        });
        this.#publishStorage();
        break;
      case "midi": {
        const [status, data1, data2] = command.data;
        this.#host?.rf_push_midi(0, status, data1, data2, command.length);
        break;
      }
    }
  }

  #boot(
    wasm: Uint8Array,
    files: SeedFile[],
    maximumFrames: number,
    channels: number,
  ) {
    const storage = new PreopenDirectory(`/${DATA_ROOT}`, seedDirectory(files));
    const wasi = new WASI(
      ["rackforge"],
      [],
      [
        new OpenFile(new File([])),
        ConsoleStdout.lineBuffered((line) => console.log(`[rackforge] ${line}`)),
        ConsoleStdout.lineBuffered((line) => console.warn(`[rackforge] ${line}`)),
        storage,
      ],
    );
    this.#storage = storage;
    const instance = new WebAssembly.Instance(new WebAssembly.Module(wasm.slice().buffer), {
      wasi_snapshot_preview1: wasi.wasiImport,
      rackforge_plugin_host: this.#pluginHost.imports,
    });
    const exports = instance.exports as unknown as HostExports;
    // A cdylib is a reactor: it initialises rather than running a main.
    wasi.initialize(instance as { exports: { memory: WebAssembly.Memory } });
    this.#pluginHost.attach(exports.memory);
    this.#host = exports;
    this.#frames = maximumFrames;
    this.#channels = channels;

    const length = exports.rf_open(sampleRate, maximumFrames, channels);
    const status = JSON.parse(this.#readResponse(length)) as {
      ok: boolean;
      error?: string;
      warnings?: string[];
    };
    if (!status.ok) {
      this.#host = null;
      this.#failed = true;
    }
    this.#post({
      kind: "booted",
      ok: status.ok,
      error: status.error,
      warnings: status.warnings ?? [],
    });
  }

  #request(request: string): string {
    const host = this.#host;
    if (!host) {
      return JSON.stringify({
        status: "error",
        code: "unavailable",
        message: "the RackForge engine is not running",
      });
    }
    const encoded = this.#encoder.encode(request);
    const pointer = host.rf_alloc(encoded.length);
    new Uint8Array(host.memory.buffer, pointer, encoded.length).set(encoded);
    try {
      return this.#readResponse(host.rf_request(pointer, encoded.length));
    } finally {
      host.rf_free(pointer, encoded.length);
    }
  }

  /**
   * Hands one archive to the host, which validates it and — for an install —
   * unpacks it into the plugin store and reloads the session over it.
   */
  #package(action: "inspect" | "install", archive: Uint8Array): string {
    const host = this.#host;
    if (!host) {
      return JSON.stringify({ ok: false, error: "the RackForge engine is not running" });
    }
    const pointer = host.rf_alloc(archive.length);
    new Uint8Array(host.memory.buffer, pointer, archive.length).set(archive);
    try {
      const length =
        action === "install"
          ? host.rf_install_plugin(pointer, archive.length)
          : host.rf_inspect_plugin(pointer, archive.length);
      return this.#readResponse(length);
    } finally {
      host.rf_free(pointer, archive.length);
    }
  }

  /** Reports what the host has written, so the page can keep it. */
  #publishStorage() {
    const storage = this.#storage;
    if (!storage) return;
    const files: SeedFile[] = [];
    collectFiles(storage.dir.contents, "", files);
    this.#post({
      kind: "storage",
      files: files.filter((file) => !file.path.startsWith(PACKAGED_STORAGE_PREFIX)),
    });
  }

  #readResponse(length: number): string {
    const host = this.#host;
    if (!host || length <= 0) {
      return "{}";
    }
    const pointer = host.rf_response_ptr();
    return this.#decoder.decode(new Uint8Array(host.memory.buffer, pointer, length));
  }

  #post(event: EngineEvent) {
    this.port.postMessage(event);
  }

  process(_inputs: Float32Array[][], outputs: Float32Array[][]): boolean {
    const output = outputs[0];
    const host = this.#host;
    if (!output || output.length === 0) {
      return !this.#failed;
    }
    if (!host) {
      for (const channel of output) {
        channel.fill(0);
      }
      return !this.#failed;
    }

    const frames = Math.min(output[0].length, this.#frames);
    const pointer = host.rf_render(frames);
    if (pointer === 0) {
      for (const channel of output) {
        channel.fill(0);
      }
      return true;
    }
    // Deinterleave: RackForge renders interleaved frames, Web Audio wants one
    // planar buffer per channel.
    const rendered = new Float32Array(host.memory.buffer, pointer, frames * this.#channels);
    for (let channel = 0; channel < output.length; channel += 1) {
      const source = Math.min(channel, this.#channels - 1);
      const destination = output[channel];
      for (let frame = 0; frame < frames; frame += 1) {
        destination[frame] = rendered[frame * this.#channels + source];
      }
    }
    return true;
  }
}

registerProcessor(ENGINE_PROCESSOR, RackForgeEngine);
