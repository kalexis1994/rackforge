import { useState } from "react";
import { type LightingMode, readLighting, storeLighting } from "../lighting";

const LIGHTING_CHOICES: ReadonlyArray<{
  value: LightingMode;
  key: string;
  title: string;
}> = [
  { value: "auto", key: "AUTO", title: "Follow the system setting" },
  { value: "light", key: "DAY", title: "DAYLIGHT — bench and rehearsal" },
  { value: "dark", key: "STAGE", title: "STAGE — house lights down" },
];

/** Three-position lighting selector, mounted on the chassis rail. */
export function LightingSwitch() {
  const [mode, setMode] = useState<LightingMode>(() => readLighting());

  const choose = (next: LightingMode) => {
    setMode(next);
    storeLighting(next);
  };

  return (
    <div className="lighting-switch" role="group" aria-label="Lighting condition">
      <span className="lighting-switch-label">Lighting</span>
      <div className="lighting-switch-keys">
        {LIGHTING_CHOICES.map((choice) => (
          <button
            key={choice.value}
            type="button"
            className={`lighting-key${mode === choice.value ? " active" : ""}`}
            aria-pressed={mode === choice.value}
            title={choice.title}
            onClick={() => choose(choice.value)}
          >
            {choice.key}
          </button>
        ))}
      </div>
    </div>
  );
}
