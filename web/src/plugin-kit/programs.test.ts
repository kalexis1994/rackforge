import { describe, expect, it } from "vitest";
import { bankName, fold, fuzzyScore, searchPrograms, stepProgram, usedBanks, type Program } from "./programs";

const programs: Program[] = [
  { id: "a", name: "Bright Grand", bank: "piano" },
  { id: "b", name: "Wurlitzer 200A", bank: "keys", detail: "Tremolo" },
  { id: "c", name: "Élan Pad", bank: "pads" },
  { id: "d", name: "Grand Soft", bank: "piano" },
  { id: "e", name: "Brass Section" },
];
const banks = [
  { id: "keys", name: "Electric Keys", order: 2 },
  { id: "piano", name: "Pianos", order: 1 },
  { id: "pads", name: "Pads", order: 3 },
];

describe("finding a program by a few letters", () => {
  it("folds case, accents and punctuation", () => {
    expect(fold("Élan-Pad  2")).toBe("elan pad 2");
    expect(fuzzyScore("elan", "Élan Pad")).not.toBeNull();
  });

  it("ranks a start above a word above anywhere above scattered letters", () => {
    const start = fuzzyScore("gra", "Grand Soft")!;
    const word = fuzzyScore("gra", "Bright Grand")!;
    const inside = fuzzyScore("ran", "Bright Grand")!;
    const scattered = fuzzyScore("wrlz", "Wurlitzer 200A")!;
    expect(start).toBeGreaterThan(word);
    expect(word).toBeGreaterThan(inside);
    expect(inside).toBeGreaterThan(scattered);
    expect(fuzzyScore("zzq", "Wurlitzer")).toBeNull();
  });

  it("finds words in any order", () => {
    expect(searchPrograms(programs, banks, "grand bright").map((m) => m.program.id)[0]).toBe("a");
  });

  it("keeps the plugin's order with no query, and orders by score with one", () => {
    expect(searchPrograms(programs, banks, "").map((m) => m.program.id)).toEqual(["a", "b", "c", "d", "e"]);
    expect(searchPrograms(programs, banks, "grand").map((m) => m.program.id)).toEqual(["d", "a"]);
  });

  it("finds by bank name, by detail, and by place in the list", () => {
    expect(searchPrograms(programs, banks, "electric").map((m) => m.program.id)).toEqual(["b"]);
    expect(searchPrograms(programs, banks, "tremolo").map((m) => m.program.id)).toEqual(["b"]);
    expect(searchPrograms(programs, banks, "4")[0].program.id).toBe("d");
  });

  it("filters by bank", () => {
    expect(searchPrograms(programs, banks, "", "piano").map((m) => m.program.id)).toEqual(["a", "d"]);
    expect(searchPrograms(programs, banks, "soft", "keys")).toEqual([]);
  });
});

describe("banks and steps", () => {
  it("offers only the banks in use, in their order, and names unknown ones by id", () => {
    expect(usedBanks(programs, banks).map((bank) => bank.id)).toEqual(["piano", "keys", "pads"]);
    expect(usedBanks([{ id: "x", name: "X", bank: "user" }], banks)).toEqual([{ id: "user", name: "user" }]);
    expect(bankName(banks, "keys")).toBe("Electric Keys");
    expect(bankName(banks, undefined)).toBeUndefined();
  });

  it("steps and wraps", () => {
    expect(stepProgram(programs, "a", 1)).toBe("b");
    expect(stepProgram(programs, "a", -1)).toBe("e");
    expect(stepProgram(programs, "e", 1)).toBe("a");
    expect(stepProgram(programs, "missing", 1)).toBe("a");
    expect(stepProgram(programs, null, -1)).toBe("e");
    expect(stepProgram([], "a", 1)).toBeNull();
  });
});
