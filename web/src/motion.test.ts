import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * The motion vocabulary is a contract, not a suggestion: one set of
 * durations and curves shared by Web, the native WebViews and the VST
 * surface. These tests keep it honest, because the way it eroded once was
 * quiet — new transitions written with ad-hoc numbers, and a
 * reduced-motion guard that named two selectors by hand while the panel
 * grew three more transitions around it.
 */

const read = (relative: string) =>
  readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");

/** Every `transition:`/`animation:` value in a stylesheet, comments stripped. */
function motionDeclarations(css: string) {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  return [...withoutComments.matchAll(/\b(transition|animation)\s*:([^;{}]*);/g)].map(
    (match) => ({ property: match[1], value: match[2].trim() }),
  );
}

const VOCABULARY = [
  "--rf-motion-instant",
  "--rf-motion-fast",
  "--rf-motion-standard",
  "--rf-motion-emphasized",
  "--rf-motion-exit",
] as const;

describe("the motion vocabulary", () => {
  const tokens = read("./design/tokens.css");

  it("defines every duration once, at the root", () => {
    for (const token of VOCABULARY) {
      expect(tokens, `${token} is missing from the vocabulary`).toContain(`${token}:`);
    }
  });

  it("collapses the whole vocabulary when the player asks for less motion", () => {
    const reduced = tokens.slice(tokens.indexOf("@media (prefers-reduced-motion: reduce)"));
    expect(reduced, "the reduce block is missing").not.toHaveLength(0);
    for (const token of VOCABULARY) {
      expect(
        reduced,
        `${token} keeps its duration under the reduce preference`,
      ).toMatch(new RegExp(`${token}:\\s*1ms`));
    }
  });
});

describe("the faceplate", () => {
  const faceplate = read("./faceplate.css");
  const declarations = motionDeclarations(faceplate);

  it("has motion to check", () => {
    // A guard that passes because it found nothing is not a guard.
    expect(declarations.length).toBeGreaterThan(0);
  });

  it("times every movement from the vocabulary, never a raw number", () => {
    const offenders = declarations.filter(
      (declaration) =>
        /\d+\s*m?s\b/.test(declaration.value) && !declaration.value.includes("var(--rf-motion"),
    );
    expect(
      offenders.map((offender) => `${offender.property}: ${offender.value}`),
      "these declarations carry their own duration instead of the shared one — " +
        "a raw duration also escapes the reduced-motion collapse in tokens.css",
    ).toEqual([]);
  });

  it("curves every movement from the vocabulary, never the browser default", () => {
    const offenders = declarations.filter(
      (declaration) =>
        declaration.value.includes("var(--rf-motion") &&
        !declaration.value.includes("var(--rf-ease"),
    );
    expect(
      offenders.map((offender) => `${offender.property}: ${offender.value}`),
      "these declarations take a shared duration but the browser's own easing",
    ).toEqual([]);
  });

  it("needs no reduced-motion guard of its own", () => {
    // Naming selectors by hand is what rotted last time: the tokens carry
    // the preference for everything that speaks the vocabulary.
    expect(faceplate).not.toMatch(/@media\s*\(prefers-reduced-motion[^)]*\)\s*{/);
  });
});
