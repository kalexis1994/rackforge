import { MasterLevel, MasterOutputMeter, MasterPan } from "../../components/MasterSection";
import { subscribeParameterTouches } from "../../gateway";
import { isVstHost } from "../../host";
import { parameterTouchLine } from "../../parameterTouch";
import { type PerformanceSnapshot, type SessionSnapshot } from "../../types";
import { describeLiveDisplay } from "../../liveDisplay";
import { Menu } from "lucide-react";
import { useEffect, useState } from "react";

/** How long a touched parameter stays on the window after the last change:
 * LITTLE's header holds it as long. */
const PARAMETER_TOUCH_HOLD_MS = 1_500;

/** The parameter a control is moving, while it moves and for a moment after:
 * each change replaces the last at once, and the hold counts from the last. */
function useParameterTouchLine() {
  const [line, setLine] = useState<{ name: string; value: string } | null>(null);
  useEffect(() => {
    let clear: number | undefined;
    const unsubscribe = subscribeParameterTouches((touch) => {
      setLine(parameterTouchLine(touch));
      window.clearTimeout(clear);
      clear = window.setTimeout(() => setLine(null), PARAMETER_TOUCH_HOLD_MS);
    });
    return () => {
      unsubscribe();
      window.clearTimeout(clear);
    };
  }, []);
  return line;
}

function modeLabel(mode: SessionSnapshot["active_mode"] | undefined): string {
  switch (mode) {
    case "live":
      return "Live Mode";
    case "play":
      return "Play Mode";
    case "idle":
      return "Idle";
    default:
      return "Connecting";
  }
}

export function TopBar({
  snapshot,
  performance = null,
  menuOpen,
  onMenu,
}: {
  snapshot: SessionSnapshot | null;
  performance?: PerformanceSnapshot | null;
  menuOpen: boolean;
  onMenu: () => void;
}) {
  const active = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  );
  const selected = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
  // In LIVE the window says where the stage is, not which plugin: RACK, or
  // the song (and the setlist it is in), then the Rack and its part.
  const live = snapshot?.active_mode === "live" ? describeLiveDisplay(performance) : null;
  // On a narrow screen the bar shows what is playing or the master volume
  // and pan, not both: OUT switches between them. Wider, both fit and the
  // switch does nothing (see the faceplate).
  const hasMixerToggle = !isVstHost();
  const [mixerOpen, setMixerOpen] = useState(false);
  const touched = useParameterTouchLine();
  return (
    <header
      className={`topbar${hasMixerToggle ? " has-mixer-toggle" : ""}${
        mixerOpen ? " mixer-open" : ""
      }`}
    >
      <button
        className="mobile-menu-button"
        onClick={onMenu}
        aria-label="Open RackForge menu"
        aria-expanded={menuOpen}
      >
        <Menu aria-hidden="true" />
      </button>
      <div className={`now-playing${touched ? " touching" : ""}`}>
        {/* A control moving a parameter takes the whole first line -- the
            mode, and on a narrow screen the plugin beside it -- as LITTLE's
            header does, and gives it back once the control rests. */}
        {touched ? (
          <span className="now-playing-touch" role="status">
            <span className="now-playing-touch-name">{touched.name}</span>
            <span className="now-playing-touch-value">{touched.value}</span>
          </span>
        ) : null}
        {/* Which mode the host is in, where "Now playing" used to say
            nothing the program name below did not. */}
        <span className="eyebrow">{modeLabel(snapshot?.active_mode)}</span>
        {/* Mode, then plugin, then program: the same order at every size,
            so it is the markup's order and no layout rearranges it. */}
        {live ? (
          <>
            <span className="muted-inline">{live.context}</span>
            <strong>{live.name}</strong>
          </>
        ) : (
          <>
            {active && <span className="muted-inline">{active.plugin_name}</span>}
            <strong>{selected?.name ?? "Waiting for Core"}</strong>
          </>
        )}
      </div>
      <div className="top-controls" id="topbar-mixer">
        {!isVstHost() ? <MasterPan value={snapshot?.master_pan ?? 0} /> : null}
        <MasterLevel value={snapshot?.master_level ?? 0} />
        {hasMixerToggle ? (
          <MasterOutputMeter
            toggle={{ expanded: mixerOpen, onToggle: () => setMixerOpen((open) => !open) }}
          />
        ) : null}
      </div>
    </header>
  );
}
