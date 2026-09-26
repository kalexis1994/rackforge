import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { ControllerOutput } from "../../controllerMapping";
import { ControllerOutputNotice } from "./ControllerOutputNotice";

const sysex: ControllerOutput = {
  state: "asked",
  messages: ["F0 00 20 6B 7F 42 02 00 40 50 01 F7"],
  sysex: true,
};

describe("ControllerOutputNotice", () => {
  it("asks before anything is sent, and shows what would be", () => {
    const markup = renderToStaticMarkup(
      <ControllerOutputNotice output={sysex} shipped={false} canAnswer busy={false} onAnswer={vi.fn()} />,
    );

    expect(markup).toContain("asks to talk to the controller");
    expect(markup).toContain("1 SysEx message ");
    expect(markup).toContain("Nothing is sent until you allow it");
    expect(markup).toContain("F0 00 20 6B 7F 42 02 00 40 50 01 F7");
    expect(markup).toContain(">Allow<");
  });

  it("offers to stop a package the player allowed, never one RackForge ships", () => {
    const allowed: ControllerOutput = { ...sysex, state: "allowed", messages: ["B0 7F 00", "C0 05"], sysex: false };
    const own = renderToStaticMarkup(
      <ControllerOutputNotice output={allowed} shipped={false} canAnswer busy={false} onAnswer={vi.fn()} />,
    );
    expect(own).toContain("2 messages when the controller connects");
    expect(own).toContain("Stop sending");

    const shipped = renderToStaticMarkup(
      <ControllerOutputNotice output={allowed} shipped canAnswer busy={false} onAnswer={vi.fn()} />,
    );
    expect(shipped).not.toContain("<button");
  });

  it("only informs where the answer is not kept, and says nothing for a package that sends nothing", () => {
    const elsewhere = renderToStaticMarkup(
      <ControllerOutputNotice output={sysex} shipped={false} canAnswer={false} busy={false} onAnswer={vi.fn()} />,
    );
    expect(elsewhere).not.toContain("<button");

    const silent = renderToStaticMarkup(
      <ControllerOutputNotice
        output={{ state: "none", messages: [], sysex: false }}
        shipped={false}
        canAnswer
        busy={false}
        onAnswer={vi.fn()}
      />,
    );
    expect(silent).toBe("");
  });
});
