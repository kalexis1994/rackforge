import { describe, expect, it } from "vitest";
import { type ParameterTouchReport, parameterTouchLine } from "./parameterTouch";
import { type PluginParameterDescriptor } from "./types";

function descriptor(name: string, kind: PluginParameterDescriptor["kind"]): PluginParameterDescriptor {
  return {
    index: 0,
    id: name.toLowerCase(),
    name,
    page: "main",
    order: 0,
    kind,
    flags: { automatable: true, modulatable: false, read_only: false, advanced: false },
    suggested_control: "knob",
  };
}

const room = descriptor("Room Size", {
  type: "float",
  minimum: 45,
  maximum: 45000,
  default: 311.3,
  step: 1,
  unit: "m³",
  taper: "logarithmic",
});

const drawbar = descriptor("4' B", { type: "integer", minimum: 0, maximum: 8, default: 0, step: 1 });

function touch(parameter: PluginParameterDescriptor, rest: Partial<ParameterTouchReport>): ParameterTouchReport {
  return { instance_id: "play.organ", parameter, value: 0, pickup: "engaged", ...rest };
}

describe("parameterTouchLine", () => {
  it("names the parameter and its value in its own units", () => {
    expect(parameterTouchLine(touch(room, { value: 1462.3 }))).toEqual({
      name: "Room Size",
      value: "1462 m³",
    });
  });

  it("shows where a control on its way stands, which way to go and the target", () => {
    expect(parameterTouchLine(touch(drawbar, { value: 6, pickup: "move_up", control: 2 }))).toEqual({
      name: "4' B",
      value: "2 ↑6",
    });
    expect(parameterTouchLine(touch(room, { value: 45, pickup: "move_down", control: 120 })).value).toBe(
      "120 ↓45 m³",
    );
  });

  it("names a choice and a switch", () => {
    const rotary = descriptor("Rotary Mode", {
      type: "enum",
      default: 0,
      choices: [
        { value: 2, name: "Chorale" },
        { value: 3, name: "Tremolo" },
      ],
    });
    expect(parameterTouchLine(touch(rotary, { value: 3 })).value).toBe("Tremolo");
    const percussion = descriptor("Percussion", { type: "boolean", default: false });
    expect(parameterTouchLine(touch(percussion, { value: 1 })).value).toBe("On");
  });
});
