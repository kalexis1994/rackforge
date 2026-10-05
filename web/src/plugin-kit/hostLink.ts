/**
 * What the plugin kit's elements (`<rf-program-select>`, `<rf-program-save>`)
 * say to and hear from RackForge, over the plugin web protocol
 * (docs/WEB_PLUGIN_API.md). Pure: the elements post and listen; this decides
 * what the messages mean.
 */
import type { Bank, Program } from "./programs";

export const PROTOCOL = "rackforge.plugin.web@1";

/** What a context tells the selector. */
export interface ProgramContext {
  programs: Program[];
  banks: Bank[];
  selected: string | null;
  /** A Rack/Song Part owns an isolated plugin state, unlike PLAY. */
  isolated: boolean;
}

/** The programs in a host context, or null for any other message. */
export function readContext(message: unknown): ProgramContext | null {
  if (!isProtocolMessage(message) || message.kind !== "context") return null;
  const instance = (message as { instance?: unknown }).instance;
  if (!instance || typeof instance !== "object") return null;
  const { sounds, banks, selected_sound_id: selected } = instance as {
    sounds?: unknown;
    banks?: unknown;
    selected_sound_id?: unknown;
  };
  const programs = Array.isArray(sounds)
    ? sounds.flatMap((sound): Program[] => {
        if (!sound || typeof sound !== "object") return [];
        const { id, name, bank, detail, editable } = sound as Record<string, unknown>;
        if (typeof id !== "string" || typeof name !== "string") return [];
        return [
          {
            id,
            name,
            ...(typeof bank === "string" ? { bank } : {}),
            ...(typeof detail === "string" ? { detail } : {}),
            ...(editable === true ? { editable: true as const } : {}),
          },
        ];
      })
    : [];
  const bankList = Array.isArray(banks)
    ? banks.flatMap((bank): Bank[] => {
        if (!bank || typeof bank !== "object") return [];
        const { id, name, order } = bank as Record<string, unknown>;
        if (typeof id !== "string" || typeof name !== "string") return [];
        return [{ id, name, ...(typeof order === "number" ? { order } : {}) }];
      })
    : [];
  return {
    programs,
    banks: bankList,
    selected: typeof selected === "string" ? selected : null,
    isolated: (message as { isolated?: unknown }).isolated === true,
  };
}

/** The program being edited, as a context carries it (`program_draft`). */
export interface ProgramDraft {
  draftId: number;
  name: string;
  /** The program it will be saved over; absent for a new one. */
  originalProgramId?: string;
}

/**
 * The draft a context carries: the draft, null when the context has none, or
 * undefined when the message is not a context at all.
 */
export function readProgramDraft(message: unknown): ProgramDraft | null | undefined {
  if (!isProtocolMessage(message) || message.kind !== "context") return undefined;
  const draft = (message as { program_draft?: unknown }).program_draft;
  if (!draft || typeof draft !== "object") return null;
  const { draft_id: draftId, name, original_program_id: original } = draft as Record<string, unknown>;
  if (typeof draftId !== "number" || !Number.isFinite(draftId)) return null;
  return {
    draftId,
    name: typeof name === "string" ? name : "",
    ...(typeof original === "string" ? { originalProgramId: original } : {}),
  };
}

function request(requestId: string, method: string, params: Record<string, unknown>) {
  return { protocol: PROTOCOL, kind: "request", request_id: requestId, method, params } as const;
}

/** Starts a draft: of a new program from what is playing (`null`), or of one of the plugin's own. */
export function beginProgramEditRequest(requestId: string, programId: string | null) {
  return request(requestId, "plugin.begin_program_edit", { program_id: programId });
}

/** Names the draft. */
export function programNameRequest(requestId: string, draftId: number, name: string) {
  return request(requestId, "plugin.set_program_name", { draft_id: draftId, name });
}

/** Saves the draft. */
export function saveProgramRequest(requestId: string, draftId: number) {
  return request(requestId, "plugin.save_program", { draft_id: draftId });
}

/** Drops the draft. */
export function cancelProgramRequest(requestId: string, draftId: number) {
  return request(requestId, "plugin.cancel_program", { draft_id: draftId });
}

/** Asks the host for a context: the element may load after the plugin's own `ready`. */
export function readyMessage() {
  return { protocol: PROTOCOL, kind: "ready" } as const;
}

/** Chooses a program, as a plugin's own buttons would. */
export function selectRequest(requestId: string, soundId: string) {
  return {
    protocol: PROTOCOL,
    kind: "request",
    request_id: requestId,
    method: "plugin.select_sound",
    params: { sound_id: soundId },
  } as const;
}

/** The host's answer to one of the element's requests, or null. */
export function readResponse(
  message: unknown,
  requestId: string,
): { ok: boolean; error?: string } | null {
  if (!isProtocolMessage(message) || message.kind !== "response") return null;
  const response = message as { request_id?: unknown; ok?: unknown; error?: unknown };
  if (response.request_id !== requestId) return null;
  return {
    ok: response.ok === true,
    ...(typeof response.error === "string" ? { error: response.error } : {}),
  };
}

function isProtocolMessage(message: unknown): message is { protocol: string; kind: unknown } {
  return (
    !!message &&
    typeof message === "object" &&
    (message as { protocol?: unknown }).protocol === PROTOCOL
  );
}
