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
    expect(automaticWorkerCount(2)).toBe(1);
    expect(automaticWorkerCount(4)).toBe(2);
    expect(automaticWorkerCount(6)).toBe(4);
    expect(automaticWorkerCount(16)).toBe(4);
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
