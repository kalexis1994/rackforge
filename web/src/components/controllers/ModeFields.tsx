import { parameterChoices, valueLabel } from "../../controllerMapping";
import type { ParameterLinkMode, PluginParameterDescriptor } from "../../types";

/**
 * The settings of one mode, in the parameter's own terms: a choice by its
 * name, a number within the parameter's range and step.
 */
export function ModeFields({
  mode,
  parameter,
  disabled,
  onChange,
}: {
  mode: ParameterLinkMode;
  parameter: PluginParameterDescriptor;
  disabled?: boolean;
  onChange: (mode: ParameterLinkMode) => void;
}) {
  const value = (label: string, current: number, set: (value: number) => void) => (
    <ValueField label={label} parameter={parameter} value={current} disabled={disabled} onChange={set} />
  );
  switch (mode.kind) {
    case "direct":
    case "trigger":
      return null;
    case "range":
      return (
        <div className="controller-mode-fields two">
          {value("From", mode.min, (min) => onChange({ ...mode, min }))}
          {value("To", mode.max, (max) => onChange({ ...mode, max }))}
        </div>
      );
    case "set":
      return (
        <div className="controller-mode-fields">
          {value("Value", mode.value, (next) => onChange({ ...mode, value: next }))}
        </div>
      );
    case "toggle":
      return (
        <div className="controller-mode-fields two">
          {value("First", mode.first, (first) => onChange({ ...mode, first }))}
          {value("Second", mode.second, (second) => onChange({ ...mode, second }))}
        </div>
      );
    case "hold":
      return (
        <div className="controller-mode-fields two">
          {value("While held", mode.pressed, (pressed) => onChange({ ...mode, pressed }))}
          {value("When let go", mode.released, (released) => onChange({ ...mode, released }))}
        </div>
      );
    case "step":
      return (
        <div className="controller-mode-fields two">
          <label>
            <span>Direction</span>
            <select
              value={mode.direction}
              disabled={disabled}
              onChange={(event) => onChange({ ...mode, direction: event.target.value === "down" ? "down" : "up" })}
            >
              <option value="up">Up</option>
              <option value="down">Down</option>
            </select>
          </label>
          <label className="controller-mode-check">
            <input
              type="checkbox"
              checked={mode.wrap ?? false}
              disabled={disabled}
              onChange={(event) => onChange({ ...mode, wrap: event.target.checked })}
            />
            <span>Past the end, start again</span>
          </label>
        </div>
      );
    case "zones":
    case "cycle":
      return (
        <ValueSequence
          parameter={parameter}
          values={mode.values}
          disabled={disabled}
          hint={mode.kind === "zones" ? "Low to high along the control's travel." : "In the order each press reaches them."}
          onChange={(values) => onChange({ ...mode, values })}
        />
      );
  }
}

function ValueField({
  label,
  parameter,
  value,
  disabled,
  onChange,
}: {
  label: string;
  parameter: PluginParameterDescriptor;
  value: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  const choices = parameterChoices(parameter);
  if (choices.length > 0) {
    return (
      <label>
        <span>{label}</span>
        <select value={String(value)} disabled={disabled} onChange={(event) => onChange(Number(event.target.value))}>
          {choices.some((choice) => choice.value === value) ? null : (
            <option value={String(value)}>{valueLabel(parameter, value)} (not offered)</option>
          )}
          {choices.map((choice) => (
            <option key={choice.value} value={String(choice.value)}>{choice.name}</option>
          ))}
        </select>
      </label>
    );
  }
  const kind = parameter.kind;
  const bounds = kind.type === "float" || kind.type === "integer" ? kind : null;
  return (
    <label>
      <span>{label}{bounds?.unit ? ` (${bounds.unit})` : ""}</span>
      <input
        type="number"
        inputMode="decimal"
        value={Number.isFinite(value) ? value : ""}
        min={bounds?.minimum}
        max={bounds?.maximum}
        step={bounds ? (bounds.step > 0 ? bounds.step : "any") : "any"}
        disabled={disabled}
        onChange={(event) => {
          const next = event.target.valueAsNumber;
          if (Number.isFinite(next)) onChange(next);
        }}
      />
    </label>
  );
}

/**
 * The values a Zones or Cycle mode moves through: each of the parameter's
 * choices in or out, and the chosen ones in order.
 */
function ValueSequence({
  parameter,
  values,
  hint,
  disabled,
  onChange,
}: {
  parameter: PluginParameterDescriptor;
  values: number[];
  hint: string;
  disabled?: boolean;
  onChange: (values: number[]) => void;
}) {
  const choices = parameterChoices(parameter);
  const move = (index: number, offset: number) => {
    const target = index + offset;
    if (target < 0 || target >= values.length) return;
    const next = [...values];
    [next[index], next[target]] = [next[target], next[index]];
    onChange(next);
  };
  return (
    <div className="controller-mode-sequence">
      <span className="controller-mode-sequence-hint">{hint}</span>
      <ol>
        {values.map((value, index) => (
          <li key={`${value}-${index}`}>
            <span>{valueLabel(parameter, value)}</span>
            <button
              type="button"
              aria-label={`Move ${valueLabel(parameter, value)} earlier`}
              disabled={disabled || index === 0}
              onClick={() => move(index, -1)}
            >
              ↑
            </button>
            <button
              type="button"
              aria-label={`Move ${valueLabel(parameter, value)} later`}
              disabled={disabled || index === values.length - 1}
              onClick={() => move(index, 1)}
            >
              ↓
            </button>
            <button
              type="button"
              aria-label={`Leave ${valueLabel(parameter, value)} out`}
              disabled={disabled}
              onClick={() => onChange(values.filter((_, position) => position !== index))}
            >
              ×
            </button>
          </li>
        ))}
      </ol>
      {choices.some((choice) => !values.includes(choice.value)) ? (
        <label>
          <span>Add</span>
          <select
            value=""
            disabled={disabled}
            onChange={(event) => {
              if (event.target.value !== "") onChange([...values, Number(event.target.value)]);
            }}
          >
            <option value="">Choose a value…</option>
            {choices
              .filter((choice) => !values.includes(choice.value))
              .map((choice) => (
                <option key={choice.value} value={String(choice.value)}>{choice.name}</option>
              ))}
          </select>
        </label>
      ) : null}
    </div>
  );
}
