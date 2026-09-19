import { useParams } from "react-router";
import { PluginConfigSurface } from "../components/PluginFrame";
import { PluginSurfaceState } from "../components/PluginSurfaceState";
import { RfLoader } from "../components/RfLoader";
import { usePluginCatalog } from "../pluginCatalog";
import { type ConnectionStatus, type SessionSnapshot } from "../types";

export function PluginPage({
  snapshot,
  connection,
}: {
  snapshot: SessionSnapshot | null;
  connection: ConnectionStatus;
}) {
  const { instanceId } = useParams();
  const { plugins, status: catalogStatus, error: catalogError } = usePluginCatalog();
  const instance = snapshot?.instances.find(
    (item) => item.instance_id === decodeURIComponent(instanceId ?? ""),
  );
  if (
    !instance &&
    (connection === "connecting" || catalogStatus === "idle" || catalogStatus === "loading")
  ) {
    return (
      <section className="plugin-surface-shell direct-surface plugin-surface-loading">
        <RfLoader
          label="Opening plugin configuration"
          detail="Loading the plugin catalog and runtime instance…"
          size="large"
        />
      </section>
    );
  }
  if (!instance)
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin not found"
          detail="This plugin instance is no longer available in the current session."
        />
      </section>
    );
  if (catalogStatus === "idle" || catalogStatus === "loading") {
    return (
      <section className="plugin-surface-shell direct-surface plugin-surface-loading">
        <RfLoader
          label="Checking plugin activation"
          detail="RackForge is verifying the plugin before opening Config…"
          size="large"
        />
      </section>
    );
  }
  if (catalogStatus === "error") {
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin library unavailable"
          detail={catalogError ?? "RackForge could not verify the plugin catalog."}
        />
      </section>
    );
  }
  const descriptor = plugins.find((plugin) => plugin.plugin_id === instance.plugin_id);
  if (!descriptor?.active) {
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin inactive"
          detail="Activate this plugin from Plugin Manager before opening its configuration."
        />
      </section>
    );
  }
  return <PluginConfigSurface instance={instance} />;
}
