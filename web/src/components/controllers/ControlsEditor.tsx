import { useState } from "react";
import {
  type ControllerInput,
  type InputKind,
  inputMessageLabel,
  kindsForInput,
  userControllerProblem,
} from "../../controllerMapping";
import { AsyncActionLabel } from "../AsyncSpinner";

const KIND_LABELS: Record<InputKind, string> = {
  knob: "Knob",
  fader: "Fader",
  encoder: "Endless encoder",
  button: "Button",
  pad: "Pad",
  wheel: "Wheel",
  pedal: "Pedal",
};

/** One learnt control: its name, what it is, the group it is listed under. */
export function InputEditor({
  input,
  groups,
  onChange,
  onRemove,
  onClose,
}: {
  input: ControllerInput;
  groups: string[];
  onChange: (input: ControllerInput) => void;
  onRemove: () => void;
  onClose?: () => void;
}) {
  const listId = `controller-groups-${input.id}`;
  return (
    <section className="controller-input-detail" aria-label={input.name}>
      <header className="controller-input-detail-header">
        <div>
          <span className="card-kicker">Control</span>
          <h2>{input.name || "Unnamed"}</h2>
          <p>{inputMessageLabel(input)}</p>
        </div>
        {onClose ? (
          <button type="button" className="secondary-button controller-input-back" onClick={onClose}>
            All controls
          </button>
        ) : null}
      </header>
      <div className="controller-mapping-editor">
        <label>
          <span>Name</span>
          <input
            type="text"
            value={input.name}
            maxLength={48}
            autoComplete="off"
            onChange={(event) => onChange({ ...input, name: event.target.value })}
          />
        </label>
        <label>
          <span>It is a</span>
          <select
            value={input.kind}
            onChange={(event) => {
              const kind = event.target.value as InputKind;
              onChange({
                ...input,
                kind,
                // A button's report and an encoder's reading belong only to them.
                button: kind === "button" ? input.button : undefined,
                encoder: kind === "encoder" ? input.encoder : undefined,
              });
            }}
          >
            {kindsForInput(input).map((kind) => (
              <option key={kind} value={kind}>{KIND_LABELS[kind]}</option>
            ))}
          </select>
        </label>
        <label>
          <span>Group</span>
          <input
            type="text"
            value={input.group ?? ""}
            maxLength={48}
            list={listId}
            placeholder="Knobs, Pads, Transport…"
            autoComplete="off"
            onChange={(event) => onChange({ ...input, group: event.target.value.trim() ? event.target.value : undefined })}
          />
          <datalist id={listId}>
            {groups.map((group) => <option key={group} value={group} />)}
          </datalist>
        </label>
        {input.kind === "button" && typeof input.midi.cc === "number" ? (
          <label className="controller-mode-check">
            <input
              type="checkbox"
              checked={input.button?.press_only ?? false}
              onChange={(event) =>
                onChange({ ...input, button: { ...input.button, press_only: event.target.checked || undefined } })
              }
            />
            <span>It sends nothing when let go</span>
          </label>
        ) : null}
        <div className="controller-mapping-actions">
          <button type="button" className="secondary-button danger" onClick={onRemove}>
            Leave this control out
          </button>
        </div>
      </div>
    </section>
  );
}

/**
 * Names the controller and saves its controls as a package, which RackForge
 * then attaches to the input they were heard on.
 */
export function UserControllerForm({
  initialName,
  initialVendor,
  inputs,
  saving,
  existing,
  onSave,
  onCancel,
}: {
  initialName: string;
  initialVendor?: string;
  inputs: ControllerInput[];
  saving: boolean;
  existing: boolean;
  onSave: (name: string, vendor: string) => void;
  onCancel?: () => void;
}) {
  const [name, setName] = useState(initialName);
  const [vendor, setVendor] = useState(initialVendor ?? "");
  const problem = userControllerProblem(name, inputs);
  return (
    <form
      className="controllers-save-form"
      onSubmit={(event) => {
        event.preventDefault();
        if (!problem) onSave(name.trim(), vendor.trim());
      }}
    >
      <label>
        <span>Controller name</span>
        <input type="text" value={name} maxLength={64} disabled={saving} onChange={(event) => setName(event.target.value)} />
      </label>
      <label>
        <span>Made by</span>
        <input
          type="text"
          value={vendor}
          maxLength={64}
          placeholder="Optional"
          disabled={saving}
          onChange={(event) => setVendor(event.target.value)}
        />
      </label>
      <div className="controllers-save-actions">
        {onCancel ? (
          <button type="button" className="secondary-button" disabled={saving} onClick={onCancel}>
            Cancel
          </button>
        ) : null}
        <button type="submit" className="primary-button" disabled={saving || problem !== null}>
          <AsyncActionLabel active={saving} activeLabel="Saving…">
            {existing ? "Save controls" : "Save controller"}
          </AsyncActionLabel>
        </button>
      </div>
      {problem ? <p className="controller-mapping-problem">{problem}</p> : null}
    </form>
  );
}
