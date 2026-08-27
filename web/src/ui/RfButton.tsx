import {
  forwardRef,
  type ButtonHTMLAttributes,
  type MouseEvent,
} from "react";
import { AsyncSpinner } from "../components/AsyncSpinner";
import { hostHaptic } from "../host";

export type RfButtonVariant = "primary" | "secondary" | "danger" | "ghost";
export type RfButtonSize = "compact" | "default";

export interface RfButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: RfButtonVariant;
  size?: RfButtonSize;
  busy?: boolean;
  busyLabel?: string;
  iconOnly?: boolean;
  haptic?: "tap" | "confirm" | "none";
}

/** RackForge's canonical action control. */
export const RfButton = forwardRef<HTMLButtonElement, RfButtonProps>(function RfButton(
  {
    variant = "secondary",
    size = "default",
    busy = false,
    busyLabel = "Working…",
    iconOnly = false,
    haptic = "tap",
    className = "",
    disabled,
    children,
    onClick,
    type = "button",
    ...props
  },
  ref,
) {
  const classes = [
    "rf-button",
    `rf-button--${variant}`,
    `rf-button--${size}`,
    iconOnly ? "rf-button--icon" : "",
    className,
  ].filter(Boolean).join(" ");
  const unavailable = disabled || busy;
  const handleClick = (event: MouseEvent<HTMLButtonElement>) => {
    if (unavailable) return;
    if (haptic !== "none") hostHaptic(haptic);
    onClick?.(event);
  };

  return (
    <button
      {...props}
      ref={ref}
      type={type}
      className={classes}
      disabled={unavailable}
      aria-busy={busy || undefined}
      data-rf-press-feedback="local"
      onClick={handleClick}
    >
      {busy ? (
        <span className="rf-button__busy">
          <AsyncSpinner label={busyLabel} />
          <span>{busyLabel}</span>
        </span>
      ) : children}
    </button>
  );
});
