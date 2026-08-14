import type { ReactNode } from "react";

export type AsyncSpinnerSize = "small" | "medium" | "large";

interface AsyncSpinnerProps {
  label: string;
  size?: AsyncSpinnerSize;
  className?: string;
}

export function AsyncSpinner({
  label,
  size = "small",
  className = "",
}: AsyncSpinnerProps) {
  const classes = ["async-spinner", `async-spinner--${size}`, className]
    .filter(Boolean)
    .join(" ");

  return (
    <span className={classes} role="status" aria-label={label}>
      <span aria-hidden="true" />
    </span>
  );
}

export function AsyncActionLabel({
  active,
  activeLabel,
  children,
}: {
  active: boolean;
  activeLabel: string;
  children: ReactNode;
}) {
  return active ? (
    <span className="async-action-label">
      <AsyncSpinner label={activeLabel} />
      <span>{activeLabel}</span>
    </span>
  ) : (
    <>{children}</>
  );
}
