import type { ControllerDevice } from "../../controllerMapping";
import type { ModifierMode } from "../../types";

const MODE_LABELS: Record<ModifierMode, string> = {
  hold_or_double_tap: "Hold, or tap twice to latch",
  hold: "Hold",
  toggle: "Each press opens or closes",
};

/**
 * A controller's Fn button: which control it is, how it opens the Fn layer,
 * and whether the layer is open now. While it is, each control does what its
 * Fn mapping says, where it has one.
 */
export function FnButtonCard({
  device,
  open,
  readOnly,
  busy,
  onMode,
  onRemove,
}: {
  device: ControllerDevice;
  /** The Fn layer is open now, held or latched. */
  open: boolean;
  readOnly: boolean;
  busy: boolean;
  onMode: (mode: ModifierMode) => void;
  onRemove: () => void;
}) {
  const modifier = device.map?.modifier;
  const fnMappings = device.map?.plugins
    .flatMap((plugin) => plugin.mappings)
    .filter((mapping) => mapping.layer === "fn").length ?? 0;
  return (
    <section className="settings-card controller-fn-card" aria-label="Fn button">
      <div className="controller-fn-copy">
        <h2>
          Fn button
          {open ? <span className="controller-fn-open" role="status">Fn layer open</span> : null}
        </h2>
        {modifier ? (
          <p>
            <strong>{modifier.input.name}</strong> opens the Fn layer: {fnMappings === 0
              ? "no control has a mapping there yet."
              : `${fnMappings} ${fnMappings === 1 ? "mapping acts" : "mappings act"} there, and every other control keeps what it does.`}
          </p>
        ) : (
          <p>
            None. Choose a button or pad below and use it as Fn to reach the Fn layer
            {fnMappings > 0 ? `, where ${fnMappings} ${fnMappings === 1 ? "mapping waits" : "mappings wait"}` : ""}.
          </p>
        )}
      </div>
      {modifier && !readOnly ? (
        <div className="controller-fn-actions">
          <label>
            <span>Opens</span>
            <select
              value={modifier.mode ?? "hold_or_double_tap"}
              disabled={busy}
              onChange={(event) => onMode(event.target.value as ModifierMode)}
            >
              {(Object.keys(MODE_LABELS) as ModifierMode[]).map((mode) => (
                <option key={mode} value={mode}>{MODE_LABELS[mode]}</option>
              ))}
            </select>
          </label>
          <button type="button" className="secondary-button" disabled={busy} onClick={onRemove}>
            No Fn button
          </button>
        </div>
      ) : null}
    </section>
  );
}
