/**
 * Reading this interface's own type declarations back out of its source.
 *
 * TypeScript types are gone before anything runs, so a test cannot ask what
 * shape an interface has — there is nothing left to ask. The conformance
 * tests read the declarations out of the file instead, the way
 * `motion.test.ts` reads the motion vocabulary out of the stylesheet, and
 * hold them to what the host records about the same wire.
 *
 * This lives in one place because the first copy of it had a bug worth not
 * having twice: it ended a union at the first semicolon, which sits inside
 * the first member rather than after the last, and so read four members of
 * seventeen while reporting that every one of them agreed. Everything here
 * tracks brace depth for that reason.
 *
 * Only the conformance tests import this; nothing in the running interface
 * does.
 */

export interface DeclaredProperty {
  name: string;
  /** Declared with `?`, so the surface may leave it out. */
  optional: boolean;
}

/**
 * The source with its comments removed, which every reader below does first.
 *
 * A doc comment is prose, and prose contains colons. `SongPart` carries the
 * line "patterns this Part carries on stage: lane N speaks MIDI channel N+1",
 * and a reader that does not strip it first finds a property called `stage`
 * and reports that the interface declares a field the host has never heard
 * of. Braces inside a comment would mislead the depth counting in the same
 * way, so this runs over the whole file rather than over each block.
 */
export function stripComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
}

/** The span of `{ ... }` that starts at `open`, including both braces. */
function braceSpan(source: string, open: number): [number, number] {
  let depth = 0;
  for (let end = open; end < source.length; end += 1) {
    if (source[end] === "{") depth += 1;
    if (source[end] === "}") {
      depth -= 1;
      if (depth === 0) return [open, end];
    }
  }
  throw new Error(`unterminated braces from offset ${open}`);
}

/**
 * A `type` declaration, up to the semicolon that ends it — the one at brace
 * depth zero, not the first one seen.
 */
export function typeBlock(raw: string, name: string): string {
  const source = stripComments(raw);
  const start = source.indexOf(`export type ${name} =`);
  if (start < 0) throw new Error(`type ${name} is not declared`);
  const from = source.indexOf("=", start) + 1;
  let depth = 0;
  for (let end = from; end < source.length; end += 1) {
    if (source[end] === "{") depth += 1;
    if (source[end] === "}") depth -= 1;
    if (source[end] === ";" && depth === 0) return source.slice(from, end);
  }
  throw new Error(`type ${name} is never terminated`);
}

/** Whether the source declares an interface by this name. */
export function hasInterface(raw: string, name: string): boolean {
  return new RegExp(`export interface ${name}\\s*\\{`).test(stripComments(raw));
}

/** Whether the source declares a type by this name. */
export function hasType(raw: string, name: string): boolean {
  return new RegExp(`export type ${name}\\s*=`).test(stripComments(raw));
}

/** The body of `export interface Name { ... }`, braces excluded. */
export function interfaceBlock(raw: string, name: string): string {
  const source = stripComments(raw);
  const match = new RegExp(`export interface ${name}\\s*\\{`).exec(source);
  if (!match) throw new Error(`interface ${name} is not declared`);
  const open = source.indexOf("{", match.index);
  const [, close] = braceSpan(source, open);
  return source.slice(open + 1, close);
}

/** The members of a `|`-separated union, split at the top level only. */
export function unionMembers(block: string): string[] {
  const members: string[] = [];
  let depth = 0;
  let current = "";
  for (const character of block) {
    if (character === "{") depth += 1;
    if (character === "}") depth -= 1;
    if (character === "|" && depth === 0) {
      if (current.trim()) members.push(current);
      current = "";
    } else {
      current += character;
    }
  }
  if (current.trim()) members.push(current);
  return members;
}

/**
 * The properties declared directly in a block, ignoring anything nested
 * inside a further `{ ... }` — a field whose type is an object literal
 * contributes itself and not its insides.
 */
export function properties(block: string): DeclaredProperty[] {
  const found: DeclaredProperty[] = [];
  let depth = 0;
  let index = 0;
  while (index < block.length) {
    const character = block[index];
    if (character === "{") {
      depth += 1;
      index += 1;
      continue;
    }
    if (character === "}") {
      depth -= 1;
      index += 1;
      continue;
    }
    if (depth === 0) {
      const rest = block.slice(index);
      const match = /^(?:^|[;\n])\s*([A-Za-z_][A-Za-z0-9_]*)(\??):/.exec(rest);
      if (match) {
        found.push({ name: match[1], optional: match[2] === "?" });
        index += match[0].length;
        continue;
      }
    }
    index += 1;
  }
  return found;
}

/** The properties of an interface, nested object types excluded. */
export function interfaceProperties(source: string, name: string): DeclaredProperty[] {
  return properties(interfaceBlock(source, name));
}

/**
 * The properties of one union member, which arrives wearing its own braces.
 *
 * `properties` reads at brace depth zero so that a field whose type is an
 * object literal contributes itself and not its insides — which means a
 * member still wrapped in `{ }` has nothing at depth zero at all, and reads
 * as having no fields rather than as failing. Unwrap first.
 */
export function memberProperties(member: string): DeclaredProperty[] {
  const text = member.trim();
  const inner = text.startsWith("{") && text.endsWith("}") ? text.slice(1, -1) : text;
  return properties(inner);
}

/**
 * The names a union offers, in every spelling this interface uses for one:
 * a bare string literal, an object tagged with `kind`, and an object whose
 * single key is the variant name — which is what an externally tagged enum
 * with a payload looks like from here.
 */
export function unionVariantNames(block: string): string[] {
  const names: string[] = [];
  for (const member of unionMembers(block)) {
    const text = member.trim();
    const bare = /^"([a-z_]+)"$/.exec(text);
    if (bare) {
      names.push(bare[1]);
      continue;
    }
    const tagged = /kind:\s*"([a-z_]+)"/.exec(text);
    if (tagged) {
      names.push(tagged[1]);
      continue;
    }
    const [first] = memberProperties(text);
    if (first) {
      names.push(first.name);
      continue;
    }
    throw new Error(`cannot name this union member: ${text}`);
  }
  return names;
}

/** Every string literal in a block, for a union written as bare strings. */
export function stringLiterals(block: string): string[] {
  return [...block.matchAll(/"([a-z_]+)"/g)].map((match) => match[1]);
}
