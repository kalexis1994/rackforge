import { afterEach, describe, expect, it, vi } from "vitest";
import { HEADER, poolLayout, writePlanEntry, type WorkerInit } from "./renderPool";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("render worker mix transfer", () => {
  async function render(unitChannels: number | undefined, status = 0, restore = false, earlyCommit = false) {
    vi.resetModules();
    if (status < 0) vi.spyOn(console, "warn").mockImplementation(() => {});
    const frames = 2;
    const channels = unitChannels || 2;
    const geometry = { maxUnits: 1, dispatchStride: 8, sharedCapacity: 8, mixSlotSamples: frames * channels, reportStride: 4 };
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
    const loadState = vi.fn(() => 0);
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
          rackforge_parallel_report_ptr: () => 2048,
          rackforge_capacity_transfer_bytes: () => 32,
          rackforge_transfer_ptr: () => 4096,
          rackforge_load_state: loadState,
          ...(unitChannels === undefined ? {} : { rackforge_parallel_unit_channels: () => unitChannels }),
          rackforge_parallel_render_unit: () => {
            output.set(expected);
            new Uint8Array(memory.buffer, 2048, 4).set([9, 8, 7, 6]);
            return status;
          },
        };
      },
    });
    let waits = 0;
    if (earlyCommit) {
      const or = Atomics.or;
      vi.spyOn(Atomics, "or").mockImplementation((array, index, value) => {
        const result = or(array, index, value);
        if (index === HEADER.READY_MASK) control[HEADER.SEQ] = 1;
        return result;
      });
    }
    vi.spyOn(Atomics, "wait").mockImplementation(() => {
      if (waits++ === 0) control[HEADER.SEQ] = 1;
      else control[HEADER.EPOCH] = 2;
      return "ok";
    });
    await import("./render.worker");
    worker.onmessage?.({ data: {
      kind: "init", instance: 1, buffer, geometry,
      prepare: { sampleRate: 48_000, maximumFrames: frames, inputChannels: 0, outputChannels: 2 },
      component: new ArrayBuffer(0), workerIndex: 0, workerCount: 1, epoch: 1,
      state: restore ? new Uint8Array([42, 17]).buffer : undefined,
    } });
    return { actual: Array.from(mix), expected, done: control[HEADER.DONE], valid: control[HEADER.UNIT_MASK], ack: control[HEADER.ACK_MASK],
      report: [...new Uint8Array(buffer, layout.reportOffset, 4)], loadState,
      state: [...new Uint8Array(memory.buffer, 4096, 2)] };
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

  it("restores saved controls and returns reports before acknowledging the block", async () => {
    const result = await render(20, 0, true);
    expect(result.loadState).toHaveBeenCalledWith(2);
    expect(result.state).toEqual([42, 17]);
    expect(result.report).toEqual([9, 8, 7, 6]);
    expect(result.ack).toBe(1);
  });

  it("renders the first commit even when it arrives immediately after READY", async () => {
    const result = await render(20, 0, false, true);
    expect(result.actual).toEqual(result.expected);
    expect(result.done).toBe(1);
  });
});
