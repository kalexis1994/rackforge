import { describe, expect, it } from "vitest";
import {
  HEADER,
  automaticWorkerCount,
  poolLayout,
  readPlanEntry,
  unitOwner,
  writePlanEntry,
} from "./renderPool";

describe("the render pool layout", () => {
  const geometry = {
    maxUnits: 5,
    dispatchStride: 64,
    mixSlotSamples: 256,
    sharedCapacity: 1024,
  };

  it("packs regions without overlap and in declared order", () => {
    const layout = poolLayout(geometry);
    expect(layout.headerBytes).toBe(HEADER.WORDS * 4);
    expect(layout.planOffset).toBe(layout.headerBytes);
    expect(layout.sharedOffset).toBeGreaterThanOrEqual(layout.planOffset + 5 * 8);
    expect(layout.dispatchOffset).toBeGreaterThanOrEqual(
      layout.sharedOffset + geometry.sharedCapacity,
    );
    expect(layout.mixOffset).toBeGreaterThanOrEqual(
      layout.dispatchOffset + 5 * 64,
    );
    expect(layout.totalBytes).toBe(layout.mixOffset + 5 * 256 * 4);
  });

  it("aligns the shared and dispatch regions to 8", () => {
    const layout = poolLayout({ ...geometry, maxUnits: 3 });
    expect(layout.sharedOffset % 8).toBe(0);
    expect(layout.dispatchOffset % 8).toBe(0);
    expect(layout.mixOffset % 4).toBe(0);
  });

  it("round-trips plan entries", () => {
    const layout = poolLayout(geometry);
    const view = new DataView(new ArrayBuffer(layout.totalBytes));
    writePlanEntry(view, layout, 0, 4, 48);
    writePlanEntry(view, layout, 1, 0, 0);
    expect(readPlanEntry(view, layout, 0)).toEqual({ unit: 4, payloadBytes: 48 });
    expect(readPlanEntry(view, layout, 1)).toEqual({ unit: 0, payloadBytes: 0 });
  });
});

describe("worker policy", () => {
  it("reserves cores for the audio and main threads and caps at four", () => {
    expect(automaticWorkerCount(2, false)).toBe(1);
    expect(automaticWorkerCount(4, false)).toBe(2);
    expect(automaticWorkerCount(6, false)).toBe(4);
    expect(automaticWorkerCount(16, false)).toBe(4);
  });

  /** Measured on a Galaxy S24 Ultra: a fixed load split across the pool took
   *  7.69 ms on one worker, 4.26 on two, 5.25 on three and 4.32 on four. Two
   *  is the whole of the win, because `collect` waits for every unit and a
   *  slow core always holds one share. */
  it("gives a phone two workers however many cores it advertises", () => {
    expect(automaticWorkerCount(8, true)).toBe(2);
    expect(automaticWorkerCount(16, true)).toBe(2);
    expect(automaticWorkerCount(4, true)).toBe(2);
    // Still never more than the cores can carry.
    expect(automaticWorkerCount(3, true)).toBe(1);
    expect(automaticWorkerCount(2, true)).toBe(1);
  });

  it("assigns every unit a stable owner inside the pool", () => {
    for (let workers = 1; workers <= 4; workers++) {
      for (let unit = 0; unit < 16; unit++) {
        const owner = unitOwner(unit, workers);
        expect(owner).toBeGreaterThanOrEqual(0);
        expect(owner).toBeLessThan(workers);
        expect(unitOwner(unit, workers)).toBe(owner);
      }
    }
  });
});
