/** Per-instance worker arenas. A preparation chooses its render path once;
 * a late worker never replaces a sequential instance that already has voices. */
import { HEADER, poolLayout, writePlanEntry, type PoolGeometry, type PoolLayout, type PoolPrepare, type PoolRequest } from "./renderPool";

interface Pool {
  instance: number;
  module: number;
  epoch: number;
  geometry: PoolGeometry;
  prepare: PoolPrepare;
  layout: PoolLayout;
  mode: "pending" | "parallel" | "sequential";
  sent: boolean;
  deadline: number;
  control: Int32Array | null;
  bytes: Uint8Array | null;
  view: DataView | null;
  workers: number;
  inFlight: boolean;
}

export class ParallelBridge {
  #pools = new Map<number, Pool>();
  #epoch = 0;
  #memory: WebAssembly.Memory | null = null;
  #request: ((request: PoolRequest) => void) | null = null;
  #component: (module: number) => Uint8Array | null = () => null;
  #state: (instance: number) => Uint8Array = () => new Uint8Array();
  #retire: (instance: number, epoch: number) => void = () => {};

  attach(memory: WebAssembly.Memory) { this.#memory = memory; }

  configure(request: (request: PoolRequest) => void, component: (module: number) => Uint8Array | null,
    state: (instance: number) => Uint8Array, retire: (instance: number, epoch: number) => void) {
    this.#request = request;
    this.#component = component;
    this.#state = state;
    this.#retire = retire;
  }

  release(instance: number) {
    const pool = this.#pools.get(instance);
    if (!pool) return;
    this.#pools.delete(instance);
    this.#retire(instance, pool.epoch);
  }

  /** Called at the boundary of a host command, after presets/parameters restore. */
  flushRequests() {
    for (const pool of this.#pools.values()) {
      if (pool.sent || pool.mode !== "pending") continue;
      pool.sent = true;
      pool.deadline = Date.now() + 10_000;
      try {
        const component = this.#component(pool.module);
        if (!component || !this.#request) throw new Error("worker component is unavailable");
        this.#request({ kind: "pool_request", instance: pool.instance, epoch: pool.epoch,
          geometry: pool.geometry, prepare: pool.prepare, component: component.slice().buffer,
          state: this.#state(pool.instance).slice().buffer });
      } catch {
        this.unavailable(pool.instance, pool.epoch);
      }
    }
  }

  adopt(instance: number, buffer: SharedArrayBuffer, workers: number, epoch: number) {
    const pool = this.#pools.get(instance);
    if (!pool || pool.epoch !== epoch || pool.mode !== "pending") return;
    if (buffer.byteLength !== pool.layout.totalBytes || !Number.isInteger(workers)
      || workers < 1 || workers > pool.geometry.maxUnits
      || Atomics.load(new Int32Array(buffer, 0, HEADER.WORDS), HEADER.EPOCH) !== epoch) {
      this.unavailable(instance, epoch); return;
    }
    pool.control = new Int32Array(buffer, 0, HEADER.WORDS);
    pool.bytes = new Uint8Array(buffer);
    pool.view = new DataView(buffer);
    pool.workers = workers;
  }

  unavailable(instance: number, epoch: number) {
    const pool = this.#pools.get(instance);
    if (!pool || pool.epoch !== epoch || pool.mode !== "pending") return;
    pool.mode = "sequential";
    this.#retire(instance, epoch);
  }

  #settle(pool: Pool): boolean {
    if (pool.mode !== "pending") return true;
    const all = (1 << pool.workers) - 1;
    if (pool.control && pool.workers > 0 && (Atomics.load(pool.control, HEADER.READY_MASK) & all) === all) {
      pool.mode = "parallel"; return true;
    }
    if (pool.sent && Date.now() >= pool.deadline) this.unavailable(pool.instance, pool.epoch);
    return pool.mode !== "pending";
  }

  /** Boot barrier: no MIDI/audio block is consumed while workers initialize. */
  get initialized(): boolean {
    let ready = true;
    for (const pool of this.#pools.values()) if (!this.#settle(pool)) ready = false;
    return ready;
  }

  get imports() {
    return {
      rf_par_request: (instance: number, module: number, maxUnits: number, dispatchStride: number,
        mixSlotSamples: number, sharedCapacity: number, reportStride: number, sampleRate: number,
        maximumFrames: number, inputChannels: number, outputChannels: number): void => {
        this.release(instance);
        const geometry = { maxUnits, dispatchStride, mixSlotSamples, sharedCapacity, reportStride };
        const prepare = { sampleRate, maximumFrames, inputChannels, outputChannels };
        this.#pools.set(instance, { instance, module, epoch: ++this.#epoch, geometry, prepare,
          layout: poolLayout(geometry), mode: "pending", sent: false, deadline: 0,
          control: null, bytes: null, view: null, workers: 0, inFlight: false });
      },
      rf_par_ready: (instance: number): number => {
        const pool = this.#pools.get(instance);
        if (!pool) return 0;
        // A runtime-created instance that starts before its workers stays classic
        // for this preparation. Promoting it later would discard held voices.
        if (!this.#settle(pool)) this.unavailable(instance, pool.epoch);
        if (pool.mode !== "parallel" || !pool.control) return 0;
        const all = (1 << pool.workers) - 1;
        if (pool.inFlight && (Atomics.load(pool.control, HEADER.ACK_MASK) & all) !== all) {
          Atomics.add(pool.control, HEADER.DEFERRED, 1); return -1;
        }
        return pool.workers;
      },
      rf_par_begin: (instance: number, frames: number, active: number, sharedLen: number, sharedPtr: number): number => {
        const pool = this.#pools.get(instance);
        if (!pool?.control || !pool.bytes || !this.#memory || pool.mode !== "parallel") return -1;
        const all = (1 << pool.workers) - 1;
        if (pool.inFlight && (Atomics.load(pool.control, HEADER.ACK_MASK) & all) !== all) return -1;
        if (frames < 1 || frames > pool.prepare.maximumFrames || active < 0 || active > pool.geometry.maxUnits
          || sharedLen < 0 || sharedLen > pool.geometry.sharedCapacity) return -1;
        Atomics.store(pool.control, HEADER.FRAMES, frames);
        Atomics.store(pool.control, HEADER.ACTIVE, active);
        Atomics.store(pool.control, HEADER.SHARED_LEN, sharedLen);
        Atomics.store(pool.control, HEADER.DONE, 0);
        Atomics.store(pool.control, HEADER.UNIT_MASK, 0);
        Atomics.store(pool.control, HEADER.ACK_MASK, 0);
        if (sharedLen > 0) pool.bytes.set(new Uint8Array(this.#memory.buffer, sharedPtr, sharedLen), pool.layout.sharedOffset);
        return 0;
      },
      rf_par_entry: (instance: number, slot: number, unit: number, length: number, pointer: number): number => {
        const pool = this.#pools.get(instance);
        if (!pool?.view || !pool.bytes || !this.#memory) return -1;
        if (slot < 0 || slot >= pool.geometry.maxUnits || unit < 0 || unit >= pool.geometry.maxUnits
          || length < 0 || length > pool.geometry.dispatchStride) return -1;
        writePlanEntry(pool.view, pool.layout, slot, unit, length);
        if (length > 0) pool.bytes.set(new Uint8Array(this.#memory.buffer, pointer, length), pool.layout.dispatchOffset + unit * pool.geometry.dispatchStride);
        return 0;
      },
      rf_par_commit: (instance: number): number => {
        const pool = this.#pools.get(instance);
        if (!pool?.control) return -1;
        pool.inFlight = true;
        Atomics.add(pool.control, HEADER.BLOCKS, 1);
        Atomics.add(pool.control, HEADER.SEQ, 1);
        Atomics.notify(pool.control, HEADER.SEQ);
        return 0;
      },
      rf_par_collect: (instance: number, budgetMicros: number): number => {
        const control = this.#pools.get(instance)?.control;
        if (!control) return 0;
        const active = Atomics.load(control, HEADER.ACTIVE);
        const deadline = Date.now() + Math.max(1, budgetMicros / 1000);
        let done = Atomics.load(control, HEADER.DONE), lap = 0;
        while (done < active) {
          if ((++lap & 1023) === 0 && Date.now() > deadline) { Atomics.add(control, HEADER.MISSES, 1); break; }
          done = Atomics.load(control, HEADER.DONE);
        }
        return done;
      },
      rf_par_mix_read: (instance: number, unit: number, destination: number, samples: number): number =>
        this.#read(instance, unit, destination, samples * 4, false),
      rf_par_report_read: (instance: number, unit: number, destination: number, bytes: number): number =>
        this.#read(instance, unit, destination, bytes, true),
      rf_par_unit_mask: (instance: number): number => {
        const control = this.#pools.get(instance)?.control;
        return control ? Atomics.load(control, HEADER.UNIT_MASK) : 0;
      },
      rf_par_misses: (instance: number): number => {
        const control = this.#pools.get(instance)?.control;
        return control ? Atomics.load(control, HEADER.MISSES) : 0;
      },
    };
  }

  #read(instance: number, unit: number, destination: number, length: number, report: boolean): number {
    const pool = this.#pools.get(instance);
    if (!pool?.bytes || !this.#memory || unit < 0 || unit >= pool.geometry.maxUnits) return -1;
    const stride = report ? (pool.geometry.reportStride ?? 0) : pool.geometry.mixSlotSamples * 4;
    if (length < 0 || length > stride) return -1;
    const offset = (report ? pool.layout.reportOffset : pool.layout.mixOffset) + unit * stride;
    new Uint8Array(this.#memory.buffer, destination, length).set(pool.bytes.subarray(offset, offset + length));
    return 0;
  }
}
