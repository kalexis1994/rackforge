import { MasterLevel, MasterOutputMeter, MasterPan } from "../../components/MasterSection";
import { isVstHost } from "../../host";
import { type PerformanceSnapshot, type SessionSnapshot } from "../../types";
import { describeLiveDisplay } from "../../liveDisplay";
import { Menu } from "lucide-react";
import { useState } from "react";

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
      <div className="now-playing">
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
