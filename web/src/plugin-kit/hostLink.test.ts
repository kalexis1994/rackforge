import { describe, expect, it } from "vitest";
import {
  PROTOCOL,
  beginProgramEditRequest,
  cancelProgramRequest,
  programNameRequest,
  readContext,
  readProgramDraft,
  readResponse,
  readyMessage,
  saveProgramRequest,
  selectRequest,
} from "./hostLink";

describe("what the program selector says to and hears from RackForge", () => {
  it("reads the programs, banks and selection of a context", () => {
    const context = readContext({
      protocol: PROTOCOL,
      kind: "context",
      instance: {
        sounds: [
          { id: "a", name: "Bright", bank: "piano", detail: "Stage", editable: false },
          { id: 7, name: "not a program" },
          { id: "b", name: "Dark" },
          { id: "c", name: "Mine", editable: true },
        ],
        banks: [{ id: "piano", name: "Pianos", order: 1 }, { name: "no id" }],
        selected_sound_id: "b",
      },
    });
    expect(context).toEqual({
      programs: [
        { id: "a", name: "Bright", bank: "piano", detail: "Stage" },
        { id: "b", name: "Dark" },
        { id: "c", name: "Mine", editable: true },
      ],
      banks: [{ id: "piano", name: "Pianos", order: 1 }],
      selected: "b",
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

  it("reads the draft a context carries", () => {
    expect(readProgramDraft({ protocol: PROTOCOL, kind: "parameter_changed" })).toBeUndefined();
    expect(readProgramDraft({ protocol: PROTOCOL, kind: "context", program_draft: null })).toBeNull();
    expect(
      readProgramDraft({
        protocol: PROTOCOL,
        kind: "context",
        program_draft: { draft_id: 4, name: "Mine", original_program_id: "custom.2", dirty: false },
      }),
    ).toEqual({ draftId: 4, name: "Mine", originalProgramId: "custom.2" });
    expect(readProgramDraft({ protocol: PROTOCOL, kind: "context", program_draft: { draft_id: "4" } })).toBeNull();
  });

  it("saves with the program editing methods", () => {
    expect(beginProgramEditRequest("s-1", null)).toEqual({
      protocol: PROTOCOL,
      kind: "request",
      request_id: "s-1",
      method: "plugin.begin_program_edit",
      params: { program_id: null },
    });
    expect(beginProgramEditRequest("s-1", "custom.2").params).toEqual({ program_id: "custom.2" });
    expect(programNameRequest("s-2", 4, "Pad")).toMatchObject({
      method: "plugin.set_program_name",
      params: { draft_id: 4, name: "Pad" },
    });
    expect(saveProgramRequest("s-3", 4)).toMatchObject({ method: "plugin.save_program", params: { draft_id: 4 } });
    expect(cancelProgramRequest("s-4", 4)).toMatchObject({ method: "plugin.cancel_program", params: { draft_id: 4 } });
  });
});
