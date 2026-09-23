import { type CSSProperties, Suspense, useCallback, useRef, useState } from "react";
import { useNavigate } from "react-router";
import { AsyncActionLabel } from "../components/AsyncSpinner";
import { BrandMark } from "../components/BrandMark";
import { ModalDialog } from "../components/ModalDialog";
import { RfLoader } from "../components/RfLoader";
import { ResourceExplorerDialog } from "../dialogs/lazyResourceExplorer";
import { requestSessionSnapshot } from "../gateway";
import { hostHaptic, hostJson, isDesktopHost, isNativeHost, isRemoteWebClient, selectNativeResource } from "../host";
import { beginPluginOperation, invalidatePluginCatalog } from "../pluginCatalog";
import { InstalledPluginResult, activateInstalledPlugin } from "../pluginLifecycle";
import { MAX_CLIENT_RESOURCE_BYTES, postResourceApi } from "../resourceApi";
import { type PluginWebDescriptor, type ResourceEntry, type ResourceSelection } from "../types";
import { FileUp, FolderOpen } from "lucide-react";

export interface PluginInstallPreview {
  selection_id: string;
  plugin_id: string;
  plugin_name: string;
  vendor: string;
  version: string;
  description?: string | null;
  kind: string;
  platform: string;
  portable: boolean;
  archive_bytes: number;
  branding?: {
    banner_data_url: string;
    background_color?: string | null;
    accent_color?: string | null;
  } | null;
}

/**
 * What a package says about itself, before anything is installed.
 *
 * A branded package shows its own banner and accent; one without branding
 * gets the RackForge mark on the panel, not a monogram typed in a font.
 */
export function PluginInstallPreviewCard({ preview }: { preview: PluginInstallPreview }) {
  const description = preview.description?.trim()
    || `${preview.kind === "instrument" ? "Instrument" : "Plugin"} by ${preview.vendor}, packaged for RackForge.`;
  // Only a package's own branding recolours the card; everything else is the
  // faceplate's, so the card follows the lighting like the rest of the page.
  const style = preview.branding?.accent_color
    ? ({ "--preview-accent": preview.branding.accent_color } as CSSProperties)
    : undefined;
  const packageSize = preview.archive_bytes >= 1024 * 1024
    ? `${(preview.archive_bytes / (1024 * 1024)).toFixed(1)} MB`
    : `${Math.max(1, Math.round(preview.archive_bytes / 1024))} KB`;
  return (
    <div className="plugin-install-preview" style={style}>
      <div className={`plugin-install-preview-banner${preview.branding ? " branded" : ""}`}>
        {preview.branding ? (
          <img src={preview.branding.banner_data_url} alt={`${preview.plugin_name} banner`} />
        ) : (
          <BrandMark />
        )}
      </div>
      <div className="plugin-install-preview-copy">
        <span className="eyebrow">READY TO INSTALL</span>
        <h3>{preview.plugin_name} <small>v{preview.version}</small></h3>
        <p>{description}</p>
        <div className="plugin-install-preview-meta" aria-label="Package details">
          <span>{preview.vendor}</span>
          <span>{preview.kind}</span>
          <span>{preview.portable ? "Portable" : preview.platform}</span>
          <span>{packageSize}</span>
        </div>
      </div>
    </div>
  );
}

export function InstallPluginDialog({ onClose }: { onClose: () => void }) {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const native = isNativeHost();
  const desktop = isDesktopHost();
  // Browsing the host's own storage needs a host with storage to browse. A
  // page carrying its own RackForge has none, so it offers upload only.
  const remoteWeb = isRemoteWebClient();
  const [browseHost, setBrowseHost] = useState(false);
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const cancellationRequestedRef = useRef(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<PluginInstallPreview | null>(null);
  const [installed, setInstalled] = useState<InstalledPluginResult | null>(null);
  const [installedDescriptor, setInstalledDescriptor] = useState<PluginWebDescriptor | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const navigate = useNavigate();

  const releaseSelection = useCallback(async (selectionId: string) => {
    await postResourceApi("/api/v1/resources/selections/release", {
      selection_id: selectionId,
    });
  }, []);

  const closeDialog = useCallback(() => {
    if (preview && !installed) {
      void releaseSelection(preview.selection_id).catch(() => undefined);
    }
    onClose();
  }, [installed, onClose, preview, releaseSelection]);

  const inspectSelection = async (selection: ResourceSelection) => {
    setStatus(`Validating ${selection.display_name}…`);
    try {
      const inspection = await postResourceApi<PluginInstallPreview>(
        "/api/v1/plugins/inspect",
        { selection_id: selection.selection_id },
      );
      setPreview(inspection);
      setStatus(null);
    } catch (reason) {
      await releaseSelection(selection.selection_id).catch(() => undefined);
      throw reason;
    }
  };

  const finishInstall = async (result: InstalledPluginResult) => {
    setInstalled(result);
    setStatus("Refreshing the plugin library…");
    try {
      await invalidatePluginCatalog();
      const descriptor = await hostJson<PluginWebDescriptor>(
        `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
      );
      setInstalledDescriptor(descriptor);
      hostHaptic("confirm");
    } catch (reason) {
      setError(
        `The package is installed, but RackForge could not refresh its actions: ${
          reason instanceof Error ? reason.message : "unknown catalog error"
        }`,
      );
    } finally {
      setStatus(null);
    }
  };

  const installPreview = async () => {
    if (!preview) return;
    const selectionId = preview.selection_id;
    const finishOperation = beginPluginOperation(
      preview.plugin_id,
      "install",
      `Installing ${preview.plugin_name}…`,
    );
    cancellationRequestedRef.current = false;
    setCancelled(false);
    setCancelling(false);
    setBusy(true);
    setError(null);
    setStatus(`Installing ${preview.plugin_name}…`);
    try {
      const result = await postResourceApi<InstalledPluginResult>("/api/v1/plugins/install", {
        selection_id: selectionId,
      });
      setPreview(null);
      await finishInstall(result);
    } catch (reason) {
      setPreview(null);
      setStatus(null);
      if (
        cancellationRequestedRef.current ||
        (reason instanceof Error && reason.message.toLowerCase().includes("cancel"))
      ) {
        setCancelled(true);
        setError(null);
        void invalidatePluginCatalog().catch(() => undefined);
      } else {
        setError(reason instanceof Error ? reason.message : "Could not install this plugin.");
      }
    } finally {
      finishOperation();
      setBusy(false);
      setCancelling(false);
    }
  };

  const cancelInstallation = async () => {
    if (!preview || !busy || cancelling) return;
    cancellationRequestedRef.current = true;
    setCancelling(true);
    setStatus("Cancelling installation safely…");
    try {
      await postResourceApi("/api/v1/plugins/install/cancel", {
        selection_id: preview.selection_id,
      });
    } catch (reason) {
      cancellationRequestedRef.current = false;
      setCancelling(false);
      setStatus(`Installing ${preview.plugin_name}…`);
      setError(
        reason instanceof Error
          ? reason.message
          : "RackForge could not cancel the installation.",
      );
    }
  };

  const openInstalledPlugin = async (destination: "play" | "config") => {
    if (!installed) return;
    setBusy(true);
    setError(null);
    setStatus(
      destination === "play"
        ? `Opening ${installed.plugin_id} in PLAY…`
        : `Opening ${installed.plugin_id} configuration…`,
    );
    try {
      await activateInstalledPlugin(installed);
      const refreshed = await requestSessionSnapshot();
      const instance = refreshed.instances.find(
        (candidate) => candidate.plugin_id === installed.plugin_id,
      );
      if (destination === "config" && !instance) {
        throw new Error("RackForge activated the plugin but did not publish its configuration instance.");
      }
      navigate(
        destination === "play"
          ? "/play"
          : `/plugins/${encodeURIComponent(instance!.instance_id)}`,
      );
      onClose();
    } catch (reason) {
      setError(
        reason instanceof Error ? reason.message : "Could not open the installed plugin.",
      );
      setStatus(null);
    } finally {
      setBusy(false);
    }
  };

  const cancelPreview = () => {
    if (!preview || busy) return;
    const selectionId = preview.selection_id;
    setPreview(null);
    void releaseSelection(selectionId).catch(() => undefined);
    onClose();
  };

  const openNativePicker = async () => {
    setBusy(true);
    setError(null);
    setStatus("Opening file picker…");
    try {
      const selection = await selectNativeResource({
        kind: "file",
        extensions: [".rfplugin"],
      });
      await inspectSelection(selection);
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not open the file picker.",
      );
    } finally {
      setBusy(false);
    }
  };

  const uploadClientFile = async (file: File) => {
    if (file.size === 0 || file.size > MAX_CLIENT_RESOURCE_BYTES) {
      setError("The package is empty or exceeds RackForge's 512 MB limit.");
      return;
    }
    setBusy(true);
    setError(null);
    setInstalled(null);
    setStatus(`Uploading ${file.name} to RackForge…`);
    try {
      const selection = await hostJson<ResourceSelection>(
        `/api/v1/resources/uploads?name=${encodeURIComponent(file.name)}`,
        {
          method: "POST",
          headers: { "content-type": "application/octet-stream" },
          body: file,
        },
      );
      await inspectSelection(selection);
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not install this plugin.",
      );
    } finally {
      setBusy(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  const installHostEntry = async (entry: ResourceEntry) => {
    setBrowseHost(false);
    setBusy(true);
    setError(null);
    setInstalled(null);
    setStatus(`Selecting ${entry.name} on the RackForge host…`);
    try {
      const selection = await postResourceApi<ResourceSelection>(
        "/api/v1/resources/selections",
        { entry_id: entry.id },
      );
      await inspectSelection(selection);
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not install this plugin.",
      );
    } finally {
      setBusy(false);
    }
  };

  const installing = busy && preview !== null && installed === null;
  const canConfigure = installedDescriptor?.surfaces.some(
    (surface) => surface.kind === "config",
  ) ?? false;
  const dialogTitle = installed
    ? installed.already_installed ? "Plugin already installed" : "Plugin installed"
    : cancelled
      ? "Installation cancelled"
      : preview
        ? installing ? "Installing plugin" : "Review plugin"
        : "Install plugin";
  const dialogActions = installing ? (
    <button
      type="button"
      className="secondary-button"
      disabled={cancelling}
      onClick={() => void cancelInstallation()}
    >
      <AsyncActionLabel active={cancelling} activeLabel="Cancelling…">
        Cancel installation
      </AsyncActionLabel>
    </button>
  ) : preview ? (
    <>
      <button className="secondary-button" onClick={cancelPreview} disabled={busy}>
        Cancel
      </button>
      <button className="primary-button" onClick={() => void installPreview()} disabled={busy}>
        Install
      </button>
    </>
  ) : installed ? (
    <>
      <button className="secondary-button" onClick={closeDialog} disabled={busy}>
        Close
      </button>
      {canConfigure ? (
        <button
          className="secondary-button"
          onClick={() => void openInstalledPlugin("config")}
          disabled={busy}
        >
          Open configuration
        </button>
      ) : null}
      <button
        className="primary-button"
        onClick={() => void openInstalledPlugin("play")}
        disabled={busy}
      >
        <AsyncActionLabel active={busy} activeLabel="Opening…">
          Open in PLAY
        </AsyncActionLabel>
      </button>
    </>
  ) : cancelled || error ? (
    <button className="secondary-button" onClick={closeDialog} disabled={busy}>
      Close
    </button>
  ) : undefined;

  return (
    <>
      <ModalDialog
        eyebrow="Portable package"
        title={dialogTitle}
        onClose={closeDialog}
        dismissible={!busy && !browseHost}
        showClose={!installing}
        closeLabel="Close plugin installer"
        backdropClassName="install-plugin-backdrop"
        className="install-plugin-dialog"
        actions={dialogActions}
      >
        {!installed && !preview ? <p className="install-plugin-intro">
          {native || desktop
            ? "Select a portable .rfplugin package. RackForge validates it before installing anything."
            : "Choose where the .rfplugin package is located. RackForge validates it on the host before installing anything."}
        </p> : null}
        {!installed && !preview ? <div className="install-plugin-sources">
          <button
            type="button"
            className="install-source-card"
            disabled={busy}
            onClick={() => {
              if (native || desktop) void openNativePicker();
              else fileInputRef.current?.click();
            }}
          >
            <span className="install-source-icon">
              {desktop ? <FolderOpen aria-hidden="true" /> : <FileUp aria-hidden="true" />}
            </span>
            <span>
              <strong>{native || desktop ? "Choose plugin package" : "Upload from this device"}</strong>
              {remoteWeb ? (
                <small>Use the browser picker, then securely upload to the host</small>
              ) : null}
            </span>
          </button>
          {remoteWeb ? (
            <button
              type="button"
              className="install-source-card"
              disabled={busy}
              onClick={() => setBrowseHost(true)}
            >
              <span className="install-source-icon"><FolderOpen aria-hidden="true" /></span>
              <span>
                <strong>Browse the RackForge host</strong>
                <small>Select a package already stored on the host device</small>
              </span>
            </button>
          ) : null}
        </div> : null}
        <input
          ref={fileInputRef}
          className="visually-hidden"
          type="file"
          accept=".rfplugin,application/octet-stream"
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void uploadClientFile(file);
          }}
        />
        {preview ? <PluginInstallPreviewCard preview={preview} /> : null}
        {/* The faceplate's own loader -- the mark lighting limb by limb --
            as everywhere else something is being prepared, not a generic
            ring. */}
        {status ? (
          <RfLoader className="install-plugin-loader" label={status} size="compact" />
        ) : null}
        {error ? <p className="install-plugin-error">{error}</p> : null}
        {cancelled ? (
          <p className="install-plugin-cancelled" role="status">
            RackForge stopped before committing the package. No plugin was activated.
          </p>
        ) : null}
        {installed ? (
          <div className="install-plugin-complete">
            <div className="install-plugin-success" role="status">
              <strong>
                {installed.already_installed ? "Ready to open" : "Installation complete"}
              </strong>
              <span>{installed.plugin_id} v{installed.version}</span>
            </div>
            <p className="install-plugin-next-step">
              The package is installed but inactive. Open it in PLAY, configure it, or close this dialog.
            </p>
          </div>
        ) : null}
      </ModalDialog>
      {browseHost ? (
        <Suspense
          fallback={
            <div className="resource-explorer-backdrop">
              <RfLoader label="RackForge storage" detail="Opening explorer…" />
            </div>
          }
        >
          <ResourceExplorerDialog
            mode="select"
            selection={{ name: "plugin package", kind: "file" }}
            onCancel={() => setBrowseHost(false)}
            onSelected={(entry) => void installHostEntry(entry)}
          />
        </Suspense>
      ) : null}
    </>
  );
}
