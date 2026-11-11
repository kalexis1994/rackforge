import { describe, expect, it } from "vitest";
import { padGridLayout } from "./touchControllerLayout";

const docked = {
  docked: true,
  regular: true,
  controllerWidth: 1400,
  controllerHeight: 900,
  rows: 2,
  columns: 4,
};

describe("padGridLayout", () => {
  it("sizes docked pads from the dock's height, less its chrome", () => {
    // 400 dock - 48 chrome - 28 padding = 324; two rows with a 9 px gap.
    const layout = padGridLayout({ ...docked, dockHeight: 400 });
    expect(layout.cellHeight).toBeCloseTo((324 - 9) / 2);
    expect(layout.gridHeight).toBeCloseTo(324);
    expect(layout.gridWidth).toBeCloseTo(layout.cellHeight * 4 + 9 * 3);
  });

  it("grows the grid with the dock, which is what a resize drag draws", () => {
    const small = padGridLayout({ ...docked, dockHeight: 260 });
    const large = padGridLayout({ ...docked, dockHeight: 480 });
    expect(large.gridHeight).toBeGreaterThan(small.gridHeight);
    expect(large.gridWidth).toBeGreaterThan(small.gridWidth);
  });

  it("stops at the width when the controller is narrow", () => {
    const layout = padGridLayout({ ...docked, controllerWidth: 500, regular: false, dockHeight: 900 });
    // 500 - 14 padding = 486 across four square pads with 7 px gaps.
    expect(layout.gridWidth).toBeCloseTo(486);
  });

  it("names how much room each pad has", () => {
    expect(padGridLayout({ ...docked, dockHeight: 200 }).densityClass).toBe(" compact");
    expect(padGridLayout({ ...docked, dockHeight: 560 }).densityClass).toBe("");
  });
});
