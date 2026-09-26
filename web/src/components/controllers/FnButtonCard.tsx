import { type ControllerDevice, type ControllerInput, inputMessageLabel } from "../../controllerMapping";
import type { ModifierMode } from "../../types";

const MODE_LABELS: Record<ModifierMode, string> = {
  hold_or_double_tap: "Hold, or tap twice to latch",
  hold: "Hold",
  toggle: "Each press opens or closes",
};

/** Choosing the Fn button by pressing it on the controller. */
export interface FnPick {
  deviceId: string;
  /** The button or pad last pressed, waiting for the player to confirm it. */
  candidate?: ControllerInput;
  /** Why the last control moved cannot be the Fn button. */
  problem?: string;
}

/**
 * A controller's Fn button: which control it is, how it opens the Fn layer,
 * and whether the layer is open now. While it is, each control does what its
 * Fn mapping says, where it has one.
 *
 * The player chooses it by pressing it: the card listens, names the button
 * or pad pressed, and asks before using it -- an Fn button does nothing
 * else, so its own mappings go.
 */
export function FnButtonCard({
  device,
  open,
  readOnly,
  busy,
  pick,
  candidateMappings,
  onListen,
  onConfirm,
  onCancel,
  onMode,
  onRemove,
}: {
  device: ControllerDevice;
  /** The Fn layer is open now, held or latched. */
  open: boolean;
  readOnly: boolean;
  busy: boolean;
  /** Listening for the button to use, when the player asked. */
  pick: FnPick | null;
  /** How many mappings the candidate has, which it would lose. */
  candidateMappings: number;
  onListen: () => void;
  onConfirm: (input: ControllerInput) => void;
  onCancel: () => void;
  onMode: (mode: ModifierMode) => void;
  onRemove: () => void;
}) {
  const modifier = device.map?.modifier;
  const fnMappings = device.map?.plugins
    .flatMap((plugin) => plugin.mappings)
    .filter((mapping) => mapping.layer === "fn").length ?? 0;
  const candidate = pick?.candidate;

  if (pick) {
    return (
      <section className="settings-card controller-fn-card listening" aria-label="Fn button">
        <div className="controller-fn-copy">
          <h2>
            <i className="controller-fn-listening" aria-hidden="true" />
            Fn button
          </h2>
          {candidate ? (
            <p role="status">
              Use <strong>{candidate.name}</strong> ({inputMessageLabel(candidate)}) as Fn?{" "}
              {candidateMappings > 0
                ? `Its ${candidateMappings} ${candidateMappings === 1 ? "mapping goes" : "mappings go"}: an Fn button does nothing else.`
                : "Press another to choose it instead."}
            </p>
          ) : (
            <p role="status">
              Press the button or pad on {device.name} to use as Fn.
              {pick.problem ? <span className="controller-fn-problem"> {pick.problem}</span> : null}
            </p>
          )}
        </div>
        <div className="controller-fn-actions">
          {candidate ? (
            <button type="button" className="secondary-button" disabled={busy} onClick={() => onConfirm(candidate)}>
              {busy ? "Saving…" : `Use ${candidate.name}`}
            </button>
          ) : null}
          <button type="button" className="secondary-button" disabled={busy} onClick={onCancel}>
            Cancel
          </button>
        </div>
      </section>
    );
  }

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
            None yet. An Fn button gives every other control a second job
            {fnMappings > 0 ? `; ${fnMappings} ${fnMappings === 1 ? "mapping waits" : "mappings wait"} in the Fn layer` : ""}.
            {device.connected || readOnly ? "" : " Connect the controller to choose it by pressing it."}
          </p>
        )}
      </div>
      {readOnly ? null : (
        <div className="controller-fn-actions">
          {device.connected ? (
            <button type="button" className="secondary-button" disabled={busy} onClick={onListen}>
              {modifier ? "Change by pressing…" : "Choose by pressing…"}
            </button>
          ) : null}
          {modifier ? (
            <>
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
            </>
          ) : null}
        </div>
      )}
    </section>
  );
}
