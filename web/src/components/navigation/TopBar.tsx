import { MasterLevel, MasterOutputMeter, MasterPan } from "../../components/MasterSection";
import { isVstHost } from "../../host";
import { type SessionSnapshot } from "../../types";
import { Menu } from "lucide-react";
import { useState } from "react";

export function TopBar({
  snapshot,
  menuOpen,
  onMenu,
}: {
  snapshot: SessionSnapshot | null;
  menuOpen: boolean;
  onMenu: () => void;
}) {
  const active = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  );
  const selected = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
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
        <span className="eyebrow">Now playing</span>
        <strong>{selected?.name ?? "Waiting for Core"}</strong>
        {active && <span className="muted-inline">{active.plugin_name}</span>}
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
