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
}

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

class RackForgeEngine extends AudioWorkletProcessor {
  #host: HostExports | null = null;
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
      case "request":
        this.#post({
          kind: "response",
          id: command.id,
          response: this.#request(command.request),
        });
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
    const wasi = new WASI(
      ["rackforge"],
      [],
      [
        new OpenFile(new File([])),
        ConsoleStdout.lineBuffered((line) => console.log(`[rackforge] ${line}`)),
        ConsoleStdout.lineBuffered((line) => console.warn(`[rackforge] ${line}`)),
        new PreopenDirectory(`/${DATA_ROOT}`, seedDirectory(files)),
      ],
    );
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
