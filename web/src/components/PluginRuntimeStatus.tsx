import type { PluginRuntimeStatus as RuntimeStatus } from "../pluginCatalog";

/**
 * A plugin's runtime, said out loud only when something is wrong with it.
 *
 * Every card used to carry a line for every state -- "Loaded and healthy",
 * "Active · Loads on demand", "Inactive", a spinner while checking -- and a
 * list in which every entry says it is fine teaches the eye to skip the one
 * that is not. The cards already show whether a plugin is active and what it
 * is doing (the kicker, the activation key, the PLAYING / SELECT column), so
 * this renders nothing unless the runtime is unhealthy: disconnected, idle,
 * or an instance that has gone missing.
 */
export function PluginRuntimeStatus({
  status,
  className = "",
}: {
  status?: RuntimeStatus | null;
  className?: string;
}) {
  if (status?.phase !== "unhealthy") return null;
  return (
    <span
      className={["plugin-runtime-status", "is-unhealthy", className].filter(Boolean).join(" ")}
      title={status.detail}
      role="status"
    >
      <i aria-hidden="true" />
      <span>{status.detail}</span>
    </span>
  );
}
