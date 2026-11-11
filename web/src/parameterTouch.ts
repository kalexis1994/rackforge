import { type PluginParameterDescriptor } from "./types";

/** What a control last did to a plugin parameter: the engine's
 * `parameter_touch` answer, the same one LITTLE's header is made from. */
export interface ParameterTouchReport {
  instance_id: string;
  parameter: PluginParameterDescriptor;
  value: number;
  display_decimals?: number;
  pickup: "engaged" | "move_up" | "move_down";
  /** While the control is on its way to the parameter, the value it stands at. */
  control?: number;
}

/** As many decimals as a step asks for, capped where a display runs out --
 * the rule LITTLE reads a value by. */
export function parameterDecimals(step: number, displayDecimals?: number): number {
  if (displayDecimals !== undefined) return displayDecimals;
  const CEILING = 6;
  if (step >= 1) return 0;
  let scaled = Math.abs(step);
  if (scaled !== 0 && scaled <= 1e-9) return CEILING;
  let decimals = 0;
  while (decimals < CEILING && Math.abs(Math.round(scaled) - scaled) > 1e-9) {
    scaled *= 10;
    decimals += 1;
  }
  return decimals;
}

/** A parameter's value in its own units: "1462 m³", "Tremolo", "On". */
export function parameterDisplayValue(
  parameter: PluginParameterDescriptor,
  value: number,
  displayDecimals?: number,
): string {
  const withUnit = (text: string, unit?: string) => (unit ? `${text} ${unit}` : text);
  const kind = parameter.kind;
  switch (kind.type) {
    case "float":
      return withUnit(value.toFixed(parameterDecimals(kind.step, displayDecimals)), kind.unit);
    case "integer":
      return withUnit(String(Math.round(value)), kind.unit);
    case "boolean":
      return value >= 0.5 ? "On" : "Off";
    case "enum":
      return kind.choices.find((choice) => choice.value === value)?.name ?? value.toFixed(0);
    case "trigger":
      return "Press";
    case "meter":
      return withUnit(value.toFixed(displayDecimals ?? 2), kind.unit);
  }
}

/** The line a screen shows for a touch: the parameter's name, and its value
 * -- or, while the control is on its way, where the control stands, which
 * way to go and where the parameter is: "2 ↑6", one unit for both. */
export function parameterTouchLine(touch: ParameterTouchReport): { name: string; value: string } {
  const target = parameterDisplayValue(touch.parameter, touch.value, touch.display_decimals);
  if (touch.pickup === "engaged") return { name: touch.parameter.name, value: target };
  const arrow = touch.pickup === "move_up" ? "↑" : "↓";
  let shown = `${arrow}${target}`;
  if (touch.control !== undefined) {
    let at = parameterDisplayValue(touch.parameter, touch.control, touch.display_decimals);
    const space = at.lastIndexOf(" ");
    if (space > 0 && target.endsWith(at.slice(space))) at = at.slice(0, space);
    shown = `${at} ${shown}`;
  }
  return { name: touch.parameter.name, value: shown };
}
