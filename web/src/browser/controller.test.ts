import { describe, expect, it } from "vitest";
import {
  isKeyLabMainEndpoint,
  parseControllerColor,
  resolveKeyLabTransport,
  sendControllerRestorePlan,
} from "./client";

describe("browser Arturia controller transport", () => {
  it("claims only the main KeyLab MIDI endpoint", () => {
    expect(isKeyLabMainEndpoint("KL Essential 61 mk3 MIDI")).toBe(true);
    expect(isKeyLabMainEndpoint("Arturia KeyLab Essential 61 mk3 MIDI")).toBe(true);
    expect(isKeyLabMainEndpoint("KeyLab Essential 61 mk3", "Arturia")).toBe(true);
    expect(isKeyLabMainEndpoint("Arturia KeyLab Essential 61 mk3")).toBe(true);
    expect(isKeyLabMainEndpoint("KL Essential 61 mk3 MCU/HUI")).toBe(false);
    expect(isKeyLabMainEndpoint("KL Essential 61 mk3 DINTHRU")).toBe(false);
    expect(isKeyLabMainEndpoint("Other MIDI")).toBe(false);
  });

  it("keeps controller input active when Android grants MIDI without SysEx", () => {
    const input = {
      id: "native:port-in-0",
      name: "KeyLab Essential 61 mk3",
      manufacturer: "Arturia",
      state: "connected",
    };
    const output = {
      id: "native:port-out-0",
      name: "KeyLab Essential 61 mk3",
      manufacturer: "Arturia",
      state: "connected",
    };
    expect(resolveKeyLabTransport([input], [output], false)).toEqual({
      input,
      output: undefined,
    });
    expect(resolveKeyLabTransport([input], [output], true)).toEqual({
      input,
      output,
    });
  });

  it("validates saved colors before handing them to the controller", () => {
    expect(parseControllerColor("#145080")).toEqual([20, 80, 128]);
    expect(parseControllerColor("not-a-color")).toEqual([20, 80, 128]);
  });

  it("flushes queued Web MIDI before sending the cached release plan", () => {
    const calls: string[] = [];
    const sent: number[][] = [];
    const output = {
      clear: () => calls.push("clear"),
      send: (bytes: Uint8Array) => {
        calls.push("send");
        sent.push([...bytes]);
      },
    };

    expect(
      sendControllerRestorePlan(output, [
        { bytes: [0xf0, 1, 0xf7] },
        { bytes: [0xf0, 2, 0xf7] },
      ]),
    ).toBe(true);
    expect(calls).toEqual(["clear", "send", "send"]);
    expect(sent).toEqual([
      [0xf0, 1, 0xf7],
      [0xf0, 2, 0xf7],
    ]);
  });
});
