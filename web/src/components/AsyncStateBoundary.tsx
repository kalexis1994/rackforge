import type { ReactNode } from "react";
import { AsyncSpinner } from "./AsyncSpinner";
import { RfLoader, type RfLoaderSize } from "./RfLoader";
import { RfButton } from "../ui/RfButton";

export type AsyncStateStatus = "idle" | "loading" | "ready" | "error";
export type AsyncNoticeTone = "pending" | "success" | "error" | "info";

export function AsyncNotice({
  tone,
  title,
  children,
  onDismiss,
}: {
  tone: AsyncNoticeTone;
  title: string;
  children?: ReactNode;
  onDismiss?: () => void;
}) {
  const pending = tone === "pending";
  return (
    <div
      className={`rf-async-notice rf-async-notice--${tone}`}
      role={tone === "error" ? "alert" : "status"}
      aria-live={tone === "error" ? "assertive" : "polite"}
    >
      <span className="rf-async-notice__indicator" aria-hidden="true">
        {pending ? <AsyncSpinner label={title} /> : tone === "success" ? "✓" : tone === "error" ? "!" : "i"}
      </span>
      <span className="rf-async-notice__copy">
        <strong>{title}</strong>
        {children ? <small>{children}</small> : null}
      </span>
      {onDismiss ? (
        <RfButton
          variant="ghost"
          size="compact"
          iconOnly
          haptic="none"
          className="rf-async-notice__dismiss"
          onClick={onDismiss}
          aria-label={`Dismiss ${title}`}
        >
          ×
        </RfButton>
      ) : null}
    </div>
  );
}

export function AsyncStateBoundary({
  status,
  hasContent,
  loadingLabel,
  loadingDetail,
  errorTitle,
  errorDetail,
  onRetry,
  loaderSize = "medium",
  className = "",
  children,
}: {
  status: AsyncStateStatus;
  hasContent: boolean;
  loadingLabel: string;
  loadingDetail?: string;
  errorTitle: string;
  errorDetail?: string | null;
  onRetry?: () => void;
  loaderSize?: RfLoaderSize;
  className?: string;
  children: ReactNode;
}) {
  const pending = status === "idle" || status === "loading";
  const classes = ["rf-async-boundary", className].filter(Boolean).join(" ");

  if (pending && !hasContent) {
    return (
      <div className={`${classes} rf-async-boundary--initial`} aria-busy="true">
        <RfLoader label={loadingLabel} detail={loadingDetail} size={loaderSize} />
      </div>
    );
  }

  if (status === "error" && !hasContent) {
    return (
      <div className={`${classes} rf-async-boundary--initial rf-async-boundary--error`} role="alert">
        <div className="rf-async-boundary__error-copy">
          <span aria-hidden="true">!</span>
          <div>
            <strong>{errorTitle}</strong>
            {errorDetail ? <small>{errorDetail}</small> : null}
          </div>
        </div>
        {onRetry ? <RfButton onClick={onRetry}>Retry</RfButton> : null}
      </div>
    );
  }

  return (
    <div className={classes} aria-busy={pending || undefined}>
      {children}
      {pending ? (
        <div className="rf-async-boundary__overlay">
          <AsyncNotice tone="pending" title={loadingLabel}>{loadingDetail}</AsyncNotice>
        </div>
      ) : status === "error" ? (
        <div className="rf-async-boundary__overlay">
          <AsyncNotice tone="error" title={errorTitle}>
            {errorDetail}
            {onRetry ? (
              <RfButton size="compact" haptic="none" onClick={onRetry}>Retry</RfButton>
            ) : null}
          </AsyncNotice>
        </div>
      ) : null}
    </div>
  );
}
