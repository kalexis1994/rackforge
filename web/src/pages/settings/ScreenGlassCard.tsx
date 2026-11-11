import { useState } from "react";
import { ToggleSwitch } from "../../components/ToggleSwitch";
import { type ScreenGlass, readScreenGlass, storeScreenGlass } from "../../screen";
import { MonitorSmartphone } from "lucide-react";

/**
 * The cover over a plugin surface.
 *
 * On by default because it is what says the plugin is running *on* something.
 * Off is a real option, not a debug flag: a dense panel is easier to read
 * through nothing at all, and that trade belongs to whoever is playing.
 */
export function ScreenGlassCard() {
  const [glass, setGlass] = useState<ScreenGlass>(() => readScreenGlass());
  const on = glass === "glass";
  const choose = (next: ScreenGlass) => {
    setGlass(next);
    storeScreenGlass(next);
  };

  return (
    <article className="settings-card">
      <div className="settings-icon settings-icon-svg">
        <MonitorSmartphone aria-hidden="true" />
      </div>
      <div className="settings-copy">
        <span className="card-kicker">Plugin surface</span>
        <h2>Screen glass</h2>
        <p>
          Shows plugin panels behind an acrylic cover: the corners fall off, a
          sheen crosses the sheet, and it carries the film any panel picks up.
          Panels render bare unless you turn it on.
        </p>
      </div>
      <ToggleSwitch
        className="typing-keyboard-switch"
        checked={on}
        label="Screen glass"
        description={on ? "Panels sit behind the cover" : "Panels render bare"}
        checkedLabel="On"
        uncheckedLabel="Off"
        onChange={(next) => choose(next ? "glass" : "clean")}
      />
    </article>
  );
}
