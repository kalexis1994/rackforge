import { describe, expect, it } from "vitest";
import { normalizeAudioInputRoute, offeredInputCount } from "./RackAudioInputLinkEditor";
import { meterLevel } from "./AudioInputMeter";
import type { AudioInputState } from "../hooks/useAudioInputStatus";

const scarlett: AudioInputState = {
  availability: "open",
  device_name: "Scarlett 4i4",
  device_channels: 4,
  captured: [1, 2],
  gain_db: 0,
  cable_routing: true,
};

describe("an audio input cable's route", () => {
  it("is kept only when it says something", () => {
    expect(normalizeAudioInputRoute({ channels: [], gain_db: 0 })).toBeUndefined();
    expect(normalizeAudioInputRoute({ channels: [2], gain_db: 0 })).toEqual({ channels: [2] });
    expect(normalizeAudioInputRoute({ channels: [], gain_db: -6 })).toEqual({ gain_db: -6 });
  });

  it("holds its trim to the host's range, in whole decibels", () => {
    expect(normalizeAudioInputRoute({ gain_db: 40 })).toEqual({ gain_db: 24 });
    expect(normalizeAudioInputRoute({ gain_db: -90 })).toEqual({ gain_db: -60 });
    expect(normalizeAudioInputRoute({ gain_db: 2.6 })).toEqual({ gain_db: 3 });
  });

  it("never carries more than a pair", () => {
    expect(normalizeAudioInputRoute({ channels: [1, 2, 3] })).toEqual({ channels: [1, 2] });
  });
});

describe("the inputs a cable is offered", () => {
  it("are the interface's", () => {
    expect(offeredInputCount(scarlett, {})).toBe(4);
  });

  it("still show an input a Rack from another machine names", () => {
    expect(offeredInputCount(scarlett, { channels: [7] })).toBe(7);
  });

  it("are at least a pair when the host has not said", () => {
    expect(offeredInputCount(null, {})).toBe(2);
  });
});

describe("the input meter", () => {
  it("places full scale at the top and silence at the floor", () => {
    expect(meterLevel(1)).toBe(1);
    expect(meterLevel(0)).toBe(0);
    expect(meterLevel(0.001)).toBeCloseTo(0);
    expect(meterLevel(Number.NaN)).toBe(0);
    expect(meterLevel(10 ** (-30 / 20))).toBeCloseTo(0.5);
  });
});
