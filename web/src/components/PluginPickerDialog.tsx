import type { CSSProperties } from "react";
import { ModalDialog } from "./ModalDialog";
import { rackPluginsOfRole, type RackPluginRole } from "../rackPluginSelection";
import type { PluginInstance, PluginWebDescriptor } from "../types";

/**
 * Chooses the plugin a Rack node will own.
 *
 * The same dialog serves both roles, because picking a pedalboard is the same
 * act as picking a piano — but it says which one it is asking for, and offers
 * only plugins that can do that job. An effect list that quietly included
 * instruments would let someone build a Rack whose audio input feeds a
 * synthesizer.
 */
export function PluginPickerDialog({
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
  const descriptors = new Map(plugins.map((plugin) => [plugin.plugin_id, plugin]));
  const choices = rackPluginsOfRole(instances, plugins, role);
  const isEffect = role === "effect";
  return (
    <ModalDialog
      eyebrow="Rack graph"
      title={isEffect ? "Choose an effect" : "Choose an instrument"}
      onClose={onClose}
      closeLabel={isEffect ? "Close effect selector" : "Close instrument selector"}
      className="rack-instrument-picker-dialog"
      actions={
        <button type="button" className="secondary-button" onClick={onClose}>
          Cancel
        </button>
      }
    >
      <p className="rack-instrument-picker-help">
        {isEffect
          ? "Select the effect this node will own. It is wired from the audio input, so whatever is plugged in runs through it."
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
    </ModalDialog>
  );
}
