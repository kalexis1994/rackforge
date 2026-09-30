import { describe, expect, it } from "vitest";
import {
  associationControl,
  associationLayer,
  associationSource,
  menuAssociationItems,
  parameterAssociations,
} from "./parameterAssociations";
import type { ControlMapping, ParameterLink } from "./types";

const link: ParameterLink = {
  schema_version: 1,
  id: "link-1",
  instance_id: "rf-5",
  parameter_index: 17,
  source: { source_id: "usb-1", display_name: "KeyLab 61" },
  channel: { mode: "channel", channel: 1 },
  message: { type: "control_change", controller: 74 },
  transform: { invert: false },
  pass_through: "pass_through",
};

const mapping = (id: string, name: string, layer?: "fn"): ControlMapping => ({
  id,
  input: { id: `in-${id}`, name, channel: { mode: "omni" }, message: { type: "control_change", controller: 21 } },
  parameter_id: "filter-cutoff",
  ...(layer ? { layer } : {}),
});

describe("parameter associations", () => {
  it("lists the session's links, then every controller mapping", () => {
    const list = parameterAssociations([link], [
      { controller_id: "keylab", controller_name: "KeyLab mkII", mapping: mapping("a", "Knob 1") },
      { controller_id: "launchkey", controller_name: "Launchkey", mapping: mapping("b", "CC 21", "fn") },
    ]);
    expect(list.map((entry) => entry.key)).toEqual([
      "session:link-1",
      "controller:keylab:a",
      "controller:launchkey:b",
    ]);
    expect(associationSource(list[0])).toBe("KeyLab 61");
    expect(associationControl(list[0])).toBe("CC 74 · channel 1");
    expect(associationSource(list[1])).toBe("KeyLab mkII");
    expect(associationControl(list[1])).toBe("Knob 1 · CC 21 · any channel");
    // A control whose name is just its message is not named twice.
    expect(associationControl(list[2])).toBe("CC 21 · any channel");
    expect(associationLayer(list[1])).toBe("base");
    expect(associationLayer(list[2])).toBe("fn");
  });

  it("keeps the menu short: one association in it, more in the dialog", () => {
    expect(menuAssociationItems(0)).toBe("link");
    expect(menuAssociationItems(1)).toBe("one");
    expect(menuAssociationItems(2)).toBe("dialog");
    expect(menuAssociationItems(40)).toBe("dialog");
  });
});
