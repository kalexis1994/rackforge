import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { SCALES } from "./sequencer";
import {
  interfaceProperties,
  memberProperties,
  stringLiterals,
  typeBlock,
  unionMembers,
  type DeclaredProperty,
} from "./wireSource";

/**
 * The sequencer wire, from the side that builds it.
 *
 * `SequencerCommand` carries `deny_unknown_fields` in the host. A field this
 * surface invents, misspells, or leaves out does not cost a setting — it
 * costs the whole command, silently, because the transport is fire and
 * forget. And a TypeScript union cannot check itself against anything: the
 * types are gone before a test could look at them.
 *
 * So this reads the union out of the source, the way `motion.test.ts` reads
 * the motion vocabulary out of the stylesheet, and holds it to what
 * `rackforge-control-api` records in
 * `fixtures/sequencer-wire-v1.json`. Regenerate that with
 * `UPDATE_SEQUENCER_WIRE=1 cargo test -p rackforge-control-api`.
 *
 * Nothing here reads the call sites, and it does not need to:
 * `sendSequencerCommand` takes a `SequencerCommand` and is the only way out,
 * so the compiler has already held every call to this union. Checking the
 * union checks them all.
 */

interface WireCommand {
  kind: string;
  fields: string[];
  required: string[];
}

interface Wire {
  contract: string;
  commands: WireCommand[];
  quantize: string[];
  scales: string[];
  status_fields: string[];
  status_required: string[];
  lane_status_fields: string[];
  lane_status_required: string[];
}

const read = (relative: string) =>
  readFileSync(fileURLToPath(new URL(relative, import.meta.url)), "utf8");

const wire: Wire = JSON.parse(
  read("../../crates/rackforge-control-api/fixtures/sequencer-wire-v1.json"),
);
const source = read("./sequencer.ts");

/** The surface's own account of what it can send. */
const declared = unionMembers(typeBlock(source, "SequencerCommand")).map((member) => {
  const kind = /kind:\s*"([a-z_]+)"/.exec(member);
  if (!kind) throw new Error(`a SequencerCommand member has no kind: ${member}`);
  return {
    kind: kind[1],
    properties: memberProperties(member).filter((property) => property.name !== "kind"),
  };
});

const names = (list: DeclaredProperty[]) => list.map((entry) => entry.name).sort();
const requiredNames = (list: DeclaredProperty[]) =>
  list
    .filter((entry) => !entry.optional)
    .map((entry) => entry.name)
    .sort();

describe(`the sequencer wire, as this surface builds it (${wire.contract})`, () => {
  it("reads its own union", () => {
    expect(declared.length).toBeGreaterThan(0);
    expect(new Set(declared.map((command) => command.kind)).size).toBe(declared.length);
  });

  /**
   * A subset, deliberately, though today the two sets are the same size.
   * The direction that breaks a show is this one: a command the host cannot
   * parse is refused whole. The other direction — the host growing an
   * instruction this surface has no button for — is a gap in the surface and
   * a decision for whoever adds the button, not a failing test.
   */
  it("sends only instructions the host knows", () => {
    const known = new Set(wire.commands.map((command) => command.kind));
    for (const command of declared) {
      expect(known, `unknown command ${command.kind}`).toContain(command.kind);
    }
  });

  for (const command of declared) {
    const host = wire.commands.find((entry) => entry.kind === command.kind);

    it(`spells ${command.kind} the way the host reads it`, () => {
      // An invented or misspelled field is refused by `deny_unknown_fields`,
      // and the command goes with it.
      for (const property of names(command.properties)) {
        expect(host?.fields, `${command.kind}.${property} is not on the wire`).toContain(property);
      }
    });

    it(`always sends what ${command.kind} demands`, () => {
      // A field with no default on the host side: leaving it out fails the
      // whole command just as surely as inventing one.
      for (const property of host?.required ?? []) {
        expect(
          requiredNames(command.properties),
          `${command.kind}.${property} is required by the host`,
        ).toContain(property);
      }
    });
  }

  it("offers exactly the quantise boundaries the host resolves", () => {
    expect(stringLiterals(typeBlock(source, "SequencerQuantize")).sort()).toEqual(
      [...wire.quantize].sort(),
    );
  });

  it("offers exactly the scales the host can follow", () => {
    expect(SCALES.map((scale) => scale.id).sort()).toEqual([...wire.scales].sort());
  });

  it("reads only the status fields the host sends", () => {
    const declaredStatus = interfaceProperties(source, "SequencerStatus");
    for (const property of names(declaredStatus)) {
      expect(wire.status_fields, `SequencerStatus.${property} is never sent`).toContain(property);
    }
    // What the host always sends, this side may rely on.
    for (const property of wire.status_required) {
      expect(requiredNames(declaredStatus), `${property} is always sent`).toContain(property);
    }
  });

  it("reads only the lane status fields the host sends", () => {
    const declaredLane = interfaceProperties(source, "SequencerLaneStatus");
    for (const property of names(declaredLane)) {
      expect(wire.lane_status_fields, `SequencerLaneStatus.${property} is never sent`).toContain(
        property,
      );
    }
    for (const property of wire.lane_status_required) {
      expect(requiredNames(declaredLane), `${property} is always sent`).toContain(property);
    }
  });
});
