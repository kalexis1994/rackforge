import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { AsyncActionLabel } from "../components/AsyncSpinner";
import { ModalDialog } from "../components/ModalDialog";
import { RfLoader } from "../components/RfLoader";
import { deletePluginPreset, exportPluginPreset, importPluginPreset, inspectPluginPreset, loadPluginPreset, renamePluginPreset, requestPluginPresets, savePluginPreset } from "../gateway";
import { isDesktopHost, isNativeHost, readNativeTextFile, savePortableTextFile } from "../host";
import { formatFileSize } from "../pluginPresentation";
import { type HostPresetSummary, type PluginInstance, type PresetImportConflictPolicy, type RfPresetFile, type RfPresetImportPreview } from "../types";
import { FileUp, X } from "lucide-react";

export function PresetModal({
  instance,
  onClose,
}: {
  instance: PluginInstance;
  onClose: () => void;
}) {
  const [presets, setPresets] = useState<HostPresetSummary[]>([]);
  const [loadingPresets, setLoadingPresets] = useState(true);
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameName, setRenameName] = useState("");
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const importInputRef = useRef<HTMLInputElement | null>(null);
  const [importCandidate, setImportCandidate] = useState<{
    fileName: string;
    file: RfPresetFile;
    preview: RfPresetImportPreview;
  } | null>(null);
  const [busyAction, setBusyAction] = useState<{
    kind: "load" | "save" | "rename" | "delete" | "export" | "inspect" | "import";
    presetId?: string;
  } | null>(null);
  const busy = busyAction !== null;
  const [message, setMessage] = useState<string | null>(null);
  const refresh = useCallback(() => {
    setLoadingPresets(true);
    return requestPluginPresets(instance.plugin_id)
      .then(setPresets)
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setLoadingPresets(false));
  }, [instance.plugin_id]);
  useEffect(() => {
    let cancelled = false;
    requestPluginPresets(instance.plugin_id)
      .then((nextPresets) => {
        if (!cancelled) setPresets(nextPresets);
      })
      .catch((error: Error) => {
        if (!cancelled) setMessage(error.message);
      })
      .finally(() => {
        if (!cancelled) setLoadingPresets(false);
      });
    return () => {
      cancelled = true;
    };
  }, [instance.plugin_id]);
  const load = (preset: HostPresetSummary) => {
    setBusyAction({ kind: "load", presetId: preset.id });
    setMessage(null);
    loadPluginPreset(instance.instance_id, preset.id)
      .then(onClose)
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const save = (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    setBusyAction({ kind: "save" });
    setMessage(null);
    savePluginPreset(instance.instance_id, name.trim())
      .then((preset) => {
        setName("");
        setCreating(false);
        setMessage(`Saved ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const rename = (event: FormEvent, preset: HostPresetSummary) => {
    event.preventDefault();
    if (!renameName.trim()) return;
    setBusyAction({ kind: "rename", presetId: preset.id });
    setMessage(null);
    renamePluginPreset(instance.plugin_id, preset.id, renameName.trim())
      .then((renamed) => {
        setRenamingId(null);
        setRenameName("");
        setMessage(`Renamed to ${renamed.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const remove = (preset: HostPresetSummary) => {
    setBusyAction({ kind: "delete", presetId: preset.id });
    setMessage(null);
    deletePluginPreset(instance.plugin_id, preset.id)
      .then(() => {
        setDeletingId(null);
        setMessage(`Deleted ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const inspectImportText = async (fileName: string, text: string) => {
    setBusyAction({ kind: "inspect" });
    setMessage(null);
    try {
      if (!fileName.toLowerCase().endsWith(".rfpreset")) {
        throw new Error("Choose a .rfpreset file.");
      }
      if (!text || new TextEncoder().encode(text).byteLength > 2 * 1024 * 1024) {
        throw new Error("The preset file is empty or larger than 2 MiB.");
      }
      const file = JSON.parse(text) as RfPresetFile;
      const preview = await inspectPluginPreset(instance.plugin_id, file);
      setImportCandidate({ fileName, file, preview });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Could not validate the preset file.");
    } finally {
      setBusyAction(null);
    }
  };
  const chooseImport = () => {
    if (isNativeHost() || isDesktopHost()) {
      setBusyAction({ kind: "inspect" });
      setMessage(null);
      readNativeTextFile({ extensions: ["rfpreset"], maximum_bytes: 2 * 1024 * 1024 })
        .then(({ file_name, text }) => inspectImportText(file_name, text))
        .catch((error: Error) => setMessage(error.message))
        .finally(() => setBusyAction((current) => current?.kind === "inspect" ? null : current));
      return;
    }
    importInputRef.current?.click();
  };
  const exportPresetFile = (preset: HostPresetSummary) => {
    setBusyAction({ kind: "export", presetId: preset.id });
    setMessage(null);
    exportPluginPreset(instance.plugin_id, preset.id)
      .then(({ file_name, file }) => savePortableTextFile({
        file_name,
        mime_type: "application/vnd.rackforge.preset+json",
        text: `${JSON.stringify(file, null, 2)}\n`,
      }))
      .then(() => setMessage(`Exported ${preset.name}.`))
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const commitImport = (policy: PresetImportConflictPolicy) => {
    if (!importCandidate) return;
    setBusyAction({ kind: "import" });
    setMessage(null);
    importPluginPreset(instance.plugin_id, importCandidate.file, policy)
      .then((preset) => {
        setImportCandidate(null);
        setMessage(`Imported ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  return (
    <ModalDialog
      eyebrow={`${instance.plugin_name} · Complete states`}
      title="Presets"
      onClose={onClose}
      dismissible={!busy}
      closeLabel="Close presets"
      backdropClassName="plugin-area-backdrop"
    >
      {importCandidate ? (
        <section className="preset-import-stage" aria-label="Portable preset preview">
          <header className="preset-import-stage-header">
            <div>
              <span>Portable preset · Preview</span>
              <h3>Import {importCandidate.preview.preset.name}?</h3>
            </div>
            <button
              className="preset-modal-close modal-dialog-close"
              type="button"
              aria-label="Cancel preset import"
              disabled={busy}
              onClick={() => setImportCandidate(null)}
            >
              <X aria-hidden="true" />
            </button>
          </header>
          <div className="preset-import-summary">
            <dl>
              <div><dt>Plugin</dt><dd>{importCandidate.preview.preset.plugin_id}</dd></div>
              <div><dt>Plugin version</dt><dd>v{importCandidate.preview.preset.plugin_version}</dd></div>
              <div><dt>State format</dt><dd>v{importCandidate.preview.preset.state_version}</dd></div>
              <div><dt>State size</dt><dd>{formatFileSize(importCandidate.preview.byte_length)}</dd></div>
              <div><dt>File</dt><dd>{importCandidate.fileName}</dd></div>
            </dl>
            {importCandidate.preview.conflict ? (
              <p className="preset-import-conflict">A local preset already uses this name or identity.</p>
            ) : null}
            {importCandidate.preview.warnings.map((warning) => (
              <p className="preset-import-warning" key={warning}>{warning}</p>
            ))}
          </div>
          <footer className="preset-import-actions">
            <button className="secondary-button" disabled={busy} onClick={() => setImportCandidate(null)}>
              Cancel
            </button>
            {importCandidate.preview.conflict ? (
              <button className="secondary-button" disabled={busy || !importCandidate.preview.compatible} onClick={() => commitImport("keep_both")}>
                Import as copy
              </button>
            ) : null}
            {importCandidate.preview.conflict !== "ambiguous" ? (
              <button
                className="primary-button"
                disabled={busy || !importCandidate.preview.compatible}
                onClick={() => commitImport(importCandidate.preview.conflict ? "replace" : "reject")}
              >
                <AsyncActionLabel active={busyAction?.kind === "import"} activeLabel="Importing…">
                  {importCandidate.preview.conflict ? "Replace existing" : "Import preset"}
                </AsyncActionLabel>
              </button>
            ) : null}
          </footer>
        </section>
      ) : (
        <>
        <div className="preset-modal-toolbar">
          <p>Load a captured state or save the instrument exactly as it sounds now.</p>
          <div className="preset-toolbar-actions">
            <button className="preset-import-button" disabled={busy} onClick={chooseImport}>
              <FileUp aria-hidden="true" />
              <AsyncActionLabel active={busyAction?.kind === "inspect"} activeLabel="Validating…">
                Import .rfpreset
              </AsyncActionLabel>
            </button>
            <button className="preset-create-button" disabled={busy} onClick={() => setCreating((value) => !value)}>
              <span aria-hidden="true">＋</span> New preset
            </button>
          </div>
          <input
            ref={importInputRef}
            className="visually-hidden"
            type="file"
            accept=".rfpreset,application/vnd.rackforge.preset+json,application/json"
            onChange={(event) => {
              const selected = event.currentTarget.files?.[0];
              event.currentTarget.value = "";
              if (!selected) return;
              void selected.text()
                .then((text) => inspectImportText(selected.name, text))
                .catch((error: Error) => setMessage(error.message));
            }}
          />
        </div>
        {creating && (
          <form className="preset-create-form" onSubmit={save}>
            <label>
              <span>Preset name</span>
              <input autoFocus maxLength={96} value={name} onChange={(event) => setName(event.target.value)} placeholder="Warm Strings" />
            </label>
            <button disabled={busy || !name.trim()} type="submit">
              <AsyncActionLabel active={busyAction?.kind === "save"} activeLabel="Saving…">
                Capture state
              </AsyncActionLabel>
            </button>
          </form>
        )}
        {message && <p className="preset-message">{message}</p>}
        <div className="preset-list modal-list">
          {loadingPresets && presets.length === 0 ? (
            <RfLoader
              label="Loading presets"
              detail="Reading complete instrument states…"
              size="compact"
            />
          ) : presets.length === 0 ? (
            <div className="preset-empty"><span>00</span><strong>No presets yet</strong><small>Capture the current plugin state to create the first one.</small></div>
          ) : presets.map((preset) => (
            <article className="preset-row" key={preset.id}>
              {renamingId === preset.id ? (
                <form className="preset-rename-form" onSubmit={(event) => rename(event, preset)}>
                  <input autoFocus maxLength={96} value={renameName} onChange={(event) => setRenameName(event.target.value)} />
                  <button disabled={busy || !renameName.trim()} type="submit">
                    <AsyncActionLabel
                      active={busyAction?.kind === "rename" && busyAction.presetId === preset.id}
                      activeLabel="Renaming…"
                    >
                      Save
                    </AsyncActionLabel>
                  </button>
                  <button type="button" onClick={() => setRenamingId(null)}>Cancel</button>
                </form>
              ) : (
                <>
                  <button className="preset-load-target" disabled={busy} onClick={() => load(preset)}>
                    <span><strong>{preset.name}</strong><small>State v{preset.state_version} · Plugin {preset.plugin_version}</small></span>
                    <i>
                      <AsyncActionLabel
                        active={busyAction?.kind === "load" && busyAction.presetId === preset.id}
                        activeLabel="Loading…"
                      >
                        LOAD →
                      </AsyncActionLabel>
                    </i>
                  </button>
                  <div className="preset-row-actions">
                    <button disabled={busy} onClick={() => exportPresetFile(preset)}>
                      <AsyncActionLabel
                        active={busyAction?.kind === "export" && busyAction.presetId === preset.id}
                        activeLabel="Exporting…"
                      >
                        Export
                      </AsyncActionLabel>
                    </button>
                    <button disabled={busy} onClick={() => {
                      setDeletingId(null);
                      setRenamingId(preset.id);
                      setRenameName(preset.name);
                    }}>Rename</button>
                    <button className="danger" disabled={busy} onClick={() => setDeletingId(preset.id)}>Delete</button>
                  </div>
                </>
              )}
              {deletingId === preset.id && renamingId !== preset.id && (
                <div className="preset-delete-confirm">
                  <span>Delete “{preset.name}”?</span>
                  <button onClick={() => setDeletingId(null)}>Cancel</button>
                  <button className="danger" disabled={busy} onClick={() => remove(preset)}>
                    <AsyncActionLabel
                      active={busyAction?.kind === "delete" && busyAction.presetId === preset.id}
                      activeLabel="Deleting…"
                    >
                      Delete
                    </AsyncActionLabel>
                  </button>
                </div>
              )}
            </article>
          ))}
        </div>
        </>
      )}
    </ModalDialog>
  );
}
