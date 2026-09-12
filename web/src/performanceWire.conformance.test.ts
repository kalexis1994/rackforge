import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import {
  hasInterface,
  hasType,
  interfaceProperties,
  typeBlock,
  unionVariantNames,
} from "./wireSource";

/**
 * The performance library, from the side that mirrors it.
 *
 * Nearly every type in `rackforge-performance-api` is declared a second time
 * in `types.ts`, by hand, so this interface can build a rack or a song and
 * send it back. They all carry `deny_unknown_fields`. A field invented,
 * misspelled or renamed here is not a setting that fails to apply — the host
 * refuses the edit, and what the player loses is the work they just did.
 *
 * The record on the other side is not a list anybody maintains: every name in
 * it is asked of serde by offering it a field no type could have and reading
 * back what it says it expected. That is why the record knows about
 * `RackDefinition.items` and `RackSlot.program_id` — legacy spellings a
 * migration still honours, which no sample would have shown. Regenerate it
 * with `UPDATE_PERFORMANCE_WIRE=1 cargo test -p rackforge-performance-api`.
 */

interface Shape {
  name: string;
  fields: string[] | null;
}

interface Vocabulary {
  name: string;
  variants: string[] | null;
}

const read = (relative: string) =>
  readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");

const wire: { contract: string; shapes: Shape[]; vocabularies: Vocabulary[] } = JSON.parse(
  read("../../crates/rackforge-performance-api/fixtures/performance-wire-v1.json"),
);
const source = read("./types.ts");

describe(`the performance library, as this interface mirrors it (${wire.contract})`, () => {
  it("has a record to answer to", () => {
    expect(wire.shapes.length).toBeGreaterThan(0);
    expect(wire.vocabularies.length).toBeGreaterThan(0);
  });

  /**
   * The host cannot see what TypeScript declares, so its record names the
   * shapes it believes are mirrored. A name it lists and this file does not
   * declare means one side dropped or renamed a type, and this is the only
   * place the two lists meet.
   */
  it("declares every shape the host records", () => {
    const missing = wire.shapes
      .map((shape) => shape.name)
      .filter((name) => !hasInterface(source, name));
    expect(missing).toEqual([]);
  });

  it("declares every vocabulary the host records", () => {
    const missing = wire.vocabularies
      .map((vocabulary) => vocabulary.name)
      .filter((name) => !hasType(source, name));
    expect(missing).toEqual([]);
  });

  for (const shape of wire.shapes) {
    it(`spells ${shape.name} the way the host reads it`, () => {
      // A field the host does not accept is refused by
      // `deny_unknown_fields`, and the edit carrying it goes with it.
      expect(shape.fields, `${shape.name} is no longer a closed shape`).not.toBeNull();
      for (const property of interfaceProperties(source, shape.name)) {
        expect(shape.fields, `${shape.name}.${property.name} is not on the wire`).toContain(
          property.name,
        );
      }
    });
  }

  for (const vocabulary of wire.vocabularies) {
    it(`names ${vocabulary.name} the way the host reads it`, () => {
      expect(
        vocabulary.variants,
        `${vocabulary.name} is no longer a closed vocabulary`,
      ).not.toBeNull();
      for (const variant of unionVariantNames(typeBlock(source, vocabulary.name))) {
        expect(
          vocabulary.variants,
          `${vocabulary.name}.${variant} is not a variant the host knows`,
        ).toContain(variant);
      }
    });
  }
});
