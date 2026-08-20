import { describe, expect, it } from "vitest";
import {
  SESSION_SCHEMA_VERSION,
  serializeSessionCommand,
} from "./sessionCommandProtocol";

function decoded(command: { type: string; [key: string]: unknown }) {
  return JSON.parse(serializeSessionCommand("test.web", 42, command));
}

describe("RackForge session command wire contract", () => {
  it("pins the schema version expected by Core", () => {
    expect(SESSION_SCHEMA_VERSION).toBe(14);
  });

  it("wraps commands in a dispatch envelope", () => {
    expect(decoded({ type: "set_active_mode", mode: "play" })).toEqual({
      op: "dispatch",
      envelope: {
        schema_version: 14,
        client_id: "test.web",
        command_id: 42,
        command: { type: "set_active_mode", mode: "play" },
      },
    });
  });

  it("preserves Desktop plugin instance ids", () => {
    expect(
      decoded({
        type: "select_plugin",
        instance_id: "desktop.org.rackforge.rf-m1",
      }).envelope.command,
    ).toEqual({
      type: "select_plugin",
      instance_id: "desktop.org.rackforge.rf-m1",
    });
  });

  it("preserves program draft ids without string coercion", () => {
    const command = decoded({ type: "cancel_program_edit", draft_id: 19 })
      .envelope.command;
    expect(command.draft_id).toBe(19);
    expect(typeof command.draft_id).toBe("number");
  });

  it("does not lose floating-point parameter values", () => {
    expect(
      decoded({
        type: "set_plugin_parameter",
        instance_id: "android-main",
        parameter_index: 123,
        value: 0.625,
      }).envelope.command.value,
    ).toBe(0.625);
  });
});
