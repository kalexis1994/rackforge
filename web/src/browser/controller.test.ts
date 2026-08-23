import { describe, expect, it } from "vitest";
import { isKeyLabMainEndpoint, parseControllerColor } from "./client";

describe("browser Arturia controller transport", () => {
  it("claims only the main KeyLab MIDI endpoint", () => {
    expect(isKeyLabMainEndpoint("KL Essential 61 mk3 MIDI")).toBe(true);
    expect(isKeyLabMainEndpoint("Arturia KeyLab Essential 61 mk3 MIDI")).toBe(true);
    expect(isKeyLabMainEndpoint("KL Essential 61 mk3 MCU/HUI")).toBe(false);
    expect(isKeyLabMainEndpoint("KL Essential 61 mk3 DINTHRU")).toBe(false);
    expect(isKeyLabMainEndpoint("Other MIDI")).toBe(false);
  });

  it("validates saved colors before handing them to the controller", () => {
    expect(parseControllerColor("#145080")).toEqual([20, 80, 128]);
    expect(parseControllerColor("not-a-color")).toEqual([20, 80, 128]);
  });
});
