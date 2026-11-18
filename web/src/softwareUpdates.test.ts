import { describe, expect, it } from "vitest";
import { updateBusy, updateDescription, type SoftwareUpdateState } from "./softwareUpdates";

const state: SoftwareUpdateState = {
  current_version: "0.1.39", build: "host-build", platform: "linux-aarch64-services", phase: "idle",
  available: null, message: null, downloaded_bytes: 0, can_stage: true,
};
describe("signed software update feedback", () => {
  it("reports the offered host version, independently of the interface version", () => {
    expect(updateDescription({ ...state, phase: "available", available: { version: "0.1.40", notes: "", download_bytes: 42 } })).toContain("0.1.40");
  });
  it("shows signature failures instead of claiming the software is up to date", () => {
    const failed = { ...state, phase: "failed" as const, message: "Update signature verification failed" };
    expect(updateDescription(failed)).toBe(failed.message);
    expect(updateBusy(failed)).toBe(false);
  });
  it("keeps checking and downloading busy, while a verified package is ready", () => {
    expect(updateBusy({ ...state, phase: "checking" })).toBe(true);
    expect(updateBusy({ ...state, phase: "downloading" })).toBe(true);
    expect(updateBusy({ ...state, phase: "ready" })).toBe(false);
    expect(updateDescription({ ...state, phase: "ready" })).toContain("verified");
    expect(updateDescription({ ...state, phase: "ready" })).not.toContain("installed");
  });
  it("distinguishes an unpublished feed from a verified up-to-date release", () => {
    expect(updateDescription({ ...state, phase: "not_published" })).toContain("No signed RackForge update");
    expect(updateDescription({ ...state, phase: "not_published" })).not.toContain("up to date");
  });
  it("keeps installation and recovery busy until services have been checked", () => {
    for (const phase of ["installing", "recovering"] as const) expect(updateBusy({ ...state, phase })).toBe(true);
    expect(updateDescription({ ...state, phase: "installing" })).toContain("Audio will stop");
    expect(updateDescription({ ...state, phase: "recovering" })).toContain("previous installation");
    expect(updateBusy({ ...state, phase: "installed" })).toBe(false);
  });
  it("distinguishes successful activation, rollback and a failed recovery", () => {
    expect(updateDescription({ ...state, phase: "installed" })).toContain("installed and checked");
    expect(updateDescription({ ...state, phase: "rolled_back" })).toContain("previous RackForge");
    expect(updateDescription({ ...state, phase: "recovery_failed", message: "Audio hardware unavailable" })).toBe("Audio hardware unavailable");
  });
});
