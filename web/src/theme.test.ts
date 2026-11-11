import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

/**
 * The theme is three stylesheets in a fixed order, and the last one wins by
 * `!important`. That arrangement works, but it hides things: a colour
 * written in styles.css for a control the faceplate paints never reaches a
 * screen, while still reading like the answer to "what colour is this?".
 *
 * It read like the answer once. A harness built from tokens.css and
 * styles.css rendered the primary key as a cyan gradient; the product's key
 * is red, and had been for as long as the faceplate existed. Nothing was
 * broken -- the harness was simply not the theme.
 *
 * These tests hold the parts of that arrangement a reader has to trust:
 * the load order, the single home of the primary key's colour, and the two
 * copies of the dark palette that have to stay identical.
 *
 * See docs/WEB_THEME.md.
 */

const read = (relative: string) =>
  readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");

interface Rule {
  context: string;
  selector: string;
  declarations: [string, string][];
}

/** Every rule in a stylesheet, with its `@media` context kept. */
function parse(css: string): Rule[] {
  const source = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const rules: Rule[] = [];
  const stack: (string | null)[] = [];
  const token = /([^{}]*)([{}])/g;
  let match: RegExpExecArray | null;
  while ((match = token.exec(source))) {
    const head = match[1].trim();
    if (match[2] === "}") {
      stack.pop();
      continue;
    }
    if (head.startsWith("@")) {
      stack.push(head);
      continue;
    }
    const body = source.slice(token.lastIndex, source.indexOf("}", token.lastIndex));
    const declarations = body
      .split(";")
      .filter((entry) => entry.includes(":"))
      .map((entry): [string, string] => {
        const at = entry.indexOf(":");
        return [entry.slice(0, at).trim(), entry.slice(at + 1).trim()];
      });
    rules.push({
      context: stack.filter(Boolean).join(" & "),
      selector: head.replace(/\s+/g, " "),
      declarations,
    });
    stack.push(null);
  }
  return rules;
}

/** `background` covers what the longhands would have set. */
const COVERS: Record<string, string[]> = {
  background: ["background", "background-color", "background-image"],
};

function shadowedDeclarations(styles: Rule[], faceplate: Rule[]) {
  const forced = new Map<string, Set<string>>();
  for (const rule of faceplate) {
    for (const [property, value] of rule.declarations) {
      if (!value.includes("!important")) continue;
      for (const selector of rule.selector.split(",")) {
        const key = `${rule.context}||${selector.trim()}`;
        const set = forced.get(key) ?? new Set<string>();
        set.add(property);
        forced.set(key, set);
      }
    }
  }
  const shadowed: string[] = [];
  for (const rule of styles) {
    for (const selector of rule.selector.split(",")) {
      const target = forced.get(`${rule.context}||${selector.trim()}`);
      if (!target) continue;
      for (const [property, value] of rule.declarations) {
        if (value.includes("!important")) continue;
        const covered = [...target].some(
          (forcedProperty) =>
            property === forcedProperty ||
            (COVERS[forcedProperty] ?? []).includes(property),
        );
        if (covered) shadowed.push(`${selector.trim()} { ${property} }`);
      }
    }
  }
  return shadowed;
}

describe("the theme's load order", () => {
  const main = read("./main.tsx");

  it("loads the vocabulary, then the structure, then the material", () => {
    const order = ["./design/tokens.css", "./styles.css", "./faceplate.css"].map(
      (sheet) => main.indexOf(`import "${sheet}"`),
    );
    for (const [index, position] of order.entries()) {
      expect(position, `stylesheet ${index} is not imported`).toBeGreaterThan(-1);
    }
    expect(
      order,
      "the faceplate must load last or it stops being what RackForge looks like",
    ).toEqual([...order].sort((left, right) => left - right));
  });
});

describe("the primary key", () => {
  const styles = parse(read("./styles.css"));
  const faceplate = parse(read("./faceplate.css"));

  it("is painted by the faceplate", () => {
    const paints = faceplate.filter(
      (rule) =>
        rule.selector === ".primary-button" &&
        rule.declarations.some(
          ([property, value]) =>
            property === "background" && value.includes("!important"),
        ),
    );
    expect(paints.length, "the faceplate no longer fills the primary key").toBe(1);
  });

  it("is not painted anywhere else", () => {
    // Two dead fills lived here: `var(--acid)` and a cyan gradient. Neither
    // reached a screen, and both answered the question wrongly for a reader.
    const elsewhere = styles.filter(
      (rule) =>
        rule.selector === ".primary-button" &&
        rule.declarations.some(([property]) =>
          ["background", "background-color", "background-image", "color"].includes(
            property,
          ),
        ),
    );
    expect(
      elsewhere.map((rule) => rule.declarations),
      "styles.css paints the primary key again; the faceplate will win and the reader will not",
    ).toEqual([]);
  });
});

describe("what styles.css declares and the faceplate overrules", () => {
  const styles = parse(read("./styles.css"));
  const faceplate = parse(read("./faceplate.css"));
  const shadowed = shadowedDeclarations(styles, faceplate);

  it("has something to count", () => {
    // A guard that passes because it found nothing is not a guard.
    expect(shadowed.length).toBeGreaterThan(0);
  });

  it("does not grow", () => {
    // Every one of these is a declaration that reads like the answer and is
    // not. Lowering this number is welcome; raising it means a colour was
    // written where nobody will see it take effect.
    expect(
      shadowed.length,
      "styles.css declares more that the faceplate overrules; write it in the faceplate instead",
    ).toBeLessThanOrEqual(887);
  });
});

describe("the two copies of the dark palette", () => {
  const styles = read("./styles.css");

  it("agree, token for token", () => {
    // "Follow the system" and an explicit STAGE stamp are different
    // selectors, so the palette is written twice. They drift silently.
    const auto = styles.match(
      /@media \(prefers-color-scheme: dark\) \{\s*:root:not\(\[data-theme="light"\]\) \{([\s\S]*?)\n {2}\}/,
    );
    const stamped = styles.match(/:root\[data-theme="dark"\] \{([\s\S]*?)\n\}/);
    expect(auto, "the prefers-color-scheme copy is missing").not.toBeNull();
    expect(stamped, "the data-theme copy is missing").not.toBeNull();

    const tokensOf = (block: string) =>
      block
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.startsWith("--"));

    const fromAuto = tokensOf(auto![1]);
    expect(fromAuto.length, "the dark palette has no tokens to compare").toBeGreaterThan(0);
    expect(
      tokensOf(stamped![1]),
      "a dark value changed in one copy only; STAGE now depends on how you got there",
    ).toEqual(fromAuto);
  });
});
