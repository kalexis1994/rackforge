/**
 * The worklet's side of the browser render pool.
 *
 * Owns the SharedArrayBuffer once the page attaches one, and turns it into the
 * `rf_par_*` imports the Rust host calls. The bridge makes no decisions: what
 * to render, when to dispatch and how long to wait all come from Rust, the
 * same division of labour `pluginHost.ts` keeps for sequential calls.
 *
 * The one liberty it takes is the spin. Atomics.wait is forbidden on the audio
 * thread, so `collect` polls the done counter — with a bounded budget measured
 * in wall-clock time, checked coarsely so the fast path is a plain load. A
 * pool that misses its budget costs the missing units' audio for one block and
 * a telemetry mark, never a wedged audio thread.
 */

import {
  HEADER,
  poolLayout,
  writePlanEntry,
  type PoolGeometry,
  type PoolLayout,
  type PoolPrepare,
  type PoolRequest,
} from "./renderPool";

export class ParallelBridge {
  #control: Int32Array | null = null;
  #bytes: Uint8Array | null = null;
  #view: DataView | null = null;
  #layout: PoolLayout | null = null;
  #geometry: PoolGeometry | null = null;
  #workerCount = 0;
  #requestedEpoch = 0;
  #hostMemory: WebAssembly.Memory | null = null;
  #requestPool: ((request: PoolRequest) => void) | null = null;
  /** Bytes of the component the pool was requested for, held by handle. */
  #componentBytes: (module: number) => Uint8Array | null = () => null;

  attach(memory: WebAssembly.Memory) {
    this.#hostMemory = memory;
  }

  /** Wires the page messenger and the component-byte lookup in. */
  configure(
    requestPool: (request: PoolRequest) => void,
    componentBytes: (module: number) => Uint8Array | null,
  ) {
    this.#requestPool = requestPool;
    this.#componentBytes = componentBytes;
  }

  /** Called when the page reports a pool is up. */
  adopt(buffer: SharedArrayBuffer, workerCount: number, epoch: number) {
    if (epoch !== this.#requestedEpoch || !this.#geometry) {
      return; // a stale build for a plugin we already moved past
    }
    this.#control = new Int32Array(buffer, 0, HEADER.WORDS);
    this.#bytes = new Uint8Array(buffer);
    this.#view = new DataView(buffer);
    this.#layout = poolLayout(this.#geometry);
    this.#workerCount = workerCount;
  }

  /** Drops the pool, e.g. when the plugin or the prepare geometry changes. */
  release() {
    this.#control = null;
    this.#bytes = null;
    this.#view = null;
    this.#layout = null;
    this.#geometry = null;
    this.#workerCount = 0;
  }

  get imports() {
    return {
      /** Workers that are built and standing by; 0 keeps Rust sequential. */
      rf_par_ready: (): number => {
        const control = this.#control;
        if (!control || this.#workerCount === 0) return 0;
        const mask = Atomics.load(control, HEADER.READY_MASK);
        const all = (1 << this.#workerCount) - 1;
        return (mask & all) === all ? this.#workerCount : 0;
      },

      /**
       * Asks the page for a pool. Fire-and-forget and idempotent per epoch:
       * Rust calls this from `prepare`, the pool arrives whenever it arrives,
       * and until then every block takes the sequential path.
       */
      rf_par_request: (
        module: number,
        maxUnits: number,
        dispatchStride: number,
        mixSlotSamples: number,
        sharedCapacity: number,
        sampleRate: number,
        maximumFrames: number,
        inputChannels: number,
        outputChannels: number,
      ): void => {
        const component = this.#componentBytes(module);
        if (!component || !this.#requestPool) return;
        const geometry: PoolGeometry = {
          maxUnits,
          dispatchStride,
          mixSlotSamples,
          sharedCapacity,
        };
        const prepare: PoolPrepare = {
          sampleRate,
          maximumFrames,
          inputChannels,
          outputChannels,
        };
        this.release();
        this.#geometry = geometry;
        this.#requestedEpoch += 1;
        // The copy is deliberate: the message crosses threads and the module
        // bytes belong to the plugin host's lifetime, not the pool's.
        const payload = component.slice().buffer;
        this.#requestPool({
          kind: "pool_request",
          geometry,
          prepare,
          component: payload,
          epoch: this.#requestedEpoch,
        });
      },

      /** Publishes the block header and the shared payload. Not yet visible
       * to workers: SEQ is bumped by `rf_par_commit`. */
      rf_par_begin: (
        frames: number,
        active: number,
        sharedLen: number,
        sharedPtr: number,
      ): number => {
        const control = this.#control;
        const bytes = this.#bytes;
        const layout = this.#layout;
        if (!control || !bytes || !layout || !this.#hostMemory) return -1;
        Atomics.store(control, HEADER.FRAMES, frames);
        Atomics.store(control, HEADER.ACTIVE, active);
        Atomics.store(control, HEADER.SHARED_LEN, sharedLen);
        Atomics.store(control, HEADER.DONE, 0);
        Atomics.store(control, HEADER.UNIT_MASK, 0);
        if (sharedLen > 0) {
          bytes.set(
            new Uint8Array(this.#hostMemory.buffer, sharedPtr, sharedLen),
            layout.sharedOffset,
          );
        }
        return 0;
      },

      /** One plan entry and its dispatch payload. */
      rf_par_entry: (
        slot: number,
        unit: number,
        payloadLen: number,
        payloadPtr: number,
      ): number => {
        const bytes = this.#bytes;
        const view = this.#view;
        const layout = this.#layout;
        const geometry = this.#geometry;
        if (!bytes || !view || !layout || !geometry || !this.#hostMemory) return -1;
        writePlanEntry(view, layout, slot, unit, payloadLen);
        if (payloadLen > 0) {
          bytes.set(
            new Uint8Array(this.#hostMemory.buffer, payloadPtr, payloadLen),
            layout.dispatchOffset + unit * geometry.dispatchStride,
          );
        }
        return 0;
      },

      /** Makes the block visible and wakes every worker. */
      rf_par_commit: (): number => {
        const control = this.#control;
        if (!control) return -1;
        Atomics.add(control, HEADER.SEQ, 1);
        Atomics.notify(control, HEADER.SEQ);
        return 0;
      },

      /**
       * Spins until every active unit reported done, or the budget passes.
       * Returns the done count; Rust zero-fills whatever is missing.
       */
      rf_par_collect: (budgetMicros: number): number => {
        const control = this.#control;
        if (!control) return 0;
        const active = Atomics.load(control, HEADER.ACTIVE);
        const deadline = Date.now() + Math.max(1, budgetMicros / 1000);
        let done = Atomics.load(control, HEADER.DONE);
        let lap = 0;
        while (done < active) {
          if ((++lap & 1023) === 0 && Date.now() > deadline) {
            Atomics.add(control, HEADER.MISSES, 1);
            break;
          }
          done = Atomics.load(control, HEADER.DONE);
        }
        return done;
      },

      /** Copies one unit's rendered audio into host memory. */
      rf_par_mix_read: (
        unit: number,
        destinationPtr: number,
        lengthSamples: number,
      ): number => {
        const bytes = this.#bytes;
        const layout = this.#layout;
        const geometry = this.#geometry;
        if (!bytes || !layout || !geometry || !this.#hostMemory) return -1;
        const lengthBytes = lengthSamples * 4;
        const source = bytes.subarray(
          layout.mixOffset + unit * geometry.mixSlotSamples * 4,
          layout.mixOffset + unit * geometry.mixSlotSamples * 4 + lengthBytes,
        );
        new Uint8Array(this.#hostMemory.buffer, destinationPtr, lengthBytes).set(source);
        return 0;
      },

      /** Which units finished for the current block, as a bitmask. */
      rf_par_unit_mask: (): number => {
        const control = this.#control;
        return control ? Atomics.load(control, HEADER.UNIT_MASK) : 0;
      },

      /** Deadline misses since boot, drained by telemetry reads. */
      rf_par_misses: (): number => {
        const control = this.#control;
        return control ? Atomics.load(control, HEADER.MISSES) : 0;
      },
    };
  }
}
