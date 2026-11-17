import { afterEach, describe, expect, it, vi } from "vitest";
import { HEADER, poolLayout, writePlanEntry, type WorkerInit } from "./renderPool";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("render worker mix transfer", () => {
  async function render(unitChannels: number | undefined, status = 0) {
    vi.resetModules();
    if (status < 0) vi.spyOn(console, "warn").mockImplementation(() => {});
    const frames = 2;
    const channels = unitChannels || 2;
    const geometry = { maxUnits: 1, dispatchStride: 8, sharedCapacity: 8, mixSlotSamples: frames * channels };
    const layout = poolLayout(geometry);
    const buffer = new SharedArrayBuffer(layout.totalBytes);
    const control = new Int32Array(buffer, 0, HEADER.WORDS);
    control[HEADER.EPOCH] = 1;
    control[HEADER.FRAMES] = frames;
    control[HEADER.ACTIVE] = 1;
    writePlanEntry(new DataView(buffer), layout, 0, 0, 0);
    const mix = new Float32Array(buffer, layout.mixOffset, frames * channels);
    mix.fill(-99);
    const memory = new WebAssembly.Memory({ initial: 1 });
    const output = new Float32Array(memory.buffer, 128, frames * channels);
    const expected = Array.from({ length: frames * channels }, (_, index) => index + 1);
    const worker = { onmessage: undefined as undefined | ((event: { data: WorkerInit }) => void), close: vi.fn() };
    vi.stubGlobal("self", worker);
    vi.stubGlobal("WebAssembly", {
      Memory: WebAssembly.Memory,
      Module: class {},
      Instance: class {
        exports = {
          memory,
          rackforge_initialize: () => 0,
          rackforge_prepare: () => 0,
          rackforge_input_ptr: () => 0,
          rackforge_output_ptr: () => 128,
          rackforge_parallel_shared_ptr: () => 512,
          rackforge_parallel_dispatch_ptr: () => 1024,
          ...(unitChannels === undefined ? {} : { rackforge_parallel_unit_channels: () => unitChannels }),
          rackforge_parallel_render_unit: () => { output.set(expected); return status; },
        };
      },
    });
    let waits = 0;
    vi.spyOn(Atomics, "wait").mockImplementation(() => {
      if (waits++ === 0) control[HEADER.SEQ] = 1;
      else control[HEADER.EPOCH] = 2;
      return "ok";
    });
    await import("./render.worker");
    worker.onmessage?.({ data: {
      kind: "init", buffer, geometry,
      prepare: { sampleRate: 48_000, maximumFrames: frames, inputChannels: 0, outputChannels: 2 },
      component: new ArrayBuffer(0), workerIndex: 0, workerCount: 1, epoch: 1,
    } });
    return { actual: Array.from(mix), expected, done: control[HEADER.DONE], valid: control[HEADER.UNIT_MASK] };
  }

  it("transfers all twenty Concert Grand force channels rather than just stereo", async () => {
    const result = await render(20);
    expect(result.actual).toEqual(result.expected);
    expect(result.done).toBe(1);
    expect(result.valid).toBe(1);
  });

  it.each([undefined, 0])("keeps stereo for a legacy or zero-width declaration (%s)", async (channels) => {
    const result = await render(channels);
    expect(result.actual).toEqual(result.expected);
  });

  it("does not publish a previous mix slot when rendering fails", async () => {
    const result = await render(20, -1);
    expect(result.done).toBe(1);
    expect(result.valid).toBe(0);
  });
});
