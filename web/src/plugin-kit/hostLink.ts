/**
 * What `<rf-program-select>` says to and hears from RackForge, over the
 * plugin web protocol (docs/WEB_PLUGIN_API.md). Pure: the element posts and
 * listens; this decides what the messages mean.
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
        const { id, name, bank, detail } = sound as Record<string, unknown>;
        if (typeof id !== "string" || typeof name !== "string") return [];
        return [
          {
            id,
            name,
            ...(typeof bank === "string" ? { bank } : {}),
            ...(typeof detail === "string" ? { detail } : {}),
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
