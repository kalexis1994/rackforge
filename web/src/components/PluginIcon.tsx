import { useState } from "react";
import type { PluginWebDescriptor } from "../types";
import { FadeImage } from "./FadeImage";

export function PluginIcon({
  plugin,
  name,
  className = "plugin-icon",
}: {
  plugin?: PluginWebDescriptor;
  name: string;
  className?: string;
}) {
  // An icon that cannot be loaded gives way to the plugin's initials, as a
  // plugin without artwork shows.
  const [failed, setFailed] = useState(false);
  return plugin?.branding && !failed ? (
    <FadeImage
      className={className}
      src={plugin.branding.icon_url}
      onFailed={() => setFailed(true)}
    />
  ) : (
    <span className={`${className} plugin-icon-fallback`} aria-hidden="true">
      {name.slice(0, 2).toUpperCase()}
    </span>
  );
}
