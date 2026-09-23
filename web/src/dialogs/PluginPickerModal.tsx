import { type CSSProperties, useState } from "react";
import { AsyncActionLabel } from "../components/AsyncSpinner";
import { AsyncNotice, AsyncStateBoundary } from "../components/AsyncStateBoundary";
import { ModalDialog } from "../components/ModalDialog";
import { PluginIcon } from "../components/PluginIcon";
import { PluginRuntimeStatus } from "../components/PluginRuntimeStatus";
import { PluginSurfaceState } from "../components/PluginSurfaceState";
import { dispatchCommandAwait } from "../gateway";
import { hostJson } from "../host";
import { commitPlayPluginSelection, preflightPlayPluginSelection } from "../playPluginSelection";
import { beginPluginOperation, invalidatePluginCatalog, usePluginCatalog } from "../pluginCatalog";
import { synchronizePluginEnvironment } from "../pluginLifecycle";
import { formatPluginVersion } from "../pluginPresentation";
import { type PluginInstance, type PluginWebDescriptor, type SessionSnapshot } from "../types";
import { RfButton } from "../ui/RfButton";

export function PluginPickerModal({
  active,
  instances,
  plugins,
  programDraft,
  onClose,
}: {
  active: PluginInstance | undefined;
  instances: PluginInstance[];
  plugins: PluginWebDescriptor[];
  programDraft: SessionSnapshot["program_draft"];
  onClose: () => void;
}) {
  const { status: catalogStatus, error: catalogError, runtime } = usePluginCatalog();
  const [activatingId, setActivatingId] = useState<string | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const [pendingPlugin, setPendingPlugin] = useState<PluginWebDescriptor | null>(null);
  const [pendingActivation, setPendingActivation] = useState<PluginWebDescriptor | null>(null);
  const activePluginId = active?.plugin_id;
  // PLAY plays instruments. An effect belongs after one, in the FX drawer,
  // and a MIDI processor in a Rack; neither can be the instance on stage,
  // so neither is offered here.
  const instruments = plugins.filter((plugin) => plugin.kind === "instrument");
  const orderedPlugins = [
    ...instruments.filter((plugin) => plugin.plugin_id === activePluginId),
    ...instruments.filter((plugin) => plugin.plugin_id !== activePluginId),
  ];
  const activate = async (
    plugin: PluginWebDescriptor,
    { discardDraft = false }: { discardDraft?: boolean } = {},
  ) => {
    const instance = instances.find(
      (candidate) => candidate.plugin_id === plugin.plugin_id,
    );
    const request = {
      target: {
        pluginId: plugin.plugin_id,
        pluginName: plugin.plugin_name,
        instanceId: instance?.instance_id,
      },
      activeInstanceId: active?.instance_id,
      programDraft: programDraft
        ? { draftId: programDraft.draft_id, dirty: programDraft.dirty }
        : undefined,
      discardDraft,
    };
    setActivationError(null);
    const preflight = preflightPlayPluginSelection(request);
    if (preflight.status === "already_active") {
      onClose();
      return;
    }
    if (preflight.status === "confirmation_required") {
      setPendingPlugin(plugin);
      return;
    }
    setPendingPlugin(null);
    setActivatingId(plugin.plugin_id);
    const finishOperation = beginPluginOperation(
      plugin.plugin_id,
      "open",
      `Opening ${plugin.plugin_name} in PLAY…`,
    );
    try {
      await commitPlayPluginSelection(request, {
        dispatch: dispatchCommandAwait,
        activate: (pluginId) =>
          hostJson(`/api/v1/plugins/${encodeURIComponent(pluginId)}/activate`, {
            method: "POST",
          }),
        synchronize: synchronizePluginEnvironment,
      });
      onClose();
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not activate the plugin.",
      );
    } finally {
      finishOperation();
      setActivatingId(null);
    }
  };
  const requestActivation = (plugin: PluginWebDescriptor) => {
    if (!plugin.active) {
      setActivationError(null);
      setPendingActivation(plugin);
      return;
    }
    void activate(plugin);
  };
  return (
    <>
      <ModalDialog
        eyebrow="PLAY · Instruments"
        title="Select plugin"
        onClose={onClose}
        dismissible={activatingId === null && pendingActivation === null}
        closeLabel="Close plugin selector"
        className="plugin-picker-modal"
      >
        {pendingPlugin && programDraft && (
          <section className="plugin-switch-confirm" role="alert">
            <div>
              <strong>
                {programDraft.dirty
                  ? "Discard unsaved program changes?"
                  : "Close the current program editor?"}
              </strong>
              <p>
                RackForge must close the active edit session before switching to {" "}
                {pendingPlugin.plugin_name}.
              </p>
            </div>
            <div className="plugin-switch-confirm-actions">
              <button type="button" onClick={() => setPendingPlugin(null)}>
                Keep editing
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => void activate(pendingPlugin, { discardDraft: true })}
              >
                <AsyncActionLabel
                  active={activatingId === pendingPlugin.plugin_id}
                  activeLabel="Switching…"
                >
                  Discard and switch
                </AsyncActionLabel>
              </button>
            </div>
          </section>
        )}
        {activationError ? (
          <AsyncNotice tone="error" title="Could not open the plugin">
            {activationError}
          </AsyncNotice>
        ) : null}
        <AsyncStateBoundary
          className="plugin-picker-boundary"
          status={catalogStatus}
          hasContent={plugins.length > 0}
          loadingLabel="Loading plugins"
          loadingDetail="Discovering installed instruments and checking their runtimes…"
          errorTitle="Plugin library unavailable"
          errorDetail={catalogError ?? "RackForge could not load the plugin catalog."}
          onRetry={() => void invalidatePluginCatalog()}
        >
          <div className="play-plugin-selector modal-list" role="list" aria-label="Playable plugins">
            {orderedPlugins.map((plugin, index) => {
              const selected = plugin.plugin_id === activePluginId;
              const activating = activatingId === plugin.plugin_id;
              return (
                <button
                  className={`plugin-picker-card${selected ? " active" : ""}${!plugin.active ? " inactive" : ""}${plugin.branding ? " branded" : ""}`}
                  disabled={activatingId !== null}
                  key={plugin.plugin_id}
                  onClick={() => requestActivation(plugin)}
                  aria-disabled={!plugin.active}
                  style={plugin.branding ? {
                    "--plugin-accent": plugin.branding.accent_color,
                    "--plugin-background": plugin.branding.background_color,
                  } as CSSProperties : undefined}
                >
                  {plugin.branding && (
                    <>
                      <img className="plugin-picker-banner" src={plugin.branding.banner_url} alt="" />
                      <span className="plugin-picker-shade" aria-hidden="true" />
                    </>
                  )}
                  <span className="play-plugin-number">{String(index + 1).padStart(2, "0")}</span>
                  <PluginIcon plugin={plugin} name={plugin.plugin_name} className="plugin-picker-icon" />
                  <span className="play-plugin-copy">
                    <strong>{plugin.plugin_name}{formatPluginVersion(plugin.version)}</strong>
                    <PluginRuntimeStatus status={runtime[plugin.plugin_id]} />
                  </span>
                  <span className="play-plugin-status">
                    {activating ? (
                      <AsyncActionLabel active activeLabel="Loading…">SELECT</AsyncActionLabel>
                    ) : (
                      <>{selected ? "PLAYING" : plugin.active ? "SELECT" : "INACTIVE"}<i aria-hidden="true">→</i></>
                    )}
                  </span>
                </button>
              );
            })}
            {orderedPlugins.length === 0 ? (
              <PluginSurfaceState
                title="No plugins installed"
                detail="Install an .rfplugin package from the Plugins section."
              />
            ) : null}
          </div>
        </AsyncStateBoundary>
      </ModalDialog>
      {pendingActivation ? (
        <ModalDialog
          eyebrow="Plugin activation"
          title={`Activate ${pendingActivation.plugin_name}?`}
          onClose={() => setPendingActivation(null)}
          dismissible={activatingId === null}
          closeLabel="Cancel plugin activation"
          className="plugin-activation-dialog"
          actions={
            <>
              <RfButton
                variant="secondary"
                onClick={() => setPendingActivation(null)}
                disabled={activatingId !== null}
              >
                Cancel
              </RfButton>
              <RfButton
                variant="primary"
                onClick={() => {
                  const plugin = pendingActivation;
                  setPendingActivation(null);
                  void activate(plugin);
                }}
                disabled={activatingId !== null}
              >
                <AsyncActionLabel
                  active={activatingId === pendingActivation.plugin_id}
                  activeLabel="Activating…"
                >
                  Activate plugin
                </AsyncActionLabel>
              </RfButton>
            </>
          }
        >
          <div className="plugin-activation-copy">
            <p>This plugin is installed but inactive.</p>
            <p>RackForge must activate it before it can be used in PLAY.</p>
          </div>
        </ModalDialog>
      ) : null}
    </>
  );
}
