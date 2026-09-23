import { describe, expect, it } from "vitest";
import { POPOVER_MARGIN, fitPopover } from "./rackSlotPopoverGeometry";

const size = { width: 720, height: 620 };

describe("fitPopover", () => {
  it("leaves an editor that fits exactly where its node put it", () => {
    const fitted = fitPopover({ x: 300, y: 100 }, size, { width: 1400, height: 900 });
    expect(fitted).toEqual({ position: { x: 300, y: 100 }, size });
  });

  it("moves an editor up rather than letting the graph cut its bottom off", () => {
    // A node low in the graph: 455 + 620 would end 124 px past an 851 px layer.
    const fitted = fitPopover({ x: 470, y: 455 }, size, { width: 1376, height: 851 });
    expect(fitted.position.y).toBe(851 - 620 - POPOVER_MARGIN);
    expect(fitted.position.y + fitted.size.height).toBeLessThanOrEqual(851 - POPOVER_MARGIN);
    expect(fitted.size).toEqual(size);
  });

  it("shrinks to the room there is when the graph is smaller than the editor", () => {
    const fitted = fitPopover({ x: 40, y: 200 }, size, { width: 600, height: 500 });
    expect(fitted.size).toEqual({ width: 600 - 2 * POPOVER_MARGIN, height: 500 - 2 * POPOVER_MARGIN });
    expect(fitted.position).toEqual({ x: POPOVER_MARGIN, y: POPOVER_MARGIN });
  });

  it("draws where it was asked until the graph has been measured", () => {
    expect(fitPopover({ x: 5, y: 900 }, size, null)).toEqual({ position: { x: 5, y: 900 }, size });
  });
});
