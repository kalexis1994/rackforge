import { describe, expect, it } from "vitest";
import {
  activityLabel,
  checkHint,
  checkProgress,
  checkReport,
  recordCheckEvents,
  startCheck,
  valuesLabel,
} from "./controllerCheck";
import type { ControllerDevice, ControllerInput } from "./controllerMapping";

const inputs: ControllerInput[] = [
  { id: "pot-1", name: "Pot 1", kind: "knob", group: "Pots", midi: { channel: 15, cc: 21 } },
  { id: "enc-1", name: "Encoder 1", kind: "encoder", group: "Pots", midi: { channel: 15, cc: 13 } },
  { id: "pad-1", name: "Pad 1", kind: "pad", group: "Pads", midi: { channel: 8, note: 9 } },
  { id: "fader-1", name: "Fader 1", kind: "fader", group: "Faders", midi: { channel: 0, pitch_bend: true } },
  { id: "play", name: "Play", kind: "button", group: "Transport", midi: { realtime: "start" } },
];

const cc = (channel: number, number: number, value: number) => ({ status: 0xb0 | channel, data1: number, data2: value });

describe("checking a controller against its package", () => {
  it("ticks off a control when its declared message arrives", () => {
    const check = recordCheckEvents(startCheck("lc"), [cc(15, 21, 0), cc(15, 21, 64), cc(15, 21, 127)], inputs);
    expect(check.heard["pot-1"]).toMatchObject({ count: 3, min: 0, max: 127, values: [0, 64, 127] });
    expect(checkProgress(check, inputs)).toEqual({ heard: 1, total: 5 });
    expect(check.strays).toEqual({});
  });

  it("reads a note's velocity and whether it was released", () => {
    const pressed = recordCheckEvents(startCheck("lc"), [{ status: 0x98, data1: 9, data2: 100 }], inputs);
    expect(valuesLabel(pressed.heard["pad-1"], true)).toBe("100 · no release heard");
    const released = recordCheckEvents(pressed, [{ status: 0x98, data1: 9, data2: 0 }], inputs);
    expect(released.heard["pad-1"]).toMatchObject({ count: 2, values: [100], released: true });
    expect(valuesLabel(released.heard["pad-1"], true)).toBe("100 · released");
  });

  it("reads a bend's fourteen bits", () => {
    const check = recordCheckEvents(
      startCheck("lc"),
      [{ status: 0xe0, data1: 0, data2: 0 }, { status: 0xe0, data1: 0x7f, data2: 0x7f }],
      inputs,
    );
    expect(check.heard["fader-1"]).toMatchObject({ min: 0, max: 16383 });
  });

  it("keeps a message no control declares, with the control it resembles", () => {
    const check = recordCheckEvents(
      startCheck("lc"),
      [cc(0, 21, 5), cc(0, 21, 6), { status: 0xb8, data1: 9, data2: 127 }, { status: 0xc0, data1: 3, data2: 0 }],
      inputs,
    );
    expect(check.strays["CC 21 · ch 1"]).toMatchObject({ count: 2, near: "Pot 1: CC 21 · ch 16" });
    // The pad's number, sent as a control change on its channel.
    expect(check.strays["CC 9 · ch 9"]).toMatchObject({ near: "Pad 1: Note 9 · ch 9" });
    expect(check.strays["Program change · ch 1"]).toMatchObject({ values: [3] });
    expect(check.strays["Program change · ch 1"].near).toBeUndefined();
  });

  it("names every kind of message", () => {
    expect(activityLabel({ status: 0xfa, data1: 0, data2: 0 })).toBe("MIDI Start");
    expect(activityLabel({ status: 0x8f, data1: 60, data2: 0 })).toBe("Note 60 · ch 16");
    expect(activityLabel({ status: 0xa2, data1: 36, data2: 50 })).toBe("Poly pressure 36 · ch 3");
    expect(activityLabel({ status: 0xd0, data1: 50, data2: 0 })).toBe("Channel pressure · ch 1");
    expect(activityLabel({ status: 0xe4, data1: 0, data2: 64 })).toBe("Pitch bend · ch 5");
  });

  it("hints that a knob declared absolute sends relative steps", () => {
    const turns = [63, 62, 63, 65, 66, 65].map((value) => cc(15, 13, value));
    const check = recordCheckEvents(startCheck("lc"), turns, inputs);
    expect(checkHint(inputs[1], check.heard["enc-1"])).toBe("Looks relative (binary offset, 64 is still)");
    const sweep = recordCheckEvents(startCheck("lc"), [...Array(128).keys()].map((value) => cc(15, 21, value)), inputs);
    expect(checkHint(inputs[0], sweep.heard["pot-1"])).toBeNull();
    const relative: ControllerInput = { ...inputs[1], encoder: "relative_binary_offset" };
    const swept = recordCheckEvents(startCheck("lc"), [...Array(128).keys()].map((value) => cc(15, 13, value)), inputs);
    expect(checkHint(relative, swept.heard["enc-1"])).toBe("Looks absolute: it sweeps a range");
  });

  it("writes a report a package's sources can be corrected from", () => {
    const device: ControllerDevice = {
      id: "novation-launch-control",
      name: "Launch Control",
      source: { id: "alsa:20:0", name: "Launch Control:Launch Control MIDI 1 20:0" },
      connected: true,
      identified: false,
      inputs,
      roles: [],
      actions: [],
      orphaned: false,
    };
    const check = recordCheckEvents(
      startCheck(device.id, new Date("2026-09-25T10:00:00Z")),
      [cc(15, 21, 0), cc(15, 21, 127), cc(0, 21, 5), { status: 0xfa, data1: 0, data2: 0 }],
      inputs,
    );
    const report = checkReport(device, check, new Date("2026-09-25T10:05:00Z"));
    expect(report).toContain("- Package: none listed for `novation-launch-control`; its controls are the map's");
    expect(report).toContain("- Port RackForge reads: Launch Control:Launch Control MIDI 1 20:0");
    expect(report).toContain("- Recognised by its Identity Reply: no (port name only)");
    expect(report).toContain("- Heard 2 of 5 controls; 1 message no control declares");
    expect(report).toContain("| Pots | Pot 1 | CC 21 · ch 16 | 2 | 0, 127 |  |");
    expect(report).toContain("| Pads | Pad 1 | Note 9 · ch 9 | no |  |  |");
    expect(report).toContain("| CC 21 · ch 1 | 1 | 5 | Pot 1: CC 21 · ch 16 |");
  });
});
