import { useState } from "react";
import { AsyncActionLabel } from "../components/AsyncSpinner";
import { ModalDialog } from "../components/ModalDialog";
import { PluginRemovalOptions } from "../pluginRemoval";

export function PluginRemovalDialog({
  pluginName,
  active,
  removing,
  error,
  onClose,
  onConfirm,
}: {
  pluginName: string;
  active: boolean;
  removing: boolean;
  error?: string | null;
  onClose: () => void;
  onConfirm: (options: PluginRemovalOptions) => Promise<void>;
}) {
  const [deletePresets, setDeletePresets] = useState(false);
  const [deletePluginData, setDeletePluginData] = useState(false);

  return (
    <ModalDialog
      eyebrow="Installed plugin"
      title={`Remove ${pluginName}?`}
      role="alertdialog"
      onClose={onClose}
      dismissible={!removing}
      closeLabel="Close plugin removal"
      backdropClassName="plugin-remove-backdrop"
      className="plugin-remove-dialog"
      actions={
        <>
          <button className="secondary-button" disabled={removing} onClick={onClose}>Cancel</button>
          <button
            className="danger-button"
            disabled={removing}
            onClick={() => void onConfirm({
              delete_presets: deletePresets,
              delete_plugin_data: deletePluginData,
            })}
          >
            <AsyncActionLabel active={removing} activeLabel="Removing plugin…">
              Remove plugin
            </AsyncActionLabel>
          </button>
        </>
      }
    >
        <div className="plugin-remove-copy">
          <p>RackForge will always remove every installed version of this plugin package.</p>
          <fieldset className="plugin-remove-options" disabled={removing}>
            <legend>Also remove user data</legend>
            <label>
              <input
                type="checkbox"
                checked={deletePresets}
                onChange={(event) => setDeletePresets(event.target.checked)}
              />
              <span>
                <strong>Delete RackForge presets</strong>
                <small>Named presets are removed. State still referenced by racks or songs remains safe.</small>
              </span>
            </label>
            <label>
              <input
                type="checkbox"
                checked={deletePluginData}
                onChange={(event) => setDeletePluginData(event.target.checked)}
              />
              <span>
                <strong>Delete imported resources and plugin data</strong>
                <small>Removes extracted firmware or ROMs, custom programs and private caches.</small>
              </span>
            </label>
          </fieldset>
          <p className="plugin-remove-note">The archive selected for an import is temporary and is already discarded after a successful import.</p>
          {active ? <p>It is currently active, so sound will stop briefly while RackForge selects another available instrument.</p> : null}
          {error ? <p className="form-error">{error}</p> : null}
        </div>
    </ModalDialog>
  );
}
