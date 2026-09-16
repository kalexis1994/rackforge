import { afterEach, expect, it, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

it("retries registration after a transient failure", async () => {
  vi.resetModules();
  const registration = { active: {} };
  const register = vi.fn().mockRejectedValueOnce(new Error("temporarily unavailable"))
    .mockResolvedValueOnce(registration);
  vi.stubGlobal("navigator", { serviceWorker: { register } });
  vi.spyOn(console, "warn").mockImplementation(() => {});
  const { ensureServiceWorker } = await import("./pwa");
  await expect(ensureServiceWorker()).resolves.toBeNull();
  await expect(ensureServiceWorker()).resolves.toBe(registration);
  expect(register).toHaveBeenCalledTimes(2);
});
