import { useState } from "react";
import {
  type ControllerDevice,
  type ControllerInput,
  describeMode,
  inputMessageLabel,
  isButtonInput,
  isModifierInput,
  mappingLayer,
  mappingsForInput,
  standardMeaning,
} from "../../controllerMapping";
import { usePluginParameterSchema } from "../../hooks/usePluginParameterSchema";
import type { ControlMapping, MapLayer, PluginWebDescriptor } from "../../types";
import { AsyncActionLabel } from "../AsyncSpinner";
import { MappingEditor } from "./MappingEditor";

/**
 * One input: what the package means it to do, what the player made it do in
 * each plugin -- in the base layer and, with the controller's Fn button,
 * in the Fn layer -- and the editor for a new or changed mapping. A button
 * can become the Fn button itself.
 */
export function InputAssignments({
  device,
  input,
  plugins,
  playingPluginId,
  readOnly = false,
  onSaveMapping,
  onRemoveMapping,
  onSetModifier,
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
  /** Makes this input the Fn button, or (with null) leaves the controller without one. */
  onSetModifier?: (input: ControllerInput | null) => Promise<void>;
  onClose?: () => void;
}) {
  const [editing, setEditing] = useState<
    { plugin_id: string; mapping: ControlMapping } | { layer: MapLayer } | null
  >(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [settingFn, setSettingFn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const assignments = mappingsForInput(device.map, input);
  const standard = standardMeaning(input, device.roles, device.actions);
  const isFn = isModifierInput(device.map, input);
  const hasFn = Boolean(device.map?.modifier);
  const pluginName = (id: string, fallback: string) =>
    plugins.find((plugin) => plugin.plugin_id === id)?.plugin_name ?? fallback;
  // A new mapping starts on the plugin playing, unless the input already has
  // one there in that layer.
  const firstPlugin = (layer: MapLayer) => {
    const taken = new Set(
      assignments.filter((entry) => mappingLayer(entry.mapping) === layer).map((entry) => entry.plugin_id),
    );
    return playingPluginId && !taken.has(playingPluginId)
      ? playingPluginId
      : plugins.find((plugin) => !taken.has(plugin.plugin_id))?.plugin_id;
  };
  const base = assignments.filter((entry) => mappingLayer(entry.mapping) === "base");
  const withFn = assignments.filter((entry) => mappingLayer(entry.mapping) === "fn");

  const setModifier = (next: ControllerInput | null) => {
    if (!onSetModifier) return;
    setSettingFn(true);
    setError(null);
    onSetModifier(next)
      .catch((reason: unknown) =>
        setError(reason instanceof Error ? reason.message : "Could not change the Fn button."),
      )
      .finally(() => setSettingFn(false));
  };

  const row = (entry: (typeof assignments)[number]) => (
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
  );

  return (
    <section className="controller-input-detail" aria-label={input.name}>
      <header className="controller-input-detail-header">
        <div>
          <span className="card-kicker">{isFn ? `${input.kind} · Fn button` : input.kind}</span>
          <h2>{input.name}</h2>
          <p>{inputMessageLabel(input)}</p>
        </div>
        {onClose ? (
          <button type="button" className="secondary-button controller-input-back" onClick={onClose}>
            All controls
          </button>
        ) : null}
      </header>

      {error ? <p className="controller-mapping-error" role="alert">{error}</p> : null}

      {isFn ? (
        <>
          <p className="controller-input-empty">
            This is the controller's Fn button. It opens the Fn layer, where each control does what its Fn
            mapping says, and does nothing else itself.
          </p>
          {readOnly ? null : (
            <button type="button" className="secondary-button" disabled={settingFn} onClick={() => setModifier(null)}>
              <AsyncActionLabel active={settingFn} activeLabel="Changing…">Stop using it as Fn</AsyncActionLabel>
            </button>
          )}
        </>
      ) : (
        <>
          {standard ? (
            <p className="controller-input-standard">
              <span>Standard</span> {standard}
              {base.length > 0 ? " — where no mapping of yours applies" : ""}
            </p>
          ) : null}

          {editing ? (
            <MappingEditor
              key={"layer" in editing ? `new-${editing.layer}` : editing.mapping.id}
              input={input}
              plugins={plugins}
              layer={"layer" in editing ? editing.layer : mappingLayer(editing.mapping)}
              initialPluginId={"layer" in editing ? firstPlugin(editing.layer) : undefined}
              existing={"layer" in editing ? undefined : editing}
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
              ) : null}
              {base.length > 0 ? <ul className="controller-assignment-list">{base.map(row)}</ul> : null}
              {withFn.length > 0 ? (
                <>
                  <h3 className="controller-assignment-layer">
                    With Fn{hasFn ? "" : " — waits for a Fn button"}
                  </h3>
                  <ul className="controller-assignment-list">{withFn.map(row)}</ul>
                </>
              ) : null}
              {readOnly ? null : (
                <div className="controller-assign-actions">
                  <button
                    type="button"
                    className="primary-button controller-assign-button"
                    disabled={plugins.length === 0 || firstPlugin("base") === undefined}
                    onClick={() => setEditing({ layer: "base" })}
                  >
                    {base.length === 0 ? "Assign to a plugin…" : "Assign in another plugin…"}
                  </button>
                  {hasFn ? (
                    <button
                      type="button"
                      className="secondary-button controller-assign-button"
                      disabled={plugins.length === 0 || firstPlugin("fn") === undefined}
                      onClick={() => setEditing({ layer: "fn" })}
                    >
                      Assign with Fn…
                    </button>
                  ) : null}
                  {isButtonInput(input) && onSetModifier ? (
                    <button
                      type="button"
                      className="secondary-button"
                      disabled={settingFn}
                      title="It opens the Fn layer and does nothing else: what it was assigned to goes."
                      onClick={() => setModifier(input)}
                    >
                      <AsyncActionLabel active={settingFn} activeLabel="Changing…">Use as Fn button</AsyncActionLabel>
                    </button>
                  ) : null}
                </div>
              )}
              {!readOnly && plugins.length > 0 && firstPlugin("base") === undefined ? (
                <p className="controller-input-empty">This control already has a mapping in every installed plugin.</p>
              ) : null}
              {plugins.length === 0 ? (
                <p className="controller-input-empty">Install an instrument or an effect to assign this control to it.</p>
              ) : null}
            </>
          )}
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
        <strong>
          {pluginName}
          {mappingLayer(mapping) === "fn" ? <span className="controller-assignment-fn">Fn</span> : null}
          {playing ? <span className="controller-assignment-live">Playing</span> : null}
        </strong>
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
