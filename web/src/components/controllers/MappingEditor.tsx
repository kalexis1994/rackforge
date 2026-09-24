import { useMemo, useState } from "react";
import {
  type ControllerInput,
  defaultMode,
  isButtonInput,
  MODE_HINTS,
  MODE_LABELS,
  mappedInputFor,
  modeProblem,
  modesFor,
  newMappingId,
  parameterSections,
  suggestedMode,
} from "../../controllerMapping";
import { usePluginParameterSchema } from "../../hooks/usePluginParameterSchema";
import type { ControlMapping, ParameterLinkMode, PluginWebDescriptor } from "../../types";
import { AsyncActionLabel } from "../AsyncSpinner";
import { ModeFields } from "./ModeFields";

type PassThroughChoice = "default" | "pass_through" | "consume";

/**
 * One mapping of one input: which plugin, which of its parameters, and how
 * the input drives it. Nothing is stored until Save; the host checks the
 * mapping again when it is.
 */
export function MappingEditor({
  input,
  plugins,
  initialPluginId,
  existing,
  onSave,
  onCancel,
}: {
  input: ControllerInput;
  plugins: PluginWebDescriptor[];
  initialPluginId?: string;
  existing?: { plugin_id: string; mapping: ControlMapping };
  onSave: (plugin: PluginWebDescriptor, mapping: ControlMapping) => Promise<void>;
  onCancel: () => void;
}) {
  const [pluginId, setPluginId] = useState(
    existing?.plugin_id ?? initialPluginId ?? plugins[0]?.plugin_id ?? "",
  );
  const [parameterId, setParameterId] = useState(existing?.mapping.parameter_id ?? "");
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<ParameterLinkMode | null>(existing?.mapping.mode ?? null);
  const [invert, setInvert] = useState(existing?.mapping.invert ?? false);
  const [passThrough, setPassThrough] = useState<PassThroughChoice>(existing?.mapping.pass_through ?? "default");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const plugin = plugins.find((candidate) => candidate.plugin_id === pluginId);
  const schemaState = usePluginParameterSchema(plugin ? plugin.plugin_id : null, plugin?.version);
  const schema = schemaState.status === "ready" ? schemaState.schema : null;
  const parameter = schema?.parameters.find((candidate) => candidate.id === parameterId);
  const sections = useMemo(() => (schema ? parameterSections(schema, query) : []), [schema, query]);
  // A mode the chosen parameter cannot take is replaced by one it can, so
  // the form never shows a mode that would be refused.
  const effectiveMode = parameter
    ? mode && modeProblem(input, parameter, mode) === null
      ? mode
      : mode && modesFor(input, parameter).includes(mode.kind)
        ? mode
        : suggestedMode(input, parameter)
    : null;
  const problem = !parameter
    ? "Choose the parameter this control drives."
    : !effectiveMode
      ? `A ${input.kind} cannot drive ${parameter.name}.`
      : modeProblem(input, parameter, effectiveMode);
  const mappedInput = mappedInputFor(input);
  const button = isButtonInput(input);

  const save = async () => {
    if (!plugin || !parameter || !effectiveMode || problem || !mappedInput) return;
    setSaving(true);
    setError(null);
    try {
      await onSave(plugin, {
        id: existing?.mapping.id ?? newMappingId(),
        input: mappedInput,
        parameter_id: parameter.id,
        mode: effectiveMode,
        ...(invert && !button ? { invert: true } : {}),
        ...(passThrough !== "default" ? { pass_through: passThrough } : {}),
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Could not save this mapping.");
      setSaving(false);
    }
  };

  const instruments = plugins.filter((candidate) => candidate.kind === "instrument");
  const effects = plugins.filter((candidate) => candidate.kind === "effect");

  return (
    <form
      className="controller-mapping-editor"
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <label>
        <span>Plugin</span>
        <select
          value={pluginId}
          disabled={saving || Boolean(existing)}
          onChange={(event) => {
            setPluginId(event.target.value);
            setParameterId("");
            setMode(null);
          }}
        >
          {plugins.length === 0 ? <option value="">No plugin is installed</option> : null}
          {instruments.length > 0 ? (
            <optgroup label="Instruments">
              {instruments.map((candidate) => (
                <option key={candidate.plugin_id} value={candidate.plugin_id}>{candidate.plugin_name}</option>
              ))}
            </optgroup>
          ) : null}
          {effects.length > 0 ? (
            <optgroup label="Effects">
              {effects.map((candidate) => (
                <option key={candidate.plugin_id} value={candidate.plugin_id}>{candidate.plugin_name}</option>
              ))}
            </optgroup>
          ) : null}
        </select>
      </label>

      {schemaState.status === "loading" ? (
        <p className="controller-mapping-note">Reading {plugin?.plugin_name ?? "the plugin"}'s parameters…</p>
      ) : null}
      {schemaState.status === "error" ? (
        <p className="controller-mapping-error" role="alert">{schemaState.message}</p>
      ) : null}

      {schema ? (
        <>
          <label>
            <span>Find a parameter</span>
            <input
              type="search"
              value={query}
              placeholder="Leslie, cutoff, drive…"
              disabled={saving}
              onChange={(event) => setQuery(event.target.value)}
            />
          </label>
          <label>
            <span>Parameter</span>
            <select
              value={parameterId}
              disabled={saving}
              onChange={(event) => {
                setParameterId(event.target.value);
                setMode(null);
              }}
            >
              <option value="">{sections.length === 0 ? "Nothing matches" : "Choose a parameter…"}</option>
              {parameter && !sections.some((section) => section.parameters.includes(parameter)) ? (
                <option value={parameter.id}>{parameter.name}</option>
              ) : null}
              {sections.map((section) => (
                <optgroup key={section.title} label={section.title}>
                  {section.parameters.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>{candidate.name}</option>
                  ))}
                </optgroup>
              ))}
            </select>
          </label>
          {existing && parameterId && !parameter ? (
            <p className="controller-mapping-error" role="alert">
              {plugin?.plugin_name} no longer has the parameter this mapping drove ({parameterId}). Choose another, or remove the mapping.
            </p>
          ) : null}
        </>
      ) : null}

      {parameter && effectiveMode ? (
        <fieldset className="controller-mode-picker">
          <legend>Mode</legend>
          <div className="controller-mode-keys" role="radiogroup" aria-label="Mode">
            {modesFor(input, parameter).map((kind) => (
              <button
                key={kind}
                type="button"
                role="radio"
                aria-checked={effectiveMode.kind === kind}
                className={effectiveMode.kind === kind ? "active" : undefined}
                disabled={saving}
                onClick={() => setMode(kind === effectiveMode.kind ? effectiveMode : defaultMode(kind, parameter))}
              >
                {MODE_LABELS[kind]}
              </button>
            ))}
          </div>
          <p className="controller-mapping-note">{MODE_HINTS[effectiveMode.kind]}</p>
          <ModeFields mode={effectiveMode} parameter={parameter} disabled={saving} onChange={setMode} />
        </fieldset>
      ) : null}

      {parameter ? (
        <details className="controller-mapping-advanced">
          <summary>More</summary>
          {button ? null : (
            <label className="controller-mode-check">
              <input
                type="checkbox"
                checked={invert}
                disabled={saving}
                onChange={(event) => setInvert(event.target.checked)}
              />
              <span>Invert the control</span>
            </label>
          )}
          <label>
            <span>The MIDI message itself</span>
            <select
              value={passThrough}
              disabled={saving}
              onChange={(event) => setPassThrough(event.target.value as PassThroughChoice)}
            >
              <option value="default">
                {button ? "Keep it from the instrument (default for buttons)" : "Pass it on to the instrument (default for knobs)"}
              </option>
              <option value="pass_through">Pass it on to the instrument</option>
              <option value="consume">Keep it from the instrument</option>
            </select>
          </label>
        </details>
      ) : null}

      {error ? <p className="controller-mapping-error" role="alert">{error}</p> : null}
      {problem && parameter ? <p className="controller-mapping-problem">{problem}</p> : null}

      <div className="controller-mapping-actions">
        <button type="button" className="secondary-button" disabled={saving} onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="primary-button" disabled={saving || problem !== null || !mappedInput}>
          <AsyncActionLabel active={saving} activeLabel="Saving…">
            {existing ? "Save" : "Assign"}
          </AsyncActionLabel>
        </button>
      </div>
    </form>
  );
}
