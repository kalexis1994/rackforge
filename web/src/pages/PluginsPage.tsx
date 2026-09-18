import { useEffect, useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router";
import { AsyncActionLabel } from "../components/AsyncSpinner";
import { AsyncNotice, AsyncStateBoundary } from "../components/AsyncStateBoundary";
import { EmptyState } from "../components/EmptyState";
import { PageHeading } from "../components/PageHeading";
import { PluginIcon } from "../components/PluginIcon";
import { PluginRuntimeStatus } from "../components/PluginRuntimeStatus";
import { RfLoader } from "../components/RfLoader";
import { PluginRemovalDialog } from "../dialogs/PluginRemovalDialog";
import { dispatchCommandAwait } from "../gateway";
import { hostJson } from "../host";
import { ControllerSummary } from "../pages/ControllerPage";
import { commitPlayPluginSelection, preflightPlayPluginSelection } from "../playPluginSelection";
import { beginPluginOperation, canOpenInPlay, groupPluginsByKind, invalidatePluginCatalog, usePluginCatalog } from "../pluginCatalog";
import { setInstalledPluginActive, synchronizePluginEnvironment } from "../pluginLifecycle";
import { formatPluginVersion, pluginKindPresentation } from "../pluginPresentation";
import { PluginRemovalOptions, PluginRemovalResult, pluginRemovalSummary } from "../pluginRemoval";
import { type PluginWebDescriptor, type SessionSnapshot } from "../types";
import { RfButton } from "../ui/RfButton";
import { Download,  Sliders } from "lucide-react";

export function PluginsPage({
  snapshot,
  onInstall,
  showControllers = true,
}: {
  snapshot: SessionSnapshot | null;
  onInstall: () => void;
  showControllers?: boolean;
}) {
  const location = useLocation();
  const navigate = useNavigate();
  const pluginCatalog = usePluginCatalog();
  const { plugins: installed } = pluginCatalog;
  const [controllers, setControllers] = useState<ControllerSummary[]>([]);
  const [controllersStatus, setControllersStatus] = useState<"loading" | "ready" | "error">(
    showControllers ? "loading" : "ready",
  );
  const [controllerRefreshRevision, setControllerRefreshRevision] = useState(0);
  useEffect(() => {
    if (!showControllers) return;
    let cancelled = false;
    hostJson<{ controllers: ControllerSummary[] }>("/api/v1/controllers")
      .then((response) => {
        if (!cancelled) {
          setControllers(response.controllers ?? []);
          setControllersStatus("ready");
        }
      })
      .catch(() => {
        if (!cancelled) setControllersStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [controllerRefreshRevision, showControllers]);
  const [pendingRemoval, setPendingRemoval] = useState<PluginWebDescriptor | null>(null);
  const [removing, setRemoving] = useState(false);
  const [changingPluginId, setChangingPluginId] = useState<string | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const [removalError, setRemovalError] = useState<string | null>(null);
  const [removalMessage, setRemovalMessage] = useState<string | null>(() => {
    const state = location.state as { pluginRemovalMessage?: unknown } | null;
    return typeof state?.pluginRemovalMessage === "string"
      ? state.pluginRemovalMessage
      : null;
  });

  const removePlugin = async (options: PluginRemovalOptions) => {
    if (!pendingRemoval) return;
    const finishOperation = beginPluginOperation(
      pendingRemoval.plugin_id,
      "remove",
      `Removing ${pendingRemoval.plugin_name}…`,
    );
    setRemoving(true);
    setRemovalError(null);
    setRemovalMessage(null);
    try {
      const result = await hostJson<PluginRemovalResult>(
        `/api/v1/plugins/${encodeURIComponent(pendingRemoval.plugin_id)}`,
        {
          method: "DELETE",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(options),
        },
      );
      setPendingRemoval(null);
      await synchronizePluginEnvironment();
      setRemovalMessage(pluginRemovalSummary(result));
    } catch (error) {
      setRemovalError(
        error instanceof Error ? error.message : "Could not remove the plugin.",
      );
    } finally {
      finishOperation();
      setRemoving(false);
    }
  };

  const running = snapshot?.instances ?? [];
  const requestRemoval = (plugin: PluginWebDescriptor) => {
    setRemovalError(null);
    setPendingRemoval(plugin);
  };
  const changeActivation = async (plugin: PluginWebDescriptor) => {
    if (!plugin.managed && plugin.active) return;
    setChangingPluginId(plugin.plugin_id);
    setActivationError(null);
    setRemovalMessage(null);
    try {
      await setInstalledPluginActive(plugin.plugin_id, !plugin.active);
      setRemovalMessage(
        plugin.active
          ? `${plugin.plugin_name} is now inactive.`
          : `${plugin.plugin_name} is active and ready to use.`,
      );
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not change plugin activation.",
      );
    } finally {
      setChangingPluginId(null);
    }
  };
  const openInPlay = async (plugin: PluginWebDescriptor) => {
    // PLAY is one instrument. The card hides this action for anything else,
    // but the refusal belongs here too: the button is presentation, and what
    // follows moves the host into PLAY mode around whatever it is handed.
    if (!plugin.active || !canOpenInPlay(plugin)) return;
    const finishOperation = beginPluginOperation(
      plugin.plugin_id,
      "open",
      `Opening ${plugin.plugin_name} in PLAY…`,
    );
    setChangingPluginId(plugin.plugin_id);
    setActivationError(null);
    try {
      const instance = running.find(
        (candidate) => candidate.plugin_id === plugin.plugin_id,
      );
      const request = {
        target: {
          pluginId: plugin.plugin_id,
          pluginName: plugin.plugin_name,
          instanceId: instance?.instance_id,
        },
        activeInstanceId: snapshot?.active_instance_id,
      };
      if (preflightPlayPluginSelection(request).status !== "already_active") {
        await commitPlayPluginSelection(request, {
          dispatch: dispatchCommandAwait,
          activate: (pluginId) =>
            hostJson(`/api/v1/plugins/${encodeURIComponent(pluginId)}/activate`, {
              method: "POST",
            }),
          synchronize: synchronizePluginEnvironment,
        });
      }
      navigate("/play");
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not open the plugin in PLAY.",
      );
    } finally {
      finishOperation();
      setChangingPluginId(null);
    }
  };

  return (
    <>
      <div className="plugin-manager-heading">
        <PageHeading
          eyebrow="Plugin library"
          title="Plugin Manager"
          detail={showControllers
            ? "Install, configure and remove RackForge plugins: instruments and controllers. Musical controls remain in Play."
            : "Choose and manage the instruments available to this RackForge VST3 instance."}
        />
        <RfButton variant="primary" className="plugin-install-button" onClick={onInstall}>
          <Download aria-hidden="true" />
          Install plugin
        </RfButton>
      </div>
      <div className="plugin-section-heading">
        <span className="card-kicker">Audio plugins</span>
        <small>Installation and runtime activation are managed separately</small>
      </div>
      <div className="rf-floating-notice-stack">
        {removalMessage ? (
          <AsyncNotice
            tone="success"
            title="Plugin library updated"
            onDismiss={() => setRemovalMessage(null)}
          >
            {removalMessage}
          </AsyncNotice>
        ) : null}
        {activationError ? (
          <AsyncNotice
            tone="error"
            title="Plugin operation failed"
            onDismiss={() => setActivationError(null)}
          >
            {activationError}
          </AsyncNotice>
        ) : null}
        {controllersStatus === "error" ? (
          <AsyncNotice tone="error" title="Controller packages unavailable">
            Could not read the installed hardware profiles.
            <RfButton
              size="compact"
              haptic="none"
              onClick={() => {
                setControllersStatus("loading");
                setControllerRefreshRevision((current) => current + 1);
              }}
            >
              Retry
            </RfButton>
          </AsyncNotice>
        ) : null}
      </div>
      <AsyncStateBoundary
        className="plugin-manager-boundary"
        status={pluginCatalog.status}
        hasContent={installed.length > 0}
        loadingLabel="Loading Plugin Manager"
        loadingDetail="Discovering packages and checking the audio runtime…"
        errorTitle="Plugin library unavailable"
        errorDetail={pluginCatalog.error ?? "RackForge could not load installed plugins."}
        onRetry={() => void invalidatePluginCatalog()}
        loaderSize="large"
      >
        {groupPluginsByKind(installed).map((group) => (
          <section className="plugin-kind-group" key={group.kind}>
            <div className="plugin-section-heading">
              <span className="card-kicker">
                {pluginKindPresentation(group.kind).plural}
              </span>
            </div>
            <div className="plugin-grid expanded plugin-manager-grid">
        {group.plugins.map((plugin, index) => {
          const instance = running.find((candidate) => candidate.plugin_id === plugin.plugin_id);
          const busy = changingPluginId === plugin.plugin_id;
          const configAvailable = plugin.surfaces.some((surface) => surface.kind === "config");
          const kind = pluginKindPresentation(plugin.kind);
          return (
            <article
              className={`plugin-card installed-plugin-card plugin-manager-card${plugin.active ? "" : " inactive"}`}
              key={plugin.plugin_id}
            >
              <div className={`plugin-tile tile-${index % 4}${plugin.branding ? " branded" : ""}`}>
                <PluginIcon plugin={plugin} name={plugin.plugin_name} />
                {!plugin.branding && <i />}
              </div>
              <div className="plugin-manager-card-copy">
                <span className="card-kicker">
                  {plugin.active ? "Active" : "Inactive"}{" "}
                  <span className={`plugin-kind-tag ${kind.className}`}>{kind.label}</span>
                </span>
                <h3>{plugin.plugin_name}{formatPluginVersion(plugin.version)}</h3>
                <PluginRuntimeStatus status={pluginCatalog.runtime[plugin.plugin_id]} />
                <p>{plugin.surfaces.length === 0 ? "No Web interface" : "Web interface ready"}</p>
              </div>
              <div className="plugin-manager-card-actions" aria-label={`${plugin.plugin_name} actions`}>
                <RfButton
                  variant={plugin.active ? "secondary" : "primary"}
                  disabled={busy || (!plugin.managed && plugin.active)}
                  onClick={() => void changeActivation(plugin)}
                >
                  <AsyncActionLabel
                    active={busy}
                    activeLabel={plugin.active ? "Deactivating…" : "Activating…"}
                  >
                    {plugin.active
                      ? plugin.managed ? "Deactivate" : "Built-in active"
                      : "Activate"}
                  </AsyncActionLabel>
                </RfButton>
                {canOpenInPlay(plugin) ? (
                  <RfButton
                    variant="secondary"
                    disabled={!plugin.active || busy}
                    onClick={() => void openInPlay(plugin)}
                  >
                    Go to PLAY
                  </RfButton>
                ) : null}
                <RfButton
                  variant="secondary"
                  disabled={!plugin.active || !configAvailable || !instance || busy}
                  onClick={() => navigate(`/plugins/${encodeURIComponent(instance!.instance_id)}`)}
                >
                  Config
                </RfButton>
                <RfButton
                  variant="danger"
                  className="plugin-manager-remove"
                  disabled={!plugin.managed || busy}
                  onClick={() => requestRemoval(plugin)}
                >
                  Remove
                </RfButton>
              </div>
            </article>
          );
        })}
            </div>
          </section>
        ))}
      {installed.length === 0 ? (
        <EmptyState title="No plugins installed" />
      ) : null}
        </AsyncStateBoundary>
      {controllersStatus === "loading" ? (
        <RfLoader
          className="plugin-controller-loader"
          label="Loading controllers"
          detail="Discovering installed hardware profiles…"
          size="compact"
        />
      ) : null}
      {controllers.length > 0 && (
        <>
          <div className="plugin-section-heading">
            <span className="card-kicker">Controllers</span>
            <small>Hardware surfaces installed as packages</small>
          </div>
          <div className="plugin-grid expanded">
            {controllers.map((controller) => (
              <NavLink
                className="plugin-card installed-plugin-card"
                key={controller.id}
                to={`/controllers/${encodeURIComponent(controller.id)}`}
              >
                <div className="plugin-tile controller-tile">
                  <span className="controller-tile-mark" aria-hidden="true">
                    <Sliders aria-hidden="true" />
                  </span>
                </div>
                <div>
                  <span className="card-kicker">
                    {controller.enabled ? "Active package" : "Disabled package"}{" "}
                    <span className="plugin-kind-tag controller">Controller</span>
                  </span>
                  <h3>{controller.name}</h3>
                  <p>
                    Version {controller.version} · {controller.trust}
                    {controller.devices > 0 ? ` · ${controller.devices} device profile(s)` : ""}
                    {` · ${controller.runtime}`}
                  </p>
                </div>
              </NavLink>
            ))}
          </div>
        </>
      )}
      {pendingRemoval ? (
        <PluginRemovalDialog
          pluginName={pendingRemoval.plugin_name}
          active={pendingRemoval.active}
          removing={removing}
          error={removalError}
          onClose={() => setPendingRemoval(null)}
          onConfirm={removePlugin}
        />
      ) : null}
    </>
  );
}
