import { useId, type CSSProperties } from "react";
import { X } from "lucide-react";
import { useCanvasModal } from "../hooks/useCanvasModal";
import { rackPluginsOfRole, type RackPluginRole } from "../rackPluginSelection";
import type { PluginInstance, PluginWebDescriptor } from "../types";

/**
 * Chooses the plugin a Rack node will own, as a modal over the Rack
 * editor's canvas like every other edit there -- centred, over the scrim,
 * the graph behind inert.
 *
 * The same picker serves both roles, because picking a pedalboard is the same
 * act as picking a piano — but it says which one it is asking for, in its
 * colour, and offers only plugins that can do that job. An effect list that
 * quietly included instruments would let someone build a Rack whose audio
 * input feeds a synthesizer.
 */
export function RackPluginPicker({
  instances,
  plugins,
  role,
  onSelect,
  onClose,
}: {
  instances: PluginInstance[];
  plugins: PluginWebDescriptor[];
  role: RackPluginRole;
  onSelect: (instance: PluginInstance) => void;
  onClose: () => void;
}) {
  const { sectionRef, closeRef, onKeyDown } = useCanvasModal(onClose);
  const titleId = useId();
  const descriptors = new Map(plugins.map((plugin) => [plugin.plugin_id, plugin]));
  const choices = rackPluginsOfRole(instances, plugins, role);
  const isEffect = role === "effect";
  return (
    <>
      <div className="rack-link-editor-scrim" aria-hidden="true" onPointerDown={(event) => event.stopPropagation()} />
      <section
        ref={sectionRef}
        className={`rack-plugin-picker rack-instrument-picker-dialog ${isEffect ? "effect" : "instrument"}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={onKeyDown}
        onPointerDown={(event) => event.stopPropagation()}
      >
        <header>
          <div>
            <span>{isEffect ? "Add an effect" : "Add an instrument"}</span>
            <strong id={titleId}>{isEffect ? "Choose an effect" : "Choose an instrument"}</strong>
          </div>
          <button
            ref={closeRef}
            type="button"
            className="rack-plugin-editor-close"
            aria-label={isEffect ? "Close effect selector" : "Close instrument selector"}
            title="Close (Esc)"
            onClick={onClose}
          >
            <X aria-hidden="true" />
          </button>
        </header>
        <div className="rack-plugin-picker-scroll">
          <p className="rack-instrument-picker-help">
            {isEffect
              ? "Select the effect this node will own. It goes into the chain before the output, or where the cable was dropped."
              : "Select the plugin this node will own. The current PLAY instrument is not changed."}
          </p>
          <div className="play-plugin-selector modal-list rack-instrument-picker-list" role="list">
            {choices.map((instance, index) => {
              const plugin = descriptors.get(instance.plugin_id);
              const branding = plugin?.branding;
              return (
                <button
                  type="button"
                  className={"plugin-picker-card" + (branding ? " branded" : "")}
                  key={instance.plugin_id}
                  onClick={() => onSelect(instance)}
                  style={branding ? {
                    "--plugin-accent": branding.accent_color,
                    "--plugin-background": branding.background_color,
                  } as CSSProperties : undefined}
                >
                  {branding ? (
                    <>
                      <img className="plugin-picker-banner" src={branding.banner_url} alt="" />
                      <span className="plugin-picker-shade" aria-hidden="true" />
                    </>
                  ) : null}
                  <span className="play-plugin-number">
                    {String(index + 1).padStart(2, "0")}
                  </span>
                  {branding ? (
                    <img className="plugin-picker-icon" src={branding.icon_url} alt="" />
                  ) : (
                    <span className="plugin-picker-icon rack-instrument-fallback-icon">RF</span>
                  )}
                  <span className="play-plugin-copy">
                    <strong>{instance.plugin_name}</strong>
                    <small>{plugin ? "v" + plugin.version : instance.plugin_id}</small>
                  </span>
                  <span className="play-plugin-status">
                    ADD <i aria-hidden="true">→</i>
                  </span>
                </button>
              );
            })}
            {choices.length === 0 ? (
              <div className="config-library-empty">
                <strong>{isEffect ? "No active effects" : "No active instruments"}</strong>
                <p>
                  Activate {isEffect ? "an effect" : "an instrument"} from Plugin Manager
                  before adding it to a Rack.
                </p>
              </div>
            ) : null}
          </div>
        </div>
        <footer>
          <button type="button" className="rack-plugin-picker-cancel" onClick={onClose}>Cancel</button>
        </footer>
      </section>
    </>
  );
}
