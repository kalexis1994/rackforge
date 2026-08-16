/**
 * Checks the browser host outside a browser.
 *
 * The host is a WASI component driven through a small set of imports, so a
 * WASI runtime can boot it, ask it for a session and make it render — no page
 * required. That covers the part most likely to break silently: the host
 * reading its storage, loading a portable plugin and turning MIDI into audio.
 *
 * Usage: node tools/check-browser-host.mjs <host.wasm> <storage-directory>
 */

import { WASI } from "node:wasi";
import { readFile } from "node:fs/promises";

/**
 * Plugin exports addressed by index, in the order shared with the browser
 * backend of `rackforge-plugin-runtime`.
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
];

const [, , hostPath, storagePath] = process.argv;
if (!hostPath || !storagePath) {
  console.error("usage: node tools/check-browser-host.mjs <host.wasm> <storage-directory>");
  process.exit(2);
}

let hostMemory = null;
let pendingError = null;
let nextHandle = 1;
const modules = new Map();
const instances = new Map();

const guard = (failure, body) => {
  try {
    return body();
  } catch (error) {
    pendingError = error.message ?? String(error);
    return failure;
  }
};
const hostBytes = (pointer, length) => new Uint8Array(hostMemory.buffer, pointer, length);
const instanceOf = (handle) => {
  const instance = instances.get(handle);
  if (!instance) throw new Error(`no plugin instance ${handle}`);
  return instance;
};
const call = (handle, index, ...args) => {
  const call = instanceOf(handle).exports[EXPORTS[index]];
  if (typeof call !== "function") throw new Error(`missing export ${EXPORTS[index]}`);
  return call(...args);
};

const pluginHost = {
  rf_compile: (pointer, length) =>
    guard(-1, () => {
      const module = new WebAssembly.Module(hostBytes(pointer, length).slice());
      if (WebAssembly.Module.imports(module).length > 0) {
        throw new Error("wasm-v1 modules may not import host functions");
      }
      const handle = nextHandle++;
      modules.set(handle, module);
      return handle;
    }),
  rf_module_release: (module) => modules.delete(module),
  rf_instantiate: (module) =>
    guard(-1, () => {
      const instance = new WebAssembly.Instance(modules.get(module), {});
      const handle = nextHandle++;
      instances.set(handle, { exports: instance.exports, memory: instance.exports.memory });
      return handle;
    }),
  rf_instance_release: (instance) => instances.delete(instance),
  rf_memory_size: (instance) => guard(-1, () => instanceOf(instance).memory.buffer.byteLength),
  rf_memory_read: (instance, offset, destination, length) =>
    guard(-1, () => {
      hostBytes(destination, length).set(
        new Uint8Array(instanceOf(instance).memory.buffer, offset, length),
      );
      return length;
    }),
  rf_memory_write: (instance, offset, source, length) =>
    guard(-1, () => {
      new Uint8Array(instanceOf(instance).memory.buffer, offset, length).set(
        hostBytes(source, length),
      );
      return length;
    }),
  rf_export_present: (instance, index) =>
    guard(0, () => (typeof instanceOf(instance).exports[EXPORTS[index]] === "function" ? 1 : 0)),
  rf_call_0: (instance, index) => guard(-1, () => call(instance, index)),
  rf_call_1: (instance, index, argument) => guard(-1, () => call(instance, index, argument)),
  rf_call_f64: (instance, index, argument) => guard(NaN, () => call(instance, index, argument)),
  rf_call_set_parameter: (instance, index, value) => guard(-1, () => call(instance, 14, index, value)),
  rf_call_prepare: (instance, rate, frames, inputs, outputs) =>
    guard(-1, () => call(instance, 13, rate, frames, inputs, outputs)),
  rf_call_resource_begin: (instance, idLength, total) =>
    guard(-1, () => call(instance, 17, idLength, total)),
  rf_call_resource_write: (instance, offset, length) =>
    guard(-1, () => call(instance, 18, offset, length)),
  rf_call_process: (instance, frames, inputs, outputs, midi, parameters) =>
    guard(-1, () => call(instance, 24, frames, inputs, outputs, midi, parameters)),
  rf_take_error: (destination, capacity) => {
    if (pendingError === null) return 0;
    const encoded = new TextEncoder().encode(pendingError);
    pendingError = null;
    const length = Math.min(encoded.length, capacity);
    hostBytes(destination, length).set(encoded.subarray(0, length));
    return length;
  },
};

const wasi = new WASI({
  version: "preview1",
  args: ["rackforge"],
  preopens: { "/rackforge": storagePath },
  returnOnExit: true,
});
const module = await WebAssembly.compile(await readFile(hostPath));
const instance = new WebAssembly.Instance(module, {
  ...wasi.getImportObject(),
  rackforge_plugin_host: pluginHost,
});
wasi.initialize(instance);
hostMemory = instance.exports.memory;

const host = instance.exports;
const readResponse = (length) =>
  new TextDecoder().decode(new Uint8Array(hostMemory.buffer, host.rf_response_ptr(), length));
const request = (body) => {
  const encoded = new TextEncoder().encode(JSON.stringify(body));
  const pointer = host.rf_alloc(encoded.length);
  new Uint8Array(hostMemory.buffer, pointer, encoded.length).set(encoded);
  const response = readResponse(host.rf_request(pointer, encoded.length));
  host.rf_free(pointer, encoded.length);
  return JSON.parse(response);
};
const peak = (frames) => {
  const block = new Float32Array(hostMemory.buffer, host.rf_render(frames), frames * 2);
  return block.reduce((loudest, sample) => Math.max(loudest, Math.abs(sample)), 0);
};

const failures = [];
const check = (description, condition, detail) => {
  if (condition) {
    console.log(`ok   ${description}`);
  } else {
    console.log(`FAIL ${description}${detail ? `: ${detail}` : ""}`);
    failures.push(description);
  }
};

const opened = JSON.parse(readResponse(host.rf_open(48_000, 128, 2)));
check("the host boots from its mounted storage", opened.ok === true, opened.error);
if (!opened.ok) {
  process.exit(1);
}

const snapshot = request({ op: "snapshot" });
check("it publishes a session", snapshot.status === "snapshot", snapshot.message);
check(
  "the session holds a playable instrument",
  snapshot.snapshot?.instances?.length > 0 && Boolean(snapshot.snapshot.active_instance_id),
);

const performance = request({ op: "performance_snapshot" });
check(
  "it publishes a performance library",
  performance.status === "performance_snapshot",
  performance.message,
);

check("nothing sounds before a note", peak(128) === 0);
const accepted = request({
  op: "virtual_midi",
  client_id: "check.browser-host",
  message: { status: 0x90, data1: 60, data2: 100 },
});
check("it accepts a note", accepted.status === "virtual_midi_accepted", accepted.message);
let loudest = 0;
for (let block = 0; block < 20; block += 1) {
  loudest = Math.max(loudest, peak(128));
}
check("the note sounds", loudest > 0.001, `peak ${loudest}`);

const released = request({ op: "release_virtual_midi", client_id: "check.browser-host" });
check("it releases held notes", released.status === "virtual_midi_released", released.message);

process.exit(failures.length === 0 ? 0 : 1);
