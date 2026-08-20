import type { SessionCommand } from "./types";

export interface PlayPluginTarget {
  pluginId: string;
  pluginName: string;
  instanceId?: string;
}

export interface ActiveProgramDraft {
  draftId: number;
  dirty: boolean;
}

export interface PlayPluginSelectionRequest {
  target: PlayPluginTarget;
  activeInstanceId?: string;
  programDraft?: ActiveProgramDraft;
  discardDraft?: boolean;
}

export type PlayPluginSelectionPreflight =
  | { status: "already_active" }
  | { status: "confirmation_required"; dirty: boolean }
  | { status: "ready" };

export interface PlayPluginSelectionOperations {
  dispatch(command: SessionCommand): Promise<unknown>;
  activate(pluginId: string): Promise<unknown>;
  synchronize(): Promise<unknown>;
}

export function preflightPlayPluginSelection(
  request: PlayPluginSelectionRequest,
): PlayPluginSelectionPreflight {
  if (
    request.target.instanceId &&
    request.target.instanceId === request.activeInstanceId
  ) {
    return { status: "already_active" };
  }
  if (request.programDraft && !request.discardDraft) {
    return {
      status: "confirmation_required",
      dirty: request.programDraft.dirty,
    };
  }
  return { status: "ready" };
}

/**
 * Applies the PLAY ownership transition in the only safe order.
 *
 * Program editing owns an audition lease, so it must end before the host can
 * move the audio path. Every command is awaited to prevent the selector from
 * claiming success while Desktop/Core has rejected the transition.
 */
export async function commitPlayPluginSelection(
  request: PlayPluginSelectionRequest,
  operations: PlayPluginSelectionOperations,
): Promise<void> {
  const preflight = preflightPlayPluginSelection(request);
  if (preflight.status !== "ready") {
    throw new Error(`PLAY plugin selection is not ready: ${preflight.status}`);
  }

  if (request.programDraft) {
    await operations.dispatch({
      type: "cancel_program_edit",
      draft_id: request.programDraft.draftId,
    });
  }
  await operations.dispatch({ type: "set_active_mode", mode: "play" });
  if (request.target.instanceId) {
    await operations.dispatch({
      type: "select_plugin",
      instance_id: request.target.instanceId,
    });
    return;
  }
  await operations.activate(request.target.pluginId);
  await operations.synchronize();
}
