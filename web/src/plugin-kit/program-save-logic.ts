/**
 * The logic behind `<rf-program-save>`: what a name must be, what the field
 * starts with, and which ways of saving the current program allows. Pure, so
 * it is tested apart from the element.
 */
import type { Program } from "./programs";

/** The longest name RackForge keeps, as it checks it (programName.ts). */
export const MAX_NAME_LENGTH = 64;

/** Why a name cannot be saved, or null when it can. The host checks the same. */
export function nameProblem(name: string): "empty" | "long" | "control" | null {
  const trimmed = name.trim();
  if (trimmed.length === 0) return "empty";
  if (trimmed.length > MAX_NAME_LENGTH) return "long";
  if (/[\p{Cc}\p{Cf}]/u.test(trimmed)) return "control";
  return null;
}

/** How a program can be saved. */
export type SaveMode = "new" | "replace";

/** The current program, when it is one of the plugin's own and can be saved over. */
export function replaceable(programs: Program[], selected: string | null): Program | null {
  const program = programs.find((candidate) => candidate.id === selected);
  return program?.editable ? program : null;
}

/**
 * The ways the dialog offers: always a new program; over the current one
 * only when it is the plugin's own. `primary` is what Enter does.
 */
export function saveModes(
  canReplace: boolean,
  preferred: SaveMode | null,
): { modes: SaveMode[]; primary: SaveMode } {
  const modes: SaveMode[] = canReplace ? ["replace", "new"] : ["new"];
  const primary = preferred && modes.includes(preferred) ? preferred : modes[0];
  return { modes, primary };
}

/** What the name field starts with: the given name, else the current program's, else the fallback. */
export function startingName(
  given: string | null | undefined,
  current: Program | undefined,
  fallback: string,
): string {
  const name = given?.trim() || current?.name?.trim() || fallback;
  return name.slice(0, MAX_NAME_LENGTH);
}
