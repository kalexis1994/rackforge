/**
 * The browser render pool — parallel_render_v1 units on Web Workers.
 *
 * A native RackForge schedules a parallel plugin's units across a thread pool.
 * The browser host runs entirely on the page's audio thread, where none of
 * that machinery exists: the worklet cannot spawn workers, cannot block, and
 * shares memory with nothing unless the page is cross-origin isolated.
 *
 * So the native design maps onto the web like this:
 *
 *   native                          browser
 *   ------                          -------
 *   coordinator wasmtime instance   coordinator instance in the worklet
 *   worker instance per unit        unit instances in Web Workers
 *   shared process memory           one SharedArrayBuffer
 *   pool threads park on a futex    workers sleep in Atomics.wait
 *   audio thread parks briefly      worklet spins with a bounded budget
 *                                   (Atomics.wait is forbidden there)
 *
 * Unit affinity replaces work stealing. In a native process any thread can
 * render any unit because unit state lives in shared memory; here a unit's
 * persistent state lives inside one worker's wasm instance, so unit N is
 * always rendered by worker N % workers. Determinism is unaffected — the
 * combine happens in the coordinator's end_block in ascending unit order, no
 * matter who finished first — and a missed deadline costs that unit's
 * contribution for one block, never state coherence.
 *
 * The page owns the pool (spawning, teardown, the buffer); the worklet owns
 * every decision about what to render and when, exactly as the Rust side of
 * `pluginHost.ts` owns its calls.
 */

/** Worker-count policy, the browser's `automatic_audio_worker_capacity`.
 *
 * The audio thread and the main thread are both real consumers of cores, so
 * two are reserved, and four workers is where fan-out stops paying for block
 * sizes this small — the same ceiling the native pool lands on.
 */
export function automaticWorkerCount(hardwareConcurrency: number): number {
  return Math.max(1, Math.min(4, Math.floor(hardwareConcurrency) - 2));
}

/** Which worker owns a unit. Stable for the life of a pool epoch. */
export function unitOwner(unit: number, workers: number): number {
  return unit % workers;
}

/**
 * Geometry a pool is built for. All four numbers come from the plugin's own
 * parallel exports, already validated by the Rust side against the shared
 * contract limits before they reach JavaScript.
 */
export interface PoolGeometry {
  maxUnits: number;
  dispatchStride: number;
  mixSlotSamples: number;
  sharedCapacity: number;
}

/** What a worker needs to build its unit instances. */
export interface PoolPrepare {
  sampleRate: number;
  maximumFrames: number;
  inputChannels: number;
  outputChannels: number;
}

/**
 * The SharedArrayBuffer layout.
 *
 * One buffer carries everything a block needs to cross the boundary:
 *
 *   header      control words, all Int32, engine-endian
 *   plan        (unit, payloadBytes) pairs for the active units
 *   shared      the coordinator's block-shared payload
 *   dispatch    per-unit dispatch payloads at a fixed stride
 *   mix         per-unit rendered audio, f32
 *
 * The worklet writes plan/shared/dispatch, bumps SEQ and notifies; workers
 * wake, render the units they own, write mix slots, and add to DONE. A worker
 * re-checks SEQ before contributing so a late block is discarded rather than
 * corrupting the next one.
 */
export const HEADER = {
  /** Block sequence number. Bumped by the worklet to start a block. */
  SEQ: 0,
  /** Frames in the current block. */
  FRAMES: 1,
  /** Active unit count for the current block. */
  ACTIVE: 2,
  /** Bytes of shared payload the coordinator produced. */
  SHARED_LEN: 3,
  /** Units finished for the current SEQ. Workers Atomics.add here. */
  DONE: 4,
  /** Pool epoch. Bumped on rebuild so stale workers retire quietly. */
  EPOCH: 5,
  /** Worker liveness bitmask: worker k sets bit k once its instances exist. */
  READY_MASK: 6,
  /** Bitmask of units finished for the current SEQ: 16 units, one word.
   * The counter says how many; this says which, so a missed deadline
   * zero-fills exactly the slots that never arrived. */
  UNIT_MASK: 7,
  /** Deadline misses observed by the worklet, for telemetry. */
  MISSES: 8,
  WORDS: 9,
} as const;

export interface PoolLayout {
  headerBytes: number;
  planOffset: number;
  sharedOffset: number;
  dispatchOffset: number;
  mixOffset: number;
  totalBytes: number;
}

const PLAN_ENTRY_BYTES = 8;

function alignUp(value: number, alignment: number): number {
  return Math.ceil(value / alignment) * alignment;
}

/** Computes the buffer layout for a geometry. Pure, so the page, the workers
 * and the tests all derive identical offsets from the same numbers. */
export function poolLayout(geometry: PoolGeometry): PoolLayout {
  const headerBytes = HEADER.WORDS * 4;
  const planOffset = headerBytes;
  const sharedOffset = alignUp(planOffset + geometry.maxUnits * PLAN_ENTRY_BYTES, 8);
  const dispatchOffset = alignUp(sharedOffset + geometry.sharedCapacity, 8);
  const mixOffset = alignUp(
    dispatchOffset + geometry.maxUnits * geometry.dispatchStride,
    4,
  );
  const totalBytes = mixOffset + geometry.maxUnits * geometry.mixSlotSamples * 4;
  return { headerBytes, planOffset, sharedOffset, dispatchOffset, mixOffset, totalBytes };
}

/** One plan entry, as the worklet publishes it and a worker reads it. */
export function readPlanEntry(
  view: DataView,
  layout: PoolLayout,
  slot: number,
): { unit: number; payloadBytes: number } {
  const base = layout.planOffset + slot * PLAN_ENTRY_BYTES;
  return {
    unit: view.getUint32(base, true),
    payloadBytes: view.getUint32(base + 4, true),
  };
}

export function writePlanEntry(
  view: DataView,
  layout: PoolLayout,
  slot: number,
  unit: number,
  payloadBytes: number,
): void {
  const base = layout.planOffset + slot * PLAN_ENTRY_BYTES;
  view.setUint32(base, unit, true);
  view.setUint32(base + 4, payloadBytes, true);
}

/** The init message a worker receives once, before any block. */
export interface WorkerInit {
  kind: "init";
  buffer: SharedArrayBuffer;
  geometry: PoolGeometry;
  prepare: PoolPrepare;
  /** The wasm-v1 component, as bytes: workers compile their own copies. */
  component: ArrayBuffer;
  workerIndex: number;
  workerCount: number;
  epoch: number;
}

export interface WorkerClose {
  kind: "close";
}

export type WorkerMessage = WorkerInit | WorkerClose;

/** What the worklet sends the page when a parallel plugin wants a pool. */
export interface PoolRequest {
  kind: "pool_request";
  geometry: PoolGeometry;
  prepare: PoolPrepare;
  component: ArrayBuffer;
  /** Identifies the request so a stale response is ignored. */
  epoch: number;
}

/** What the page sends the worklet once the buffer and workers exist. */
export interface PoolAttach {
  kind: "pool_attach";
  buffer: SharedArrayBuffer;
  workerCount: number;
  epoch: number;
}

/** Whether this page can host a pool at all. */
export function poolSupported(): boolean {
  return typeof SharedArrayBuffer !== "undefined"
    && typeof globalThis.crossOriginIsolated === "boolean"
    && globalThis.crossOriginIsolated === true;
}
