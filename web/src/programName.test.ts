import { describe, expect, it } from "vitest";
import { validPluginProgramName } from "./programName";

describe("plugin program names", () => {
  it("accepts visible Unicode names", () => {
    expect(validPluginProgramName("Cálido Personal")).toBe(true);
    expect(validPluginProgramName("夜のステージ")).toBe(true);
  });

  it("rejects empty, oversized, control and formatting characters", () => {
    expect(validPluginProgramName("   ")).toBe(false);
    expect(validPluginProgramName("x".repeat(65))).toBe(false);
    expect(validPluginProgramName("line\nbreak")).toBe(false);
    expect(validPluginProgramName("zero\u200bwidth")).toBe(false);
  });
});
