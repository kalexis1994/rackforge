import type { SessionCommand } from "./types";

export const SESSION_SCHEMA_VERSION = 14;

export function serializeSessionCommand(
  clientId: string,
  commandId: number,
  command: SessionCommand,
): string {
  return JSON.stringify({
    op: "dispatch",
    envelope: {
      schema_version: SESSION_SCHEMA_VERSION,
      client_id: clientId,
      command_id: commandId,
      command,
    },
  });
}
