import { describe, expect, it } from "vitest";
import { MAX_NAME_LENGTH, nameProblem, replaceable, saveModes, startingName } from "./program-save-logic";

describe("what the program save dialog allows", () => {
  it("checks a name as RackForge does", () => {
    expect(nameProblem("  Low Strings ")).toBeNull();
    expect(nameProblem("   ")).toBe("empty");
    expect(nameProblem("x".repeat(MAX_NAME_LENGTH + 1))).toBe("long");
    expect(nameProblem("Bell\u0007")).toBe("control");
  });

  it("saves over only the plugin's own programs", () => {
    const programs = [
      { id: "factory.1", name: "Brass" },
      { id: "custom.1", name: "Mine", editable: true as const },
    ];
    expect(replaceable(programs, "custom.1")?.name).toBe("Mine");
    expect(replaceable(programs, "factory.1")).toBeNull();
    expect(replaceable(programs, null)).toBeNull();
  });

  it("offers replacing first when it can, and honours a preferred Enter", () => {
    expect(saveModes(false, null)).toEqual({ modes: ["new"], primary: "new" });
    expect(saveModes(false, "replace")).toEqual({ modes: ["new"], primary: "new" });
    expect(saveModes(true, null)).toEqual({ modes: ["replace", "new"], primary: "replace" });
    expect(saveModes(true, "new")).toEqual({ modes: ["replace", "new"], primary: "new" });
  });

  it("starts the field from what it is given, the current program, or a fallback", () => {
    expect(startingName(" Pad ", { id: "a", name: "Brass" }, "New program")).toBe("Pad");
    expect(startingName(null, { id: "a", name: "Brass" }, "New program")).toBe("Brass");
    expect(startingName("", undefined, "New program")).toBe("New program");
    expect(startingName("y".repeat(80), undefined, "x")).toHaveLength(MAX_NAME_LENGTH);
  });
});
