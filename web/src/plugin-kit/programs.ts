/**
 * The logic behind `<rf-program-select>`: finding a program among many by a
 * few typed letters, filtering by bank, and stepping to the next one. Pure,
 * so it is tested apart from the element.
 */

/** A program as the host's context lists it (`instance.sounds`). */
export interface Program {
  id: string;
  name: string;
  bank?: string;
  detail?: string;
}

/** A bank as the host's context lists it (`instance.banks`). */
export interface Bank {
  id: string;
  name: string;
  order?: number;
}

/** Host contexts are cloned across the iframe bridge, even when unchanged. */
export function samePrograms(left: readonly Program[], right: readonly Program[]): boolean {
  return left === right || (
    left.length === right.length &&
    left.every((program, index) => {
      const other = right[index];
      return program.id === other.id && program.name === other.name &&
        program.bank === other.bank && program.detail === other.detail;
    })
  );
}

export function sameBanks(left: readonly Bank[], right: readonly Bank[]): boolean {
  return left === right || (
    left.length === right.length &&
    left.every((bank, index) => {
      const other = right[index];
      return bank.id === other.id && bank.name === other.name && bank.order === other.order;
    })
  );
}

export interface ProgramMatch {
  program: Program;
  /** Its place in the plugin's own list, from 0: what "12/128" counts. */
  index: number;
  score: number;
}

/** Lower case, accents and punctuation gone: "Élan-2" and "elan 2" meet. */
export function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();
}

/**
 * How well `query` finds `text`, or null when it does not. Higher is better:
 * the whole text, then its start, then a word's start, then anywhere, then
 * the query's letters in order with gaps (so "wrlz" finds "Wurlitzer").
 */
export function fuzzyScore(query: string, text: string): number | null {
  const needle = fold(query);
  if (!needle) return 0;
  const haystack = fold(text);
  if (!haystack) return null;
  if (haystack === needle) return 1000;
  if (haystack.startsWith(needle)) return 900 - haystack.length;
  const words = haystack.split(" ");
  if (words.some((word) => word.startsWith(needle))) return 800 - haystack.length;
  const at = haystack.indexOf(needle);
  if (at >= 0) return 700 - at;
  // Every word of the query at a word's start, in any order: "grand bright"
  // finds "Bright Grand".
  const parts = needle.split(" ");
  if (
    parts.length > 1 &&
    parts.every((part) => words.some((word) => word.startsWith(part)))
  ) {
    return 600;
  }
  // The query's letters in order; each gap costs.
  let position = 0;
  let gaps = 0;
  for (const letter of needle.replace(/ /g, "")) {
    const found = haystack.indexOf(letter, position);
    if (found < 0) return null;
    gaps += found - position;
    position = found + 1;
  }
  return Math.max(1, 400 - gaps * 4);
}

/** The bank's shown name: its own, or the id the program carries. */
export function bankName(banks: readonly Bank[], id: string | undefined): string | undefined {
  if (!id) return undefined;
  return banks.find((bank) => bank.id === id)?.name ?? id;
}

/**
 * The programs a query and a bank filter leave, best first. An empty query
 * keeps the plugin's own order. A number finds the program at that place,
 * as the "12/128" beside the name counts.
 */
export function searchPrograms(
  programs: readonly Program[],
  banks: readonly Bank[],
  query: string,
  bank: string | null = null,
): ProgramMatch[] {
  const trimmed = query.trim();
  const number = /^\d+$/.test(trimmed) ? Number(trimmed) : null;
  const matches: ProgramMatch[] = [];
  programs.forEach((program, index) => {
    if (bank !== null && program.bank !== bank) return;
    if (!trimmed) {
      matches.push({ program, index, score: 0 });
      return;
    }
    const scores = [
      fuzzyScore(trimmed, program.name),
      // The bank and the detail find too, below the name.
      scaled(fuzzyScore(trimmed, bankName(banks, program.bank) ?? ""), 0.5),
      scaled(fuzzyScore(trimmed, program.detail ?? ""), 0.4),
      number !== null && index + 1 === number ? 1100 : null,
    ].filter((score): score is number => score !== null);
    if (scores.length > 0) matches.push({ program, index, score: Math.max(...scores) });
  });
  if (trimmed) matches.sort((left, right) => right.score - left.score || left.index - right.index);
  return matches;
}

function scaled(score: number | null, factor: number): number | null {
  return score === null ? null : score * factor;
}

/** The banks worth offering as filters: those some program is filed under, in order. */
export function usedBanks(programs: readonly Program[], banks: readonly Bank[]): Bank[] {
  const used = new Set(programs.flatMap((program) => (program.bank ? [program.bank] : [])));
  const known = banks
    .filter((bank) => used.has(bank.id))
    .sort((left, right) => (left.order ?? 0) - (right.order ?? 0));
  const unknown = [...used]
    .filter((id) => !banks.some((bank) => bank.id === id))
    .map((id) => ({ id, name: id }));
  return [...known, ...unknown];
}

/**
 * The program `delta` places away, wrapping round the list; the first (or
 * last) when none is chosen. Null only for an empty list.
 */
export function stepProgram(
  programs: readonly Program[],
  currentId: string | null | undefined,
  delta: number,
): string | null {
  if (programs.length === 0) return null;
  const at = programs.findIndex((program) => program.id === currentId);
  if (at < 0) return programs[delta < 0 ? programs.length - 1 : 0].id;
  const next = (((at + delta) % programs.length) + programs.length) % programs.length;
  return programs[next].id;
}
