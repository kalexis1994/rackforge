import { AsyncSpinner } from "./AsyncSpinner";
import type { PluginRuntimeStatus as RuntimeStatus } from "../pluginCatalog";

const FALLBACK_STATUS: RuntimeStatus = {
  plugin_id: "",
  phase: "loading",
  loaded: false,
  healthy: null,
  detail: "Checking runtime…",
};

export function PluginRuntimeStatus({
  status,
  className = "",
}: {
  status?: RuntimeStatus | null;
  className?: string;
}) {
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
