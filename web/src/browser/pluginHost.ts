/**
 * The `rackforge_plugin_host` imports the RackForge browser host calls into.
 *
 * Portable RackForge plugins are `wasm-v1` components that may not import host
 * functions, so a page can run one directly. What it cannot do is instantiate a
 * module from inside another module: that needs JavaScript. This file is that
 * JavaScript, and nothing more — every decision about what to call and when is
 * made in Rust, on both sides of the boundary.
 */

/**
 * Plugin exports addressed by index. The order is the contract shared with
 * `rackforge-plugin-runtime`'s browser backend; changing it changes the ABI.
 */
const EXPORTS = [
  "rackforge_abi_version",
  "rackforge_input_ptr",
  "rackforge_output_ptr",
  "rackforge_midi_ptr",
  "rackforge_parameter_ptr",
  "rackforge_transfer_ptr",
  "rackforge_exchange_input_ptr",
  "rackforge_capacity_input_samples",
  "rackforge_capacity_output_samples",
  "rackforge_capacity_midi_events",
  "rackforge_capacity_parameter_events",
  "rackforge_capacity_transfer_bytes",
  "rackforge_initialize",
  "rackforge_prepare",
  "rackforge_set_parameter",
  "rackforge_get_parameter",
  "rackforge_reset",
  "rackforge_resource_begin",
  "rackforge_resource_write",
  "rackforge_resource_end",
  "rackforge_preset_catalog",
  "rackforge_load_preset",
  "rackforge_save_state",
  "rackforge_load_state",
  "rackforge_process",
  "rackforge_program_editing_capabilities",
  "rackforge_program_begin_edit",
  "rackforge_program_prepare_save",
  "rackforge_program_install",
  "rackforge_program_preview",
  "rackforge_program_editor_view",
  "rackforge_program_apply_edit",
] as const;

interface PluginInstance {
  exports: Record<string, unknown>;
  memory: WebAssembly.Memory;
}

/**
 * Owns the compiled components and their instances, and copies bytes between
 * a plugin's linear memory and the host's. The two modules cannot share memory
 * — a page without cross-origin isolation has no `SharedArrayBuffer` — so a
 * block of audio is copied in and out, once each way.
 */
export class PluginHost {
  #modules = new Map<number, WebAssembly.Module>();
  #instances = new Map<number, PluginInstance>();
  #nextHandle = 1;
  #error: string | null = null;
  #hostMemory: WebAssembly.Memory | null = null;

  /** Called once the host module exists, since imports are built before it. */
  attach(memory: WebAssembly.Memory) {
    this.#hostMemory = memory;
  }

  get imports() {
    return {
      rf_compile: (pointer: number, length: number) => this.#compile(pointer, length),
      rf_module_release: (module: number) => {
        this.#modules.delete(module);
      },
      rf_instantiate: (module: number) => this.#instantiate(module),
      rf_instance_release: (instance: number) => {
        this.#instances.delete(instance);
      },
      rf_memory_size: (instance: number) =>
        this.#guard(-1, () => this.#instance(instance).memory.buffer.byteLength),
      rf_memory_read: (instance: number, offset: number, destination: number, length: number) =>
        this.#guard(-1, () => {
          const source = new Uint8Array(
            this.#instance(instance).memory.buffer,
            offset,
            length,
          );
          this.#hostBytes(destination, length).set(source);
          return length;
        }),
      rf_memory_write: (instance: number, offset: number, source: number, length: number) =>
        this.#guard(-1, () => {
          const destination = new Uint8Array(
            this.#instance(instance).memory.buffer,
            offset,
            length,
          );
          destination.set(this.#hostBytes(source, length));
          return length;
        }),
      rf_export_present: (instance: number, index: number) =>
        this.#guard(0, () => (this.#lookup(instance, index) ? 1 : 0)),
      rf_call_0: (instance: number, index: number) =>
        this.#guard(-1, () => this.#call(instance, index) as number),
      rf_call_1: (instance: number, index: number, argument: number) =>
        this.#guard(-1, () => this.#call(instance, index, argument) as number),
      rf_call_f64: (instance: number, index: number, argument: number) =>
        this.#guard(Number.NaN, () => this.#call(instance, index, argument) as number),
      rf_call_set_parameter: (instance: number, index: number, value: number) =>
        this.#guard(-1, () => this.#call(instance, 14, index, value) as number),
      rf_call_prepare: (
        instance: number,
        sampleRate: number,
        maximumFrames: number,
        inputChannels: number,
        outputChannels: number,
      ) =>
        this.#guard(
          -1,
          () =>
            this.#call(
              instance,
              13,
              sampleRate,
              maximumFrames,
              inputChannels,
              outputChannels,
            ) as number,
        ),
      rf_call_resource_begin: (instance: number, idLength: number, totalBytes: bigint) =>
        this.#guard(-1, () => this.#call(instance, 17, idLength, totalBytes) as number),
      rf_call_resource_write: (instance: number, offset: bigint, length: number) =>
        this.#guard(-1, () => this.#call(instance, 18, offset, length) as number),
      rf_call_process: (
        instance: number,
        frames: number,
        inputChannels: number,
        outputChannels: number,
        midiEvents: number,
        parameterEvents: number,
      ) =>
        this.#guard(
          -1,
          () =>
            this.#call(
              instance,
              24,
              frames,
              inputChannels,
              outputChannels,
              midiEvents,
              parameterEvents,
            ) as number,
        ),
      rf_take_error: (destination: number, capacity: number) => this.#takeError(destination, capacity),
    };
  }

  #compile(pointer: number, length: number): number {
    return this.#guard(-1, () => {
      // Copied, because compilation may outlive the caller's buffer.
      const bytes = this.#hostBytes(pointer, length).slice();
      const module = new WebAssembly.Module(bytes);
      if (WebAssembly.Module.imports(module).length > 0) {
        throw new Error("wasm-v1 modules may not import host functions");
      }
      const handle = this.#nextHandle++;
      this.#modules.set(handle, module);
      return handle;
    });
  }

  #instantiate(module: number): number {
    return this.#guard(-1, () => {
      const compiled = this.#modules.get(module);
      if (!compiled) {
        throw new Error(`no compiled plugin ${module}`);
      }
      const instance = new WebAssembly.Instance(compiled, {});
      const exports = instance.exports as Record<string, unknown>;
      const memory = exports.memory;
      if (!(memory instanceof WebAssembly.Memory)) {
        throw new Error("wasm-v1 plugin does not export memory");
      }
      const handle = this.#nextHandle++;
      this.#instances.set(handle, { exports, memory });
      return handle;
    });
  }

  #instance(handle: number): PluginInstance {
    const instance = this.#instances.get(handle);
    if (!instance) {
      throw new Error(`no plugin instance ${handle}`);
    }
    return instance;
  }

  #lookup(handle: number, index: number): ((...args: unknown[]) => unknown) | null {
    const name = EXPORTS[index];
    if (!name) {
      throw new Error(`unknown plugin export index ${index}`);
    }
    const candidate = this.#instance(handle).exports[name];
    return typeof candidate === "function"
      ? (candidate as (...args: unknown[]) => unknown)
      : null;
  }

  #call(handle: number, index: number, ...args: unknown[]): unknown {
    const call = this.#lookup(handle, index);
    if (!call) {
      throw new Error(`wasm-v1 plugin is missing export ${EXPORTS[index]}`);
    }
    return call(...args);
  }

  #hostBytes(pointer: number, length: number): Uint8Array {
    if (!this.#hostMemory) {
      throw new Error("the RackForge host memory is not attached yet");
    }
    return new Uint8Array(this.#hostMemory.buffer, pointer, length);
  }

  /**
   * Runs one boundary call, turning a guest trap or a broken handle into a
   * message the Rust side collects with `rf_take_error`, so a misbehaving
   * plugin surfaces as an error rather than as an aborted host.
   */
  #guard<T>(failure: T, body: () => T): T {
    try {
      return body();
    } catch (error) {
      this.#error = error instanceof Error ? error.message : String(error);
      return failure;
    }
  }

  #takeError(destination: number, capacity: number): number {
    if (this.#error === null) {
      return 0;
    }
    const encoded = new TextEncoder().encode(this.#error);
    this.#error = null;
    const length = Math.min(encoded.length, capacity);
    try {
      this.#hostBytes(destination, length).set(encoded.subarray(0, length));
    } catch {
      return 0;
    }
    return length;
  }
}
