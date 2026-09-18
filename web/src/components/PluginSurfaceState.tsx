import { BrandMark } from "../components/BrandMark";

export function PluginSurfaceState({
  title,
  detail,
}: {
  title: string;
  detail: string;
}) {
  return (
    <div className="plugin-surface-state">
      {/* Same two-letter stand-in the empty bay carried, from before the mark
          existed. */}
      <span className="plugin-surface-state-mark"><BrandMark /></span>
      <div>
        <h2>{title}</h2>
        <p>{detail}</p>
      </div>
    </div>
  );
}
