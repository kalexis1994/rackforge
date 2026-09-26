import { describe, expect, it } from "vitest";
import { PROTOCOL, readContext, readResponse, readyMessage, selectRequest } from "./hostLink";

describe("what the program selector says to and hears from RackForge", () => {
  it("reads the programs, banks and selection of a context", () => {
    const context = readContext({
      protocol: PROTOCOL,
      kind: "context",
      isolated: true,
      instance: {
        sounds: [
          { id: "a", name: "Bright", bank: "piano", detail: "Stage", editable: false },
          { id: 7, name: "not a program" },
          { id: "b", name: "Dark" },
        ],
        banks: [{ id: "piano", name: "Pianos", order: 1 }, { name: "no id" }],
        selected_sound_id: "b",
      },
    });
    expect(context).toEqual({
      programs: [
        { id: "a", name: "Bright", bank: "piano", detail: "Stage" },
        { id: "b", name: "Dark" },
      ],
      banks: [{ id: "piano", name: "Pianos", order: 1 }],
      selected: "b",
      isolated: true,
    });
  });

  it("ignores other messages and other protocols", () => {
    expect(readContext({ protocol: PROTOCOL, kind: "parameter_changed" })).toBeNull();
    expect(readContext({ protocol: "other", kind: "context", instance: {} })).toBeNull();
    expect(readContext(null)).toBeNull();
    expect(readContext({ protocol: PROTOCOL, kind: "context", instance: {} })).toEqual({
      programs: [],
      banks: [],
      selected: null,
      isolated: false,
    });
  });

  it("chooses with plugin.select_sound and reads only its own answer", () => {
    expect(selectRequest("rf-program-select-1", "a")).toEqual({
      protocol: PROTOCOL,
      kind: "request",
      request_id: "rf-program-select-1",
      method: "plugin.select_sound",
      params: { sound_id: "a" },
    });
    const response = { protocol: PROTOCOL, kind: "response", request_id: "rf-program-select-1", ok: false, error: "nope" };
    expect(readResponse(response, "rf-program-select-1")).toEqual({ ok: false, error: "nope" });
    expect(readResponse(response, "rf-program-select-2")).toBeNull();
    expect(readResponse({ ...response, ok: true, error: undefined }, "rf-program-select-1")).toEqual({ ok: true });
    expect(readyMessage()).toEqual({ protocol: PROTOCOL, kind: "ready" });
  });
});
