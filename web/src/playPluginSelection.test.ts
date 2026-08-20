import { describe, expect, it, vi } from "vitest";
import {
  commitPlayPluginSelection,
  preflightPlayPluginSelection,
  type PlayPluginSelectionOperations,
  type PlayPluginSelectionRequest,
} from "./playPluginSelection";

const target = {
  pluginId: "org.rackforge.piano",
  pluginName: "Piano",
  instanceId: "desktop.org.rackforge.piano",
};

function operations(log: string[] = []): PlayPluginSelectionOperations {
  return {
    dispatch: vi.fn(async (command) => {
      log.push(`dispatch:${command.type}`);
    }),
    activate: vi.fn(async (pluginId) => {
      log.push(`activate:${pluginId}`);
    }),
    synchronize: vi.fn(async () => {
      log.push("synchronize");
    }),
  };
}

describe("PLAY plugin selection preflight", () => {
  it("recognizes the already active instance", () => {
    expect(
      preflightPlayPluginSelection({
        target,
        activeInstanceId: target.instanceId,
      }),
    ).toEqual({ status: "already_active" });
  });

  it("requires confirmation for a dirty program draft", () => {
    expect(
      preflightPlayPluginSelection({
        target,
        activeInstanceId: "desktop.org.rackforge.synth",
        programDraft: { draftId: 7, dirty: true },
      }),
    ).toEqual({ status: "confirmation_required", dirty: true });
  });

  it("also closes a clean edit explicitly instead of silently stealing its lease", () => {
    expect(
      preflightPlayPluginSelection({
        target,
        programDraft: { draftId: 7, dirty: false },
      }),
    ).toEqual({ status: "confirmation_required", dirty: false });
  });

  it("becomes ready after discard is confirmed", () => {
    expect(
      preflightPlayPluginSelection({
        target,
        programDraft: { draftId: 7, dirty: true },
        discardDraft: true,
      }),
    ).toEqual({ status: "ready" });
  });
});

describe("PLAY plugin selection transaction", () => {
  it("moves an existing instance to PLAY in order", async () => {
    const log: string[] = [];
    await commitPlayPluginSelection({ target }, operations(log));
    expect(log).toEqual([
      "dispatch:set_active_mode",
      "dispatch:select_plugin",
    ]);
  });

  it("cancels the edit lease before changing mode or plugin", async () => {
    const log: string[] = [];
    await commitPlayPluginSelection(
      {
        target,
        programDraft: { draftId: 19, dirty: true },
        discardDraft: true,
      },
      operations(log),
    );
    expect(log).toEqual([
      "dispatch:cancel_program_edit",
      "dispatch:set_active_mode",
      "dispatch:select_plugin",
    ]);
  });

  it("activates and synchronizes an installed package without an instance", async () => {
    const log: string[] = [];
    const request: PlayPluginSelectionRequest = {
      target: {
        pluginId: "org.rackforge.new",
        pluginName: "New Instrument",
      },
    };
    await commitPlayPluginSelection(request, operations(log));
    expect(log).toEqual([
      "dispatch:set_active_mode",
      "activate:org.rackforge.new",
      "synchronize",
    ]);
  });

  it("stops immediately when Core rejects a command", async () => {
    const log: string[] = [];
    const ops = operations(log);
    vi.mocked(ops.dispatch).mockRejectedValueOnce(new Error("mode rejected"));
    await expect(commitPlayPluginSelection({ target }, ops)).rejects.toThrow(
      "mode rejected",
    );
    expect(ops.activate).not.toHaveBeenCalled();
    expect(log).toEqual([]);
  });

  it("does not run a transaction that still needs confirmation", async () => {
    const ops = operations();
    await expect(
      commitPlayPluginSelection(
        {
          target,
          programDraft: { draftId: 7, dirty: true },
        },
        ops,
      ),
    ).rejects.toThrow("confirmation_required");
    expect(ops.dispatch).not.toHaveBeenCalled();
  });
});
