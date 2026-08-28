/**
 * One audio render worker: the browser's half of parallel_render_v1.
 *
 * Holds an isolated wasm instance per owned unit — the same isolation the
 * native host gets from separate wasmtime instances — and renders those units
 * whenever the worklet publishes a block. The worker sleeps in Atomics.wait
 * between blocks, so an idle pool costs nothing.
 *
 * Everything here is bounded and allocation-free once running: instances,
 * scratch views and the shared buffer all exist before the first block.
 */

import {
  HEADER,
  poolLayout,
  readPlanEntry,
  unitOwner,
  type PoolLayout,
  type WorkerMessage,
} from "./renderPool";

interface UnitInstance {
  exports: Record<string, unknown>;
  memory: WebAssembly.Memory;
  inputPtr: number;
  outputPtr: number;
  sharedPtr: number;
  dispatchPtr: number;
  renderUnit: (
    unit: number,
    payloadBytes: number,
    sharedBytes: number,
    frames: number,
    outputChannels: number,
  ) => number;
}

let control: Int32Array | null = null;
let bytes: Uint8Array | null = null;
let view: DataView | null = null;
let layout: PoolLayout | null = null;
let units: (UnitInstance | null)[] = [];
let dispatchStride = 0;
let mixSlotBytes = 0;
let outputChannels = 0;
let inputChannels = 0;
let workerIndex = 0;
let workerCount = 1;
let epoch = 0;
let closed = false;
const failedUnits = new Set<number>();

function callable(exports: Record<string, unknown>, name: string): (...a: number[]) => number {
  const candidate = exports[name];
  if (typeof candidate !== "function") {
    throw new Error(`plugin is missing export ${name}`);
  }
  return candidate as (...a: number[]) => number;
}

function buildUnit(
  module: WebAssembly.Module,
  prepare: { sampleRate: number; maximumFrames: number; inputChannels: number; outputChannels: number },
): UnitInstance {
  const instance = new WebAssembly.Instance(module, {});
  const exports = instance.exports as Record<string, unknown>;
  const memory = exports.memory;
  if (!(memory instanceof WebAssembly.Memory)) {
    throw new Error("wasm-v1 plugin does not export memory");
  }
  const initialize = callable(exports, "rackforge_initialize");
  if (initialize() < 0) {
    throw new Error("unit instance failed to initialize");
  }
  const prepareCall = callable(exports, "rackforge_prepare");
  const status = prepareCall(
    prepare.sampleRate,
    prepare.maximumFrames,
    prepare.inputChannels,
    prepare.outputChannels,
  );
  if (status < 0) {
    throw new Error(`unit instance failed to prepare (${status})`);
  }
  return {
    exports,
    memory,
    inputPtr: callable(exports, "rackforge_input_ptr")(),
    outputPtr: callable(exports, "rackforge_output_ptr")(),
    sharedPtr: callable(exports, "rackforge_parallel_shared_ptr")(),
    dispatchPtr: callable(exports, "rackforge_parallel_dispatch_ptr")(),
    renderUnit: callable(exports, "rackforge_parallel_render_unit") as UnitInstance["renderUnit"],
  };
}

function renderBlock(seq: number): void {
  if (!control || !bytes || !view || !layout) return;
  const frames = Atomics.load(control, HEADER.FRAMES);
  const active = Atomics.load(control, HEADER.ACTIVE);
  const sharedLen = Atomics.load(control, HEADER.SHARED_LEN);

  for (let slot = 0; slot < active; slot++) {
    const entry = readPlanEntry(view, layout, slot);
    if (unitOwner(entry.unit, workerCount) !== workerIndex) continue;
    const unit = units[entry.unit];
    if (!unit) continue;

    // A rebuilt or superseded block: contribute nothing rather than racing.
    if (Atomics.load(control, HEADER.SEQ) !== seq) return;

    const memory = new Uint8Array(unit.memory.buffer);
    if (sharedLen > 0) {
      memory.set(
        bytes.subarray(layout.sharedOffset, layout.sharedOffset + sharedLen),
        unit.sharedPtr,
      );
    }
    if (entry.payloadBytes > 0) {
      const payloadBase = layout.dispatchOffset + entry.unit * dispatchStride;
      memory.set(
        bytes.subarray(payloadBase, payloadBase + entry.payloadBytes),
        unit.dispatchPtr + entry.unit * dispatchStride,
      );
    }
    // Instruments run with no input channels; effects would stage input here.
    let status = -1;
    try {
      status = unit.renderUnit(
        entry.unit,
        entry.payloadBytes,
        sharedLen,
        frames,
        outputChannels,
      );
    } catch (error) {
      console.error(`rf-worker ${workerIndex}: render_unit(${entry.unit}) trapped:`, error);
    }
    if (status < 0 && !failedUnits.has(entry.unit)) {
      failedUnits.add(entry.unit);
      console.warn(`rf-worker ${workerIndex}: render_unit(${entry.unit}) status ${status}`);
    }
    if (status >= 0) {
      const samples = frames * outputChannels;
      const mix = new Uint8Array(
        unit.memory.buffer,
        unit.outputPtr,
        samples * 4,
      );
      if (Atomics.load(control, HEADER.SEQ) !== seq) return;
      bytes.set(mix, layout.mixOffset + entry.unit * mixSlotBytes);
    }
    // A failed unit still counts as done: the worklet zero-fills its slot and
    // the block completes rather than waiting out the whole budget.
    if (Atomics.load(control, HEADER.SEQ) !== seq) return;
    Atomics.or(control, HEADER.UNIT_MASK, 1 << entry.unit);
    Atomics.add(control, HEADER.DONE, 1);
    Atomics.notify(control, HEADER.DONE);
  }
}

self.onmessage = (event: MessageEvent<WorkerMessage>) => {
  const message = event.data;
  if (message.kind === "close") {
    closed = true;
    self.close();
    return;
  }
  if (message.kind !== "init") return;

  try {
    const geometry = message.geometry;
    layout = poolLayout(geometry);
    mixSlotBytes = geometry.mixSlotSamples * 4;
    control = new Int32Array(message.buffer, 0, HEADER.WORDS);
    bytes = new Uint8Array(message.buffer);
    view = new DataView(message.buffer);
    dispatchStride = geometry.dispatchStride;
    inputChannels = message.prepare.inputChannels;
    outputChannels = message.prepare.outputChannels;
    workerIndex = message.workerIndex;
    workerCount = message.workerCount;
    epoch = message.epoch;
    void inputChannels;

    const module = new WebAssembly.Module(new Uint8Array(message.component));
    units = new Array(geometry.maxUnits).fill(null);
    for (let unit = 0; unit < geometry.maxUnits; unit++) {
      if (unitOwner(unit, workerCount) === workerIndex) {
        units[unit] = buildUnit(module, message.prepare);
      }
    }

    Atomics.or(control, HEADER.READY_MASK, 1 << workerIndex);
    Atomics.notify(control, HEADER.READY_MASK);

    // The block loop. Sleeping on SEQ costs nothing while the instrument is
    // sequential or idle; a notify wakes every worker for the new block.
    let lastSeq = Atomics.load(control, HEADER.SEQ);
    const run = () => {
      for (;;) {
        if (closed) return;
        if (Atomics.load(control, HEADER.EPOCH) !== epoch) return;
        const wait = Atomics.wait(control, HEADER.SEQ, lastSeq, 250);
        const seq = Atomics.load(control, HEADER.SEQ);
        if (seq !== lastSeq) {
          lastSeq = seq;
          renderBlock(seq);
        } else if (wait === "timed-out") {
          continue;
        }
      }
    };
    run();
  } catch (error) {
    // A worker that cannot build its instances never sets its ready bit, so
    // the pool simply does not come up and the host stays sequential.
    console.error("rackforge render worker failed:", error);
  }
};
