import { AsyncSpinner } from "./AsyncSpinner";
import type { PluginRuntimeStatus as RuntimeStatus } from "../pluginCatalog";

const FALLBACK_STATUS: RuntimeStatus = {
  plugin_id: "",
  phase: "loading",
  loaded: false,
  healthy: null,
  detail: "Checking runtime…",
};

/**
 * A plugin's runtime state as a lamp and a line.
 *
 * `problemsOnly` keeps it quiet unless the runtime is unhealthy --
 * disconnected, idle, or its instance gone. The Plugin Manager is where a
 * plugin's state is read, so it shows every state; the PLAY selector is for
 * picking an instrument, and a line on every entry saying it is fine only
 * hides the one that is not.
 */
export function PluginRuntimeStatus({
  status,
  className = "",
  problemsOnly = false,
}: {
  status?: RuntimeStatus | null;
  className?: string;
  problemsOnly?: boolean;
}) {
  if (problemsOnly && status?.phase !== "unhealthy") return null;
  const current = status ?? FALLBACK_STATUS;
  return (
    <span
      className={["plugin-runtime-status", `is-${current.phase}`, className]
        .filter(Boolean)
        .join(" ")}
      title={current.detail}
      data-loaded={current.loaded}
      data-healthy={current.healthy ?? "unknown"}
    >
      {current.phase === "loading" ? (
        <AsyncSpinner label={current.detail} />
      ) : (
        <i aria-hidden="true" />
      )}
      <span>{current.detail}</span>
    </span>
  );
}
