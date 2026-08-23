/**
 * Checks the browser host outside a browser, against what it claims.
 *
 * The host is a WASI component driven through a small set of imports, so a
 * WASI runtime can boot it, ask it for a session and make it render — no page
 * required.
 *
 * Every capability the host reports as supported is exercised here. A
 * capability cannot be declared supported in
 * `crates/rackforge-host-capabilities` without either a probe below or an
 * entry in `PAGE_SIDE`, so the parity table cannot claim something the host
 * does not do.
 *
 * Usage: node tools/check-browser-host.mjs <host.wasm> <storage-directory> [package.rfplugin]
 */

import { WASI } from "node:wasi";
import { readFile } from "node:fs/promises";
import { existsSync, readFileSync } from "node:fs";

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

const [, , hostPath, storagePath, packagePath] = process.argv;
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

/**
 * Capabilities that belong to the page rather than to the host, and are
 * checked where they live: in the browser tests, not here.
 */
const PAGE_SIDE = new Map([
  ["persistent_storage", "the page files the host's storage in IndexedDB"],
  ["offline_operation", "the service worker serves the site with the network off"],
  ["midi_input", "Web MIDI delivers messages the page forwards as live MIDI"],
  ["midi_output", "the page schedules certified SysEx plans on a Web MIDI output"],
  ["midi_hotplug", "the page re-attaches inputs when Web MIDI reports a change"],
]);

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

const sessionInstances = snapshot.snapshot.instances;
const instanceId = snapshot.snapshot.active_instance_id;

/**
 * One probe per capability the host claims. Each returns a message on failure
 * and nothing on success, so a claim is only as good as what it can do here.
 */
const PROBES = {
  play_instrument: () =>
    sessionInstances.length > 0 && loudest > 0.001 ? null : "no instrument rendered any audio",
  select_program: () => {
    const sound = sessionInstances.find((instance) => instance.instance_id === instanceId)
      ?.sounds?.[0];
    if (!sound) return "the active instrument exposes no program";
    const applied = dispatch({ type: "select_sound", instance_id: instanceId, sound_id: sound.id });
    return applied.status === "command_applied" ? null : applied.message;
  },
  master_level_and_pan: () => {
    const level = dispatch({ type: "set_master_level", level: 750 });
    const pan = dispatch({ type: "set_master_pan", pan: -250 });
    return level.status === "command_applied" && pan.status === "command_applied"
      ? null
      : (level.message ?? pan.message);
  },
  virtual_midi: () => (accepted.status === "virtual_midi_accepted" ? null : accepted.message),
  plugin_parameters: () => {
    const parameters = request({ op: "plugin_parameters", instance_id: instanceId });
    if (parameters.status !== "plugin_parameters") return parameters.message;
    const first = parameters.values?.[0];
    if (!first) return null;
    const written = request({
      op: "set_plugin_parameter",
      instance_id: instanceId,
      parameter_index: first.index,
      value: first.value,
    });
    return written.status === "plugin_parameter_set" ? null : written.message;
  },
  host_presets: () => {
    const saved = request({ op: "save_plugin_preset", instance_id: instanceId, name: "Probe" });
    if (saved.status !== "plugin_preset_saved") return saved.message;
    const loaded = request({
      op: "load_plugin_preset",
      instance_id: instanceId,
      preset_id: saved.preset.id,
    });
    if (loaded.status !== "plugin_preset_loaded") return loaded.message;
    const deleted = request({
      op: "delete_plugin_preset",
      plugin_id: saved.preset.plugin_id,
      preset_id: saved.preset.id,
    });
    return deleted.status === "plugin_preset_deleted" ? null : deleted.message;
  },
  performance_library: () => {
    const library = request({ op: "performance_snapshot" });
    return library.status === "performance_snapshot" ? null : library.message;
  },
  session_restore: () => {
    // The checkpoint is written where the next boot reads it; a second host
    // over the same storage is what a returning visit actually is.
    const path = `${storagePath}/sessions/live.main.json`;
    if (!existsSync(path)) return `no checkpoint at ${path}`;
    const saved = JSON.parse(readFileSync(path, "utf8"));
    const held = saved.active_instance_id ?? saved.session?.active_instance_id;
    return held === instanceId
      ? null
      : `the checkpoint holds ${held ?? "nothing"} rather than the active instrument`;
  },
  plugin_web_surfaces: () => {
    // The page serves the files; the host's part is reporting which files a
    // package declares as its interface.
    const listed = JSON.parse(readResponse(host.rf_plugin_catalog()));
    const declared = listed.catalog?.some((plugin) => plugin.surfaces?.length > 0);
    return declared ? null : "no loaded plugin declares a web surface";
  },
  controller_packages: () => {
    host.rf_controller_connect();
    if (host.rf_controller_output_pending() !== 1) {
      return "the bundled controller produced no acquisition plan";
    }
    const messages = JSON.parse(readResponse(host.rf_controller_output()));
    const valid =
      messages.length > 3 &&
      messages.every(
        (message) =>
          Array.isArray(message.bytes) &&
          message.bytes[0] === 0xf0 &&
          message.bytes.at(-1) === 0xf7 &&
          Number.isInteger(message.settle_after_ms),
      );
    host.rf_controller_disconnect();
    readResponse(host.rf_controller_output());
    return valid ? null : "the bundled controller returned an invalid SysEx plan";
  },
  plugin_install: () => {
    if (!packagePath) return "no .rfplugin was given to install";
    const archive = new Uint8Array(readFileSync(packagePath));
    const inspected = JSON.parse(withArchive(archive, host.rf_inspect_plugin));
    if (!inspected.ok) return inspected.error;
    const installed = JSON.parse(withArchive(archive, host.rf_install_plugin));
    if (!installed.ok) return installed.error;
    installedPluginId = installed.installed.plugin_id;
    const listed = JSON.parse(readResponse(host.rf_plugin_catalog()));
    return listed.catalog?.some((plugin) => plugin.plugin_id === installedPluginId)
      ? null
      : "the installed plugin is not in the catalog";
  },
  plugin_removal: () => {
    if (!installedPluginId) return "nothing was installed to remove";
    const removed = JSON.parse(
      withArchive(
        new TextEncoder().encode(JSON.stringify({ plugin_id: installedPluginId })),
        host.rf_uninstall_plugin,
      ),
    );
    if (!removed.ok) return removed.error;
    const listed = JSON.parse(readResponse(host.rf_plugin_catalog()));
    return listed.catalog?.some((plugin) => plugin.plugin_id === installedPluginId)
      ? "the removed plugin is still in the catalog"
      : null;
  },
};

let installedPluginId = null;

function dispatch(command) {
  return request({
    op: "dispatch",
    envelope: {
      schema_version: snapshot.snapshot.schema_version,
      client_id: "check.browser-host",
      command_id: nextCommandId++,
      command,
    },
  });
}
let nextCommandId = 1;

function withArchive(bytes, call) {
  const pointer = host.rf_alloc(bytes.length);
  new Uint8Array(hostMemory.buffer, pointer, bytes.length).set(bytes);
  try {
    return readResponse(call(pointer, bytes.length));
  } finally {
    host.rf_free(pointer, bytes.length);
  }
}

const declared = JSON.parse(readResponse(host.rf_capabilities()));
check("it reports what it can do", declared.ok === true, declared.error);
for (const capability of declared.capabilities ?? []) {
  if (!capability.supported) {
    check(
      `${capability.id} explains why it is missing`,
      Boolean(capability.reason) || capability.state === "unaudited",
    );
    continue;
  }
  const probe = PROBES[capability.id];
  if (!probe) {
    const reason = PAGE_SIDE.get(capability.id);
    check(
      `${capability.id} is claimed with somewhere to check it`,
      Boolean(reason),
      "no probe here and not listed as page-side",
    );
    continue;
  }
  const failure = probe();
  check(`${capability.id} does what it claims`, failure === null, failure ?? undefined);
}

process.exit(failures.length === 0 ? 0 : 1);
