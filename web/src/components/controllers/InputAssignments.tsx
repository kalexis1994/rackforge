import { useState } from "react";
import {
  type ControllerDevice,
  type ControllerInput,
  describeMode,
  inputMessageLabel,
  mappingsForInput,
  standardMeaning,
} from "../../controllerMapping";
import { usePluginParameterSchema } from "../../hooks/usePluginParameterSchema";
import type { ControlMapping, PluginWebDescriptor } from "../../types";
import { AsyncActionLabel } from "../AsyncSpinner";
import { MappingEditor } from "./MappingEditor";

/**
 * One input: what the package means it to do, what the player made it do in
 * each plugin, and the editor for a new or changed mapping.
 */
export function InputAssignments({
  device,
  input,
  plugins,
  playingPluginId,
  readOnly = false,
  onSaveMapping,
  onRemoveMapping,
  onClose,
}: {
  device: ControllerDevice;
  input: ControllerInput;
  plugins: PluginWebDescriptor[];
  playingPluginId?: string;
  /** The host keeps no maps: what is mapped is shown, nothing is offered. */
  readOnly?: boolean;
  onSaveMapping: (plugin: PluginWebDescriptor, mapping: ControlMapping) => Promise<void>;
  onRemoveMapping: (pluginId: string, mappingId: string) => Promise<void>;
  onClose?: () => void;
}) {
  const [editing, setEditing] = useState<{ plugin_id: string; mapping: ControlMapping } | "new" | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const assignments = mappingsForInput(device.map, input);
  const standard = standardMeaning(input, device.roles, device.actions);
  const pluginName = (id: string, fallback: string) =>
    plugins.find((plugin) => plugin.plugin_id === id)?.plugin_name ?? fallback;
  const available = new Set(assignments.map((entry) => entry.plugin_id));
  // A new mapping starts on the plugin playing, unless the input already has
  // one there.
  const firstPlugin = playingPluginId && !available.has(playingPluginId)
    ? playingPluginId
    : plugins.find((plugin) => !available.has(plugin.plugin_id))?.plugin_id;

  return (
    <section className="controller-input-detail" aria-label={input.name}>
      <header className="controller-input-detail-header">
        <div>
          <span className="card-kicker">{input.kind}</span>
          <h2>{input.name}</h2>
          <p>{inputMessageLabel(input)}</p>
        </div>
        {onClose ? (
          <button type="button" className="secondary-button controller-input-back" onClick={onClose}>
            All controls
          </button>
        ) : null}
      </header>

      {standard ? (
        <p className="controller-input-standard">
          <span>Standard</span> {standard}
          {assignments.length > 0 ? " — where no mapping of yours applies" : ""}
        </p>
      ) : null}

      {error ? <p className="controller-mapping-error" role="alert">{error}</p> : null}

      {editing ? (
        <MappingEditor
          key={editing === "new" ? "new" : editing.mapping.id}
          input={input}
          plugins={plugins}
          initialPluginId={firstPlugin}
          existing={editing === "new" ? undefined : editing}
          onCancel={() => setEditing(null)}
          onSave={async (plugin, mapping) => {
            await onSaveMapping(plugin, mapping);
            setEditing(null);
          }}
        />
      ) : (
        <>
          {assignments.length === 0 ? (
            <p className="controller-input-empty">
              {standard
                ? "No mapping of yours. The standard meaning applies wherever a plugin takes it."
                : "Nothing is assigned to this control yet."}
            </p>
          ) : (
            <ul className="controller-assignment-list">
              {assignments.map((entry) => (
                <AssignmentRow
                  key={entry.mapping.id}
                  pluginId={entry.plugin_id}
                  pluginName={pluginName(entry.plugin_id, entry.plugin_name)}
                  pluginVersion={plugins.find((plugin) => plugin.plugin_id === entry.plugin_id)?.version}
                  installed={plugins.some((plugin) => plugin.plugin_id === entry.plugin_id)}
                  playing={entry.plugin_id === playingPluginId}
                  mapping={entry.mapping}
                  removing={removing === entry.mapping.id}
                  readOnly={readOnly}
                  onEdit={() => setEditing({ plugin_id: entry.plugin_id, mapping: entry.mapping })}
                  onRemove={() => {
                    setRemoving(entry.mapping.id);
                    setError(null);
                    onRemoveMapping(entry.plugin_id, entry.mapping.id)
                      .catch((reason: unknown) =>
                        setError(reason instanceof Error ? reason.message : "Could not remove this mapping."),
                      )
                      .finally(() => setRemoving(null));
                  }}
                />
              ))}
            </ul>
          )}
          {readOnly ? null : (
            <button
              type="button"
              className="primary-button controller-assign-button"
              disabled={plugins.length === 0 || firstPlugin === undefined}
              onClick={() => setEditing("new")}
            >
              {assignments.length === 0 ? "Assign to a plugin…" : "Assign in another plugin…"}
            </button>
          )}
          {!readOnly && plugins.length > 0 && firstPlugin === undefined ? (
            <p className="controller-input-empty">This control already has a mapping in every installed plugin.</p>
          ) : null}
          {plugins.length === 0 ? (
            <p className="controller-input-empty">Install an instrument or an effect to assign this control to it.</p>
          ) : null}
        </>
      )}
    </section>
  );
}

function AssignmentRow({
  pluginId,
  pluginName,
  pluginVersion,
  installed,
  playing,
  mapping,
  removing,
  readOnly,
  onEdit,
  onRemove,
}: {
  pluginId: string;
  pluginName: string;
  pluginVersion?: string;
  installed: boolean;
  playing: boolean;
  mapping: ControlMapping;
  removing: boolean;
  readOnly: boolean;
  onEdit: () => void;
  onRemove: () => void;
}) {
  const schema = usePluginParameterSchema(installed ? pluginId : null, pluginVersion);
  const parameter = schema.status === "ready"
    ? schema.schema.parameters.find((candidate) => candidate.id === mapping.parameter_id)
    : undefined;
  const missing = schema.status === "ready" && !parameter;
  return (
    <li className={`controller-assignment${playing ? " playing" : ""}${missing || !installed ? " pending" : ""}`}>
      <div className="controller-assignment-copy">
        <strong>{pluginName}{playing ? <span className="controller-assignment-live">Playing</span> : null}</strong>
        <span>{parameter?.name ?? mapping.parameter_id}</span>
        <small>
          {!installed
            ? "The plugin is not installed: the mapping waits for it."
            : missing
              ? "The plugin no longer has this parameter: the mapping waits."
              : describeMode(mapping.mode, parameter)}
        </small>
      </div>
      {readOnly ? null : (
        <div className="controller-assignment-actions">
          <button type="button" className="secondary-button" disabled={!installed || removing} onClick={onEdit}>
            Edit
          </button>
          <button type="button" className="secondary-button danger" disabled={removing} onClick={onRemove}>
            <AsyncActionLabel active={removing} activeLabel="Removing…">Remove</AsyncActionLabel>
          </button>
        </div>
      )}
    </li>
  );
}
