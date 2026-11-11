import { useEffect, useState } from "react";
import { SlidersHorizontal } from "lucide-react";
import { type ControlTakeover, requestControllerMaps, setControllerTakeover } from "../../gateway";

const TAKEOVERS: ReadonlyArray<readonly [ControlTakeover, string, string]> = [
  [
    "pickup",
    "Pickup",
    "A knob or fader does nothing until it reaches the value the parameter has; the screen shows which way to go. Nothing jumps.",
  ],
  [
    "scale",
    "Scale",
    "The parameter follows the control's direction at once, in proportion, and the two meet at the end of the travel. Nothing jumps and nothing waits.",
  ],
  [
    "jump",
    "Jump",
    "The parameter takes the control's value the moment it moves. The quickest, and the one that can jump audibly.",
  ],
];

/** How every knob and fader takes over a parameter standing somewhere else
 * -- set from the screen, by a pad, by a sound. Kept by the host, for every
 * controller at once. */
export function ControllerTakeoverCard() {
  const [takeover, setTakeover] = useState<ControlTakeover | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    requestControllerMaps()
      .then((answer) => {
        if (alive) setTakeover(answer.takeover);
      })
      .catch((reason: unknown) => {
        if (alive) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      alive = false;
    };
  }, []);

  const choose = (next: ControlTakeover) => {
    if (next === takeover || saving) return;
    const previous = takeover;
    setTakeover(next);
    setSaving(true);
    setError(null);
    setControllerTakeover(next)
      .then(setTakeover)
      .catch((reason: unknown) => {
        setTakeover(previous);
        setError(reason instanceof Error ? reason.message : String(reason));
      })
      .finally(() => setSaving(false));
  };

  const hint = TAKEOVERS.find(([id]) => id === takeover)?.[2];
  return (
    <article className="settings-card controller-takeover-card">
      <div className="settings-icon settings-icon-svg">
        <SlidersHorizontal aria-hidden="true" strokeWidth={1.7} />
      </div>
      <div className="settings-copy">
        <span className="card-kicker">Controllers</span>
        <h2>Knob &amp; Fader Takeover</h2>
        <p>
          What a knob or fader does to a parameter that is somewhere else, set
          from the screen, a pad or a sound.
        </p>
        <div className="controller-mode-keys" role="radiogroup" aria-label="Takeover">
          {TAKEOVERS.map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={takeover === id}
              className={takeover === id ? "active" : undefined}
              disabled={takeover === null || saving}
              onClick={() => choose(id)}
            >
              {label}
            </button>
          ))}
        </div>
        {hint ? <p className="controller-mapping-note">{hint}</p> : null}
        {error ? (
          <p className="controller-mapping-note" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </article>
  );
}
