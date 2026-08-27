import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
} from "react";
import { RfButton } from "../ui/RfButton";

const FOCUSABLE_SELECTOR =
  'button:not(:disabled), [href], input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';
const modalStack: symbol[] = [];
let bodyScrollLocks = 0;
let bodyOverflowBeforeLock = "";

function lockBodyScroll() {
  if (bodyScrollLocks === 0) {
    bodyOverflowBeforeLock = document.body.style.overflow;
    document.body.style.overflow = "hidden";
  }
  bodyScrollLocks += 1;
  return () => {
    bodyScrollLocks = Math.max(0, bodyScrollLocks - 1);
    if (bodyScrollLocks === 0) document.body.style.overflow = bodyOverflowBeforeLock;
  };
}

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
  const modalIdentity = useRef(Symbol("rackforge-modal"));
  const backdropPressRef = useRef(false);
  const onCloseRef = useRef(onClose);
  const dismissibleRef = useRef(dismissible);

  useEffect(() => {
    onCloseRef.current = onClose;
    dismissibleRef.current = dismissible;
  }, [dismissible, onClose]);

  useEffect(() => {
    const identity = modalIdentity.current;
    modalStack.push(identity);
    const unlockBodyScroll = lockBodyScroll();
    const previousFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    dialogRef.current?.focus({ preventScroll: true });

    const closeOnEscape = (event: KeyboardEvent) => {
      if (modalStack.at(-1) !== identity) return;
      if (dismissibleRef.current && event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = Array.from(
        dialogRef.current.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
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
      const index = modalStack.lastIndexOf(identity);
      if (index >= 0) modalStack.splice(index, 1);
      unlockBodyScroll();
      window.removeEventListener("keydown", closeOnEscape);
      previousFocus?.focus({ preventScroll: true });
    };
  }, []);

  return (
    <div
      className={`preset-modal-backdrop modal-dialog-backdrop ${backdropClassName}`.trim()}
      onPointerDown={(event) => {
        backdropPressRef.current = event.target === event.currentTarget;
      }}
      onPointerUp={(event) => {
        const completedOnBackdrop = backdropPressRef.current && event.target === event.currentTarget;
        backdropPressRef.current = false;
        if (dismissible && completedOnBackdrop) onClose();
      }}
      onPointerCancel={() => {
        backdropPressRef.current = false;
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
            <RfButton
              variant="ghost"
              size="compact"
              iconOnly
              className="preset-modal-close modal-dialog-close"
              disabled={!dismissible}
              onClick={onClose}
              aria-label={closeLabel}
            >
              <span aria-hidden="true">×</span>
            </RfButton>
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
