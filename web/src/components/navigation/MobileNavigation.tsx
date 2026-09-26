import { useCallback, useEffect, useRef, useState } from "react";
import { ConnectionBadge } from "../../components/ConnectionBadge";
import { ControllerDockToggle } from "../../components/ControllerDockToggle";
import { LightingSwitch } from "../../components/LightingSwitch";
import { RevisionFooter } from "../../components/RevisionFooter";
import { NavigationLinks } from "../../components/navigation/NavigationLinks";
import { systemNavItems, vstSystemNavItems, vstWorkspaceNavItems, workspaceNavItems } from "../../components/navigation/navItems";
import { Activity, Blocks, LogOut, Play, Settings2, SlidersHorizontal, X } from "lucide-react";

export function MobileNavigation({
  vstHost,
  connection,
  onClose,
  performanceSurface,
  liveSetlistSelected,
  onPlayRequest,
  dockableController,
  controllerDockOpen,
  onControllerToggle,
  onPerformanceAction,
}: {
  vstHost: boolean;
  connection: string;
  onClose: () => void;
  performanceSurface?: "play" | "live";
  liveSetlistSelected: boolean;
  onPlayRequest: () => void;
  dockableController: boolean;
  controllerDockOpen: boolean;
  onControllerToggle: () => void;
  onPerformanceAction: (
    action: "select-plugin" | "effects" | "presets" | "live-perform" | "live-configure" | "live-exit-setlist" | "live-save-editor" | "live-close-editor",
  ) => void;
}) {
  const panelRef = useRef<HTMLElement | null>(null);
  const [closing, setClosing] = useState(false);
  const requestClose = useCallback(() => setClosing(true), []);
  useEffect(() => {
    if (!closing) return;
    const timeout = window.setTimeout(onClose, 190);
    return () => window.clearTimeout(timeout);
  }, [closing, onClose]);
  useEffect(() => {
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    panelRef.current?.focus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") requestClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [requestClose]);

  return (
    <div
      className={`mobile-menu-backdrop${closing ? " closing" : ""}`}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) requestClose();
      }}
    >
      <section
        ref={panelRef}
        className={`mobile-menu-panel${closing ? " closing" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="mobile-menu-title"
        tabIndex={-1}
      >
        <span className="mobile-menu-handle" aria-hidden="true" />
        <header>
          <div className="mobile-menu-heading">
            <h2 id="mobile-menu-title">RackForge</h2>
            <p>Instrument workspace</p>
          </div>
          <button onClick={requestClose} aria-label="Close RackForge menu">
            <X aria-hidden="true" />
          </button>
        </header>
        <div className="mobile-menu-scroll">
          {performanceSurface ? (
            <>
              <span className="mobile-menu-section">
                {performanceSurface === "play" ? "Play controls" : "Live workspace"}
              </span>
              <nav
                className="primary-nav mobile-menu-actions performance-menu-actions"
                aria-label={performanceSurface === "play" ? "Play controls" : "Live workspace"}
              >
                {performanceSurface === "play" ? (
                  <>
                    <button
                      className="nav-item"
                      onClick={() => onPerformanceAction("select-plugin")}
                    >
                      <span className="nav-mark"><Blocks aria-hidden="true" strokeWidth={1.9} /></span>
                      <span className="nav-copy">
                        <span>Select plugin</span>
                        <small>Choose the active instrument</small>
                      </span>
                    </button>
                    <button
                      className="nav-item"
                      onClick={() => onPerformanceAction("effects")}
                    >
                      <span className="nav-mark"><SlidersHorizontal aria-hidden="true" strokeWidth={1.9} /></span>
                      <span className="nav-copy">
                        <span>Effects</span>
                        <small>Show or hide the effects chain</small>
                      </span>
                    </button>
                    <button
                      className="nav-item"
                      onClick={() => onPerformanceAction("presets")}
                    >
                      <span className="nav-mark"><Activity aria-hidden="true" strokeWidth={1.9} /></span>
                      <span className="nav-copy">
                        <span>Presets</span>
                        <small>Load or manage plugin presets</small>
                      </span>
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      className="nav-item"
                      onClick={() => onPerformanceAction("live-perform")}
                    >
                      <span className="nav-mark"><Play aria-hidden="true" strokeWidth={1.9} /></span>
                      <span className="nav-copy">
                        <span>Perform</span>
                        <small>Open the stage-ready LIVE view</small>
                      </span>
                    </button>
                    <button
                      className="nav-item"
                      onClick={() => onPerformanceAction("live-configure")}
                    >
                      <span className="nav-mark"><Settings2 aria-hidden="true" strokeWidth={1.9} /></span>
                      <span className="nav-copy">
                        <span>Configure</span>
                        <small>Edit racks, songs, and setlists</small>
                      </span>
                    </button>
                    {liveSetlistSelected ? (
                      <button
                        className="nav-item"
                        onClick={() => onPerformanceAction("live-exit-setlist")}
                      >
                        <span className="nav-mark"><LogOut aria-hidden="true" strokeWidth={1.9} /></span>
                        <span className="nav-copy">
                          <span>Exit current Setlist</span>
                          <small>Return to the Setlist chooser without stopping audio</small>
                        </span>
                      </button>
                    ) : null}
                  </>
                )}
              </nav>
            </>
          ) : null}
          <span className="mobile-menu-section">Workspace</span>
          {/* Same reasoning as the rail: where the controller is a dock it is
              not a destination, so it leaves this list and becomes the switch
              below. In landscape it really is a route, and it stays here. */}
          <NavigationLinks
            items={(vstHost ? vstWorkspaceNavItems : workspaceNavItems).filter(
              (item) => !(dockableController && item.path === "/controller"),
            )}
            detailed
            onNavigate={requestClose}
            onPlayRequest={onPlayRequest}
          />
          <span className="mobile-menu-section">System</span>
          <NavigationLinks
            items={vstHost ? vstSystemNavItems : systemNavItems}
            detailed
            onNavigate={requestClose}
          />
          <RevisionFooter />
        </div>
        {/* The drawer is the only way to reach the lighting switch on a phone,
            where the rail that normally carries it is collapsed to icons. It
            shares the footer strip with the status badge: both are chassis
            furniture rather than workspace controls. */}
        {/* One plinth at the foot rather than a switch floating between the
            scroll area and the strip: everything here is chassis furniture and
            it all sits on the same ground. */}
        <div className="mobile-menu-footer">
          {dockableController ? (
            <ControllerDockToggle
              open={controllerDockOpen}
              onToggle={() => {
                onControllerToggle();
                requestClose();
              }}
            />
          ) : null}
          <div className="mobile-menu-footer-row">
            <LightingSwitch />
            <ConnectionBadge status={connection} />
          </div>
        </div>
      </section>
    </div>
  );
}
