import { type ControllerCheck, checkProgress, strayCount, valuesLabel } from "../../controllerCheck";
import type { ControllerDevice } from "../../controllerMapping";

/** Enough strays to see a pattern without burying the list. */
const STRAYS_SHOWN = 8;

/**
 * Checking a controller against its package: how many of its controls the
 * host has heard send what the package declares, and the messages none of
 * them declares. The list below ticks each control off; the report carries
 * the values, for the package's SOURCES.md.
 */
export function ControllerCheckCard({
  device,
  check,
  canCopy,
  onCopy,
  onSave,
  onRestart,
  onStop,
}: {
  device: ControllerDevice;
  check: ControllerCheck;
  canCopy: boolean;
  onCopy: () => void;
  onSave: () => void;
  onRestart: () => void;
  onStop: () => void;
}) {
  const { heard, total } = checkProgress(check, device.inputs);
  const strays = Object.values(check.strays).sort((left, right) => right.count - left.count);
  return (
    <section className="settings-card controller-check" aria-label="Hardware check">
      <div className="controller-check-copy">
        <h2>Checking {device.name}</h2>
        <p>
          Move every knob and fader through its whole travel, turn each encoder both ways, and press every
          button and pad. Each control ticks off when RackForge hears the message its package declares.
        </p>
        <meter
          className="controller-check-meter"
          min={0}
          max={Math.max(total, 1)}
          value={heard}
          aria-label="Controls heard"
        />
        <p className="controller-check-count" role="status">
          Heard {heard} of {total} controls · {strayCount(check)} no control declares
        </p>
      </div>
      <div className="controller-check-actions">
        {canCopy ? (
          <button type="button" className="secondary-button" onClick={onCopy}>
            Copy report
          </button>
        ) : null}
        <button type="button" className="secondary-button" onClick={onSave}>
          Save report
        </button>
        <button type="button" className="secondary-button" onClick={onRestart}>
          Start over
        </button>
        <button type="button" className="secondary-button" onClick={onStop}>
          Done
        </button>
      </div>
      {strays.length > 0 ? (
        <ul className="controller-check-strays" aria-label="Messages no control declares">
          {strays.slice(0, STRAYS_SHOWN).map((stray) => (
            <li key={stray.label}>
              <strong>{stray.label}</strong>
              <span>
                ×{stray.count} · {valuesLabel(stray, stray.label.startsWith("Note "))}
              </span>
              {stray.near ? <small>Close to {stray.near}</small> : null}
            </li>
          ))}
          {strays.length > STRAYS_SHOWN ? (
            <li className="controller-check-more">and {strays.length - STRAYS_SHOWN} more in the report</li>
          ) : null}
        </ul>
      ) : null}
    </section>
  );
}
