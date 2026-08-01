export type RfLoaderSize = "compact" | "medium" | "large";

interface RfLoaderProps {
  label?: string;
  detail?: string;
  size?: RfLoaderSize;
  className?: string;
}

export function RfLoader({
  label = "RackForge",
  detail,
  size = "medium",
  className = "",
}: RfLoaderProps) {
  const classes = [
    "rf-async-loader",
    `rf-async-loader--${size}`,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={classes}
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <span className="rf-async-loader__mark" aria-hidden="true">
        <span className="rf-async-loader__letter rf-async-loader__letter--base">
          RF
        </span>
        <span className="rf-async-loader__letter rf-async-loader__letter--fill">
          RF
        </span>
        <span className="rf-async-loader__scanner" />
      </span>
      <span className="rf-async-loader__copy">
        <strong>{label}</strong>
        {detail && <small>{detail}</small>}
      </span>
    </div>
  );
}
