import type { PluginWebDescriptor } from "../types";

/** A plugin's icon from its branding, or its initials on the accent when it has none. */
export function PluginIcon({
  plugin,
  name,
  className = "plugin-icon",
}: {
  plugin?: PluginWebDescriptor;
  name: string;
  className?: string;
}) {
  return plugin?.branding ? (
    <img className={className} src={plugin.branding.icon_url} alt="" />
  ) : (
    <span className={`${className} plugin-icon-fallback`} aria-hidden="true">
      {name.slice(0, 2).toUpperCase()}
    </span>
  );
}
