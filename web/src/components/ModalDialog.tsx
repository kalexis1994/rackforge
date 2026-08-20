import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
} from "react";

export interface ModalDialogProps {
  title: ReactNode;
  eyebrow?: ReactNode;
  message?: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
  onClose: () => void;
  dismissible?: boolean;
  showClose?: boolean;
  closeLabel?: string;
  role?: "dialog" | "alertdialog";
  className?: string;
  backdropClassName?: string;
}

/**
 * RackForge's common modal shell. It owns keyboard/backdrop dismissal, focus
 * restoration, scroll locking, accessible labelling and responsive structure;
 * callers only provide the dialog-specific content and actions.
 */
export function ModalDialog({
  title,
  eyebrow,
  message,
  children,
  actions,
  onClose,
  dismissible = true,
  showClose = true,
  closeLabel = "Close dialog",
  role = "dialog",
  className = "",
  backdropClassName = "",
}: ModalDialogProps) {
  const titleId = useId();
  const descriptionId = useId();
  const dialogRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);
  const dismissibleRef = useRef(dismissible);

  useEffect(() => {
    onCloseRef.current = onClose;
    dismissibleRef.current = dismissible;
  }, [dismissible, onClose]);

  useEffect(() => {
    const previousOverflow = document.body.style.overflow;
    const previousFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    document.body.style.overflow = "hidden";
    dialogRef.current?.focus({ preventScroll: true });

    const closeOnEscape = (event: KeyboardEvent) => {
      if (dismissibleRef.current && event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(
          'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => !element.hidden && element.getClientRects().length > 0);
      if (focusable.length === 0) {
        event.preventDefault();
        dialogRef.current.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (document.activeElement === dialogRef.current) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", closeOnEscape);
      previousFocus?.focus({ preventScroll: true });
    };
  }, []);

  return (
    <div
      className={`preset-modal-backdrop modal-dialog-backdrop ${backdropClassName}`.trim()}
      onPointerDown={(event) => {
        if (dismissible && event.target === event.currentTarget) onClose();
      }}
    >
      <section
        ref={dialogRef}
        className={`preset-modal modal-dialog ${className}`.trim()}
        role={role}
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={message ? descriptionId : undefined}
        tabIndex={-1}
      >
        <header className="preset-modal-header modal-dialog-header">
          <div className="modal-dialog-heading">
            {eyebrow ? <span className="eyebrow">{eyebrow}</span> : null}
            <h2 id={titleId}>{title}</h2>
          </div>
          {showClose ? (
            <button
              type="button"
              className="preset-modal-close modal-dialog-close"
              disabled={!dismissible}
              onClick={onClose}
              aria-label={closeLabel}
            >
              <span aria-hidden="true">×</span>
            </button>
          ) : null}
        </header>
        {message ? (
          <div className="modal-dialog-message" id={descriptionId}>
            {message}
          </div>
        ) : null}
        {children}
        {actions ? <footer className="modal-dialog-actions">{actions}</footer> : null}
      </section>
    </div>
  );
}
