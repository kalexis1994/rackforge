import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
} from "react";
import { useSelector } from "react-redux";
import {
  Activity,
  Blocks,
  Download,
  FileUp,
  FolderOpen,
  House,
  Info,
  Menu,
  Piano,
  Play,
  RadioTower,
  Settings2,
  X,
} from "lucide-react";
import {
  NavLink,
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
  useParams,
} from "react-router";
import {
  connectGateway,
  deletePluginPreset,
  dispatchCommand,
  loadPluginPreset,
  requestPluginParameters,
  requestPluginPresets,
  renamePluginPreset,
  savePluginPreset,
  setPluginParameter,
  stopGateway,
} from "./gateway";
import { RfLoader } from "./components/RfLoader";
import { PerformanceInfoBar } from "./components/PerformanceInfoBar";
import {
  bindNativePluginResource,
  hostHaptic,
  hostJson,
  HostRequestError,
  isNativeHost,
  selectNativePluginSound,
  selectNativeResource,
  syncNativeRoute,
} from "./host";
import { LivePage } from "./LivePage";
import { TouchControllerPage } from "./TouchControllerPage";
import type { RootState } from "./store";
import type {
  PluginInstance,
  HostPresetSummary,
  HostAudioPreferences,
  HostAudioSettings,
  ProgramEditorField,
  ProgramEditorPage,
  ProgramEditorValue,
  PluginWebDescriptor,
  PluginWebSurfaceKind,
  PluginResourceRequirement,
  ResourceEntry,
  ResourceGrant,
  ResourceSelection,
  PluginRepositoryConfig,
  PluginRepositoryFile,
  SessionSnapshot,
  StoreCatalogResponse,
  WebAuthStatus,
  WebPublicConfig,
} from "./types";

const ResourceExplorerDialog = lazy(() =>
  import("./ResourceExplorerDialog").then((module) => ({
    default: module.ResourceExplorerDialog,
  })),
);

function BrandMark() {
  return (
    <span className="brand-mark" aria-hidden="true">
      <img src="/brand/rackforge-mark.svg" alt="" />
    </span>
  );
}

function PluginIcon({
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

async function postResourceApi<T>(url: string, body: unknown): Promise<T> {
  return hostJson<T>(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

function findEditorField(
  pages: ProgramEditorPage[],
  fieldId: string,
): ProgramEditorField | undefined {
  for (const page of pages) {
    const field = page.fields?.find((candidate) => candidate.id === fieldId);
    if (field) return field;
    const nested = findEditorField(page.pages ?? [], fieldId);
    if (nested) return nested;
  }
  return undefined;
}

function isProgramEditorValue(value: unknown): value is ProgramEditorValue {
  if (!value || typeof value !== "object" || !("type" in value)) return false;
  const candidate = value as { type: unknown; value?: unknown };
  switch (candidate.type) {
    case "inherited":
      return !("value" in candidate);
    case "boolean":
      return typeof candidate.value === "boolean";
    case "integer":
      return (
        typeof candidate.value === "number" &&
        Number.isSafeInteger(candidate.value)
      );
    case "choice":
    case "sound_id":
      return typeof candidate.value === "string";
    default:
      return false;
  }
}

const navItems = [
  {
    path: "/",
    label: "Home",
    detail: "Current instrument and system overview",
    section: "workspace",
    icon: House,
  },
  {
    path: "/live",
    label: "Live",
    detail: "Performance racks, songs and setlists",
    section: "workspace",
    icon: RadioTower,
  },
  {
    path: "/play",
    label: "Play",
    detail: "Play and edit the active instrument",
    section: "workspace",
    icon: Play,
  },
  {
    path: "/controller",
    label: "Touch Controller",
    detail: "On-screen keyboard and pads",
    section: "workspace",
    icon: Piano,
  },
  {
    path: "/plugins",
    label: "Plugins",
    detail: "Install, manage and configure instruments",
    section: "system",
    icon: Blocks,
  },
  {
    path: "/settings",
    label: "Settings",
    detail: "Audio, MIDI and host configuration",
    section: "system",
    icon: Settings2,
  },
];

const diagnosticsItem = {
  path: "/diagnostics",
  label: "Diagnostics",
  detail: "Connected audio, MIDI and USB devices",
  section: "workspace",
  icon: Activity,
};

const audioMidiItem = {
  path: "/settings",
  label: "Audio & MIDI",
  detail: "Output, latency, gain and controllers",
  section: "system",
  icon: Settings2,
};

const installedPluginsItem = {
  ...navItems[4],
  label: "Installed plugins",
  detail: "Manage versions and active instruments",
};

const aboutItem = {
  path: "/about",
  label: "About RackForge",
  detail: "Version and runtime information",
  section: "system",
  icon: Info,
};

const ROOMY_CONTROLLER_QUERY = "(min-width: 1100px) and (min-height: 620px)";

function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);

  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [query]);

  return matches;
}

export function App() {
  const [auth, setAuth] = useState<WebAuthStatus | null>(null);
  const [authError, setAuthError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const refresh = () =>
      hostJson<WebAuthStatus>("/api/v1/auth/status")
        .then((status) => {
          if (!cancelled) {
            setAuth(status);
            setAuthError(null);
          }
        })
        .catch(() => {
          if (!cancelled) setAuthError("RackForge Web is not responding.");
        });
    void refresh();
    const timer = window.setInterval(refresh, 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  if (authError) {
    return <AuthLoading message={authError} />;
  }
  if (!auth) {
    return <AuthLoading message="Connecting to RackForge…" />;
  }
  if (auth.requires_pin) {
    return (
      <PinGatePage
        key={`${auth.pin_state}:${auth.locked_for}`}
        status={auth}
        onUnlocked={() =>
          setAuth({ ...auth, unlocked: true, requires_pin: false })
        }
      />
    );
  }
  return <RackForgeApp />;
}

function RackForgeApp() {
  const { connection, snapshot, performance, performancePending, error } = useSelector(
    (state: RootState) => state.rackforge,
  );
  const location = useLocation();
  const navigate = useNavigate();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [installPluginOpen, setInstallPluginOpen] = useState(false);
  const [playOverlay, setPlayOverlay] = useState<"plugins" | "presets" | null>(null);
  const [liveSurface, setLiveSurface] = useState<"perform" | "configure">("perform");
  const [liveWorkspace, setLiveWorkspace] = useState<{ kind: "rack"; name: string } | null>(null);
  const [settingsBootstrap, setSettingsBootstrap] = useState<HostSettingsBootstrap | null>(null);
  const [controllerDockOpen, setControllerDockOpen] = useState(false);
  const roomyController = useMediaQuery(ROOMY_CONTROLLER_QUERY);
  const lastContentRoute = useRef(location.pathname === "/controller" ? "/play" : location.pathname);
  useTactileFeedback();
  const isControllerSurface = location.pathname === "/controller" && !roomyController;
  const isPluginSurface =
    location.pathname === "/play" ||
    location.pathname.startsWith("/plugins/");
  const isPerformanceSurface =
    location.pathname === "/play" || location.pathname === "/live";
  const isLiveSurface = location.pathname === "/live";
  const renderRackSlotPluginSurface = useCallback(
    ({
      instance,
      onSelectSound,
    }: {
      instance: PluginInstance;
      onSelectSound: (soundId: string) => Promise<unknown>;
    }) => (
      <PluginFrame
        instance={instance}
        surface="play"
        isolated
        onSelectSound={onSelectSound}
      />
    ),
    [],
  );

  useEffect(() => {
    connectGateway();
    return stopGateway;
  }, []);

  useEffect(() => {
    let active = true;
    requestHostSettingsBootstrap().then((bootstrap) => {
      if (active) setSettingsBootstrap(bootstrap);
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    syncNativeRoute(location.pathname);
  }, [location.pathname]);

  useEffect(() => {
    if (location.pathname !== "/controller") {
      lastContentRoute.current = location.pathname;
      return;
    }
    if (roomyController) {
      let active = true;
      window.queueMicrotask(() => {
        if (!active) return;
        setControllerDockOpen(true);
        navigate(lastContentRoute.current, { replace: true });
      });
      return () => {
        active = false;
      };
    }
  }, [location.pathname, navigate, roomyController]);

  const showControllerDock = roomyController && controllerDockOpen;

  return (
    <div className={`app-shell${isPluginSurface ? " plugin-surface-active" : ""}${
      isControllerSurface ? " controller-surface-active" : ""
    }${isPerformanceSurface ? " performance-surface-active" : ""}${
      isLiveSurface ? " live-surface-active" : ""
    }${liveWorkspace ? " rack-workspace-active" : ""
    }${showControllerDock ? " controller-dock-active" : ""
    }`}>
      <aside className="rail">
        <div className="brand-lockup" aria-label="RackForge">
          <BrandMark />
          <span className="brand-name">RACKFORGE</span>
        </div>
        <NavigationLinks
          controllerDockOpen={showControllerDock}
          onControllerToggle={roomyController
            ? () => setControllerDockOpen((open) => !open)
            : undefined}
        />
        {liveWorkspace ? (
          <nav className="rail-context-actions" aria-label="Rack editor actions">
            <span className="rail-context-label">Rack editor</span>
            <button
              type="button"
              className="nav-item"
              onClick={() => window.dispatchEvent(new Event("rackforge:save-rack-workspace"))}
            >
              <span className="nav-mark"><Activity aria-hidden="true" strokeWidth={1.9} /></span>
              <span className="nav-copy"><span>Save Rack</span></span>
            </button>
            <button
              type="button"
              className="nav-item"
              onClick={() => window.dispatchEvent(new Event("rackforge:close-rack-workspace"))}
            >
              <span className="nav-mark"><Blocks aria-hidden="true" strokeWidth={1.9} /></span>
              <span className="nav-copy"><span>Back to LIVE</span></span>
            </button>
          </nav>
        ) : null}
        <ConnectionBadge status={connection} />
      </aside>

      <main className="workspace">
        {!isControllerSurface ? (
          <TopBar
            snapshot={snapshot}
            menuOpen={mobileMenuOpen}
            onMenu={() => setMobileMenuOpen((open) => !open)}
          />
        ) : null}
        {isPerformanceSurface ? (
          <FloatingPerformanceMenuButton
            menuOpen={mobileMenuOpen}
            onOpen={() => setMobileMenuOpen(true)}
          />
        ) : null}
        {error && <div className="error-banner">{error}</div>}
        <div
          className={`page${isPluginSurface ? " plugin-host-page" : ""}${
            isControllerSurface ? " controller-host-page" : ""
          }${
            location.pathname === "/" ? " home-page" : ""
          }${
            isLiveSurface ? " live-host-page" : ""
          }`}
        >
          <Routes>
            <Route path="/" element={<HomePage snapshot={snapshot} />} />
            <Route
              path="/live"
              element={
                <LivePage
                  session={snapshot}
                  performance={performance}
                  pending={performancePending}
                  surface={liveSurface}
                  onSurfaceChange={setLiveSurface}
                  onWorkspaceChange={setLiveWorkspace}
                  renderPluginSurface={renderRackSlotPluginSurface}
                />
              }
            />
            <Route
              path="/play"
              element={
                <PlayPage
                  snapshot={snapshot}
                  overlay={playOverlay}
                  onOverlayChange={setPlayOverlay}
                />
              }
            />
            <Route
              path="/controller"
              element={
                <TouchControllerPage
                  snapshot={snapshot}
                  connection={connection}
                  onOpenNavigation={() => setMobileMenuOpen(true)}
                  onExit={() => navigate("/play")}
                />
              }
            />
            <Route
              path="/plugins"
              element={<PluginsPage snapshot={snapshot} />}
            />
            <Route
              path="/plugins/:instanceId"
              element={<PluginPage snapshot={snapshot} />}
            />
            <Route
              path="/settings"
              element={settingsBootstrap ? (
                <SettingsPage
                  initial={settingsBootstrap}
                  onConfigChange={(config) => setSettingsBootstrap((current) =>
                    current ? { ...current, config } : current
                  )}
                  onRepositoriesChange={(repositoryFile) => setSettingsBootstrap((current) =>
                    current ? { ...current, repositoryFile } : current
                  )}
                  onAudioChange={(audioSettings) => setSettingsBootstrap((current) =>
                    current ? { ...current, audioSettings } : current
                  )}
                />
              ) : (
                <RfLoader label="Settings" detail="Reading host capabilities…" size="medium" />
              )}
            />
            <Route path="/diagnostics" element={<DiagnosticsPage />} />
            <Route path="/about" element={<AboutPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </div>
        {showControllerDock ? (
          <div className="global-controller-dock">
            <TouchControllerPage
              snapshot={snapshot}
              connection={connection}
              onOpenNavigation={() => setMobileMenuOpen(true)}
              onExit={() => setControllerDockOpen(false)}
              docked
            />
          </div>
        ) : null}
      </main>
      {mobileMenuOpen ? (
        <MobileNavigation
          connection={connection}
          onClose={() => setMobileMenuOpen(false)}
          onInstall={() => {
            setMobileMenuOpen(false);
            setInstallPluginOpen(true);
          }}
          performanceSurface={
            location.pathname === "/play"
              ? "play"
              : location.pathname === "/live"
                ? "live"
                : undefined
          }
          liveWorkspaceActive={liveWorkspace !== null}
          onPerformanceAction={(action) => {
            setMobileMenuOpen(false);
            if (action === "select-plugin") setPlayOverlay("plugins");
            if (action === "presets") setPlayOverlay("presets");
            if (action === "live-perform") setLiveSurface("perform");
            if (action === "live-configure") setLiveSurface("configure");
            if (action === "live-save-editor") {
              window.dispatchEvent(new Event("rackforge:save-rack-workspace"));
            }
            if (action === "live-close-editor") {
              window.dispatchEvent(new Event("rackforge:close-rack-workspace"));
            }
          }}
        />
      ) : null}
      {installPluginOpen ? (
        <InstallPluginDialog onClose={() => setInstallPluginOpen(false)} />
      ) : null}
    </div>
  );
}

const PERFORMANCE_MENU_POSITION_KEY = "rackforge.performance-menu-position.v1";

function readPerformanceMenuPosition() {
  try {
    const saved = JSON.parse(localStorage.getItem(PERFORMANCE_MENU_POSITION_KEY) ?? "null") as {
      x?: number;
      y?: number;
    } | null;
    return {
      x: Math.min(1, Math.max(0, saved?.x ?? 0)),
      y: Math.min(1, Math.max(0, saved?.y ?? 0)),
    };
  } catch {
    return { x: 0, y: 0 };
  }
}

function FloatingPerformanceMenuButton({
  menuOpen,
  onOpen,
}: {
  menuOpen: boolean;
  onOpen: () => void;
}) {
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const [position, setPosition] = useState(readPerformanceMenuPosition);
  const positionRef = useRef(position);
  const [dragging, setDragging] = useState(false);
  const gestureRef = useRef<{
    pointerId: number;
    startX: number;
    startY: number;
    grabX: number;
    grabY: number;
    armed: boolean;
    moved: boolean;
    timer: number;
  } | null>(null);
  const suppressClickRef = useRef(false);
  const finishGestureRef = useRef<() => void>(() => undefined);
  const dragCueAnimationRef = useRef<Animation | null>(null);

  const stopDragCue = useCallback(() => {
    dragCueAnimationRef.current?.cancel();
    dragCueAnimationRef.current = null;
  }, []);

  const startDragCue = useCallback(() => {
    stopDragCue();
    const button = buttonRef.current;
    if (!button) return;
    dragCueAnimationRef.current = button.animate(
      [
        {
          color: "var(--acid)",
          backgroundColor: "rgba(4, 17, 26, 0.84)",
          borderColor: "rgba(85, 231, 255, 0.34)",
          boxShadow: "0 8px 26px rgba(0, 3, 7, 0.38)",
          transform: "scale(1)",
        },
        {
          color: "#021016",
          backgroundColor: "var(--acid)",
          borderColor: "#baf7ff",
          boxShadow:
            "0 0 0 7px rgba(85, 231, 255, 0.17), 0 16px 42px rgba(28, 211, 239, 0.42)",
          transform: "scale(1.2)",
        },
      ],
      {
        duration: 180,
        easing: "cubic-bezier(0.2, 0.82, 0.2, 1)",
        fill: "forwards",
      },
    );
  }, [stopDragCue]);

  const moveGesture = useCallback((pointerId: number, clientX: number, clientY: number) => {
    const gesture = gestureRef.current;
    const button = buttonRef.current;
    if (!gesture || gesture.pointerId !== pointerId || !button) return;
    const distance = Math.hypot(clientX - gesture.startX, clientY - gesture.startY);
    if (!gesture.armed) {
      if (distance > 10) {
        gesture.moved = true;
        window.clearTimeout(gesture.timer);
      }
      return;
    }
    gesture.moved = true;
    const shell = button.closest(".app-shell")?.getBoundingClientRect();
    if (!shell) return;
    const availableX = Math.max(1, shell.width - button.offsetWidth - 16);
    const availableY = Math.max(1, shell.height - button.offsetHeight - 16);
    const nextPosition = {
      x: Math.min(1, Math.max(0, (clientX - shell.left - gesture.grabX - 8) / availableX)),
      y: Math.min(1, Math.max(0, (clientY - shell.top - gesture.grabY - 8) / availableY)),
    };
    positionRef.current = nextPosition;
    setPosition(nextPosition);
  }, []);

  const finishGesture = useCallback((pointerId?: number) => {
    const gesture = gestureRef.current;
    if (!gesture || (pointerId !== undefined && gesture.pointerId !== pointerId)) return;
    window.clearTimeout(gesture.timer);
    stopDragCue();
    suppressClickRef.current = gesture.armed || gesture.moved;
    if (gesture.armed) {
      localStorage.setItem(PERFORMANCE_MENU_POSITION_KEY, JSON.stringify(positionRef.current));
    }
    gestureRef.current = null;
    setDragging(false);
    const button = buttonRef.current;
    if (button?.hasPointerCapture(gesture.pointerId)) {
      button.releasePointerCapture(gesture.pointerId);
    }
  }, [stopDragCue]);

  useEffect(() => {
    finishGestureRef.current = () => finishGesture();
    return () => {
      finishGestureRef.current = () => undefined;
    };
  }, [finishGesture]);

  useEffect(() => {
    const finishPointer = (event: PointerEvent) => finishGesture(event.pointerId);
    const finishAnyGesture = () => finishGesture();
    const finishWhenHidden = () => {
      if (document.visibilityState !== "visible") finishGesture();
    };
    window.addEventListener("pointerup", finishPointer, true);
    window.addEventListener("pointercancel", finishPointer, true);
    window.addEventListener("touchend", finishAnyGesture, true);
    window.addEventListener("touchcancel", finishAnyGesture, true);
    window.addEventListener("mouseup", finishAnyGesture, true);
    window.addEventListener("rackforge:native-touch-end", finishAnyGesture);
    window.addEventListener("blur", finishAnyGesture);
    document.addEventListener("visibilitychange", finishWhenHidden);
    return () => {
      window.removeEventListener("pointerup", finishPointer, true);
      window.removeEventListener("pointercancel", finishPointer, true);
      window.removeEventListener("touchend", finishAnyGesture, true);
      window.removeEventListener("touchcancel", finishAnyGesture, true);
      window.removeEventListener("mouseup", finishAnyGesture, true);
      window.removeEventListener("rackforge:native-touch-end", finishAnyGesture);
      window.removeEventListener("blur", finishAnyGesture);
      document.removeEventListener("visibilitychange", finishWhenHidden);
      const gesture = gestureRef.current;
      if (gesture) window.clearTimeout(gesture.timer);
      stopDragCue();
      gestureRef.current = null;
    };
  }, [finishGesture, stopDragCue]);

  return (
    <button
      ref={buttonRef}
      className={`performance-menu-button${dragging ? " dragging" : ""}`}
      style={{
        left: `calc(8px + ${position.x * 100}% - ${position.x * 60}px)`,
        top: `calc(8px + ${position.y * 100}% - ${position.y * 60}px)`,
      }}
      onPointerDown={(event) => {
        if (!event.isPrimary || event.button !== 0) return;
        const rect = event.currentTarget.getBoundingClientRect();
        const gesture = {
          pointerId: event.pointerId,
          startX: event.clientX,
          startY: event.clientY,
          grabX: event.clientX - rect.left,
          grabY: event.clientY - rect.top,
          armed: false,
          moved: false,
          timer: 0,
        };
        gesture.timer = window.setTimeout(() => {
          if (gestureRef.current !== gesture) return;
          gesture.armed = true;
          setDragging(true);
          startDragCue();
          hostHaptic("tap");
        }, 360);
        gestureRef.current = gesture;
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        moveGesture(event.pointerId, event.clientX, event.clientY);
      }}
      onPointerUp={(event) => finishGesture(event.pointerId)}
      onPointerCancel={(event) => finishGesture(event.pointerId)}
      onTouchEnd={() => finishGesture()}
      onTouchCancel={() => finishGesture()}
      onMouseUp={() => finishGesture()}
      onLostPointerCapture={(event) => finishGesture(event.pointerId)}
      onContextMenu={(event) => event.preventDefault()}
      onClick={() => {
        if (
          suppressClickRef.current ||
          gestureRef.current?.armed ||
          gestureRef.current?.moved
        ) {
          suppressClickRef.current = false;
          finishGesture();
          return;
        }
        onOpen();
      }}
      aria-label="Open RackForge menu. Hold and drag to move this button."
      aria-expanded={menuOpen}
      title="Tap to open · Hold and drag to move"
    >
      <Menu aria-hidden="true" />
    </button>
  );
}

function useTactileFeedback() {
  useEffect(() => {
    const DRAG_THRESHOLD_PX = 10;
    let gesture: {
      candidate: HTMLElement;
      pointerId: number;
      startX: number;
      startY: number;
      cancelled: boolean;
      pressTimer: number | null;
    } | null = null;
    const clearPressTimer = () => {
      if (gesture?.pressTimer !== null && gesture?.pressTimer !== undefined) {
        window.clearTimeout(gesture.pressTimer);
        gesture.pressTimer = null;
      }
    };
    const clearPressed = () => {
      clearPressTimer();
      gesture?.candidate.classList.remove("rf-pressed");
      gesture = null;
    };
    const cancelGesture = () => {
      if (!gesture) return;
      clearPressTimer();
      gesture.cancelled = true;
      gesture.candidate.classList.remove("rf-pressed");
    };
    const pointerDown = (event: PointerEvent) => {
      if (event.pointerType === "mouse" && event.button !== 0) return;
      const origin = event.target instanceof Element ? event.target : null;
      const candidate = origin?.closest<HTMLElement>(
        "button:not(:disabled), a[href], [role='button']:not([aria-disabled='true'])",
      );
      if (!candidate || !candidate.closest("#root")) return;
      // Piano keys and pads provide their own immediate pressed state. A
      // delayed expanding ripple obscures adjacent notes and feels sluggish
      // when playing quickly or gliding across the keyboard.
      if (candidate.closest(".touch-instrument, .performance-menu-button")) return;
      clearPressed();
      gesture = {
        candidate,
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        cancelled: false,
        pressTimer: null,
      };
      candidate.classList.add("rf-tactile");
      gesture.pressTimer = window.setTimeout(() => {
        if (gesture?.candidate === candidate && !gesture.cancelled) {
          candidate.classList.add("rf-pressed");
          gesture.pressTimer = null;
        }
      }, 70);
    };
    const pointerMove = (event: PointerEvent) => {
      if (!gesture || gesture.pointerId !== event.pointerId || gesture.cancelled) return;
      if (
        Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY) >
        DRAG_THRESHOLD_PX
      ) {
        cancelGesture();
      }
    };
    const pointerUp = (event: PointerEvent) => {
      if (!gesture || gesture.pointerId !== event.pointerId) return;
      const { candidate, cancelled } = gesture;
      clearPressTimer();
      candidate.classList.remove("rf-pressed");
      gesture = null;
      const releaseTarget = document.elementFromPoint(event.clientX, event.clientY);
      if (cancelled || !releaseTarget || !candidate.contains(releaseTarget)) return;
      const bounds = candidate.getBoundingClientRect();
      const localX = Math.min(bounds.width, Math.max(0, event.clientX - bounds.left));
      const localY = Math.min(bounds.height, Math.max(0, event.clientY - bounds.top));
      const diameter = Math.max(
        Math.hypot(localX, localY),
        Math.hypot(bounds.width - localX, localY),
        Math.hypot(localX, bounds.height - localY),
        Math.hypot(bounds.width - localX, bounds.height - localY),
      ) * 2;
      const ripple = document.createElement("span");
      ripple.className = "rf-touch-ripple";
      ripple.style.width = `${diameter}px`;
      ripple.style.height = `${diameter}px`;
      ripple.style.left = `${localX - diameter / 2}px`;
      ripple.style.top = `${localY - diameter / 2}px`;
      candidate.appendChild(ripple);
      ripple.addEventListener("animationend", () => ripple.remove(), { once: true });
      hostHaptic("tap");
    };
    document.addEventListener("pointerdown", pointerDown, { capture: true, passive: true });
    document.addEventListener("pointermove", pointerMove, { capture: true, passive: true });
    document.addEventListener("pointerup", pointerUp, { capture: true, passive: true });
    document.addEventListener("pointercancel", clearPressed, { capture: true, passive: true });
    document.addEventListener("scroll", cancelGesture, { capture: true, passive: true });
    window.addEventListener("blur", clearPressed);
    return () => {
      clearPressed();
      document.removeEventListener("pointerdown", pointerDown, true);
      document.removeEventListener("pointermove", pointerMove, true);
      document.removeEventListener("pointerup", pointerUp, true);
      document.removeEventListener("pointercancel", clearPressed, true);
      document.removeEventListener("scroll", cancelGesture, true);
      window.removeEventListener("blur", clearPressed);
    };
  }, []);
}

function NavigationLinks({
  items = navItems,
  detailed = false,
  onNavigate,
  controllerDockOpen = false,
  onControllerToggle,
}: {
  items?: typeof navItems;
  detailed?: boolean;
  onNavigate?: () => void;
  controllerDockOpen?: boolean;
  onControllerToggle?: () => void;
}) {
  return (
    <nav className="primary-nav" aria-label="RackForge sections">
      {items.map((item) => {
        const Icon = item.icon;
        if (item.path === "/controller" && onControllerToggle) {
          return (
            <button
              key={item.path}
              type="button"
              onClick={() => {
                onControllerToggle();
                onNavigate?.();
              }}
              className={`nav-item controller-toggle${controllerDockOpen ? " dock-open" : ""}`}
              aria-pressed={controllerDockOpen}
            >
              <span className="nav-mark">
                <Icon aria-hidden="true" strokeWidth={1.9} />
              </span>
              <span className="nav-copy">
                <span>{item.label}</span>
                {detailed ? <small>{item.detail}</small> : null}
              </span>
            </button>
          );
        }
        return (
          <NavLink
            key={item.path}
            to={item.path}
            end={item.path === "/"}
            onClick={onNavigate}
            className={({ isActive }) =>
              `nav-item${isActive ? " active" : ""}`
            }
          >
            <span className="nav-mark">
              <Icon aria-hidden="true" strokeWidth={1.9} />
            </span>
            <span className="nav-copy">
              <span>{item.label}</span>
              {detailed ? <small>{item.detail}</small> : null}
            </span>
          </NavLink>
        );
      })}
    </nav>
  );
}

function MobileNavigation({
  connection,
  onClose,
  onInstall,
  performanceSurface,
  liveWorkspaceActive,
  onPerformanceAction,
}: {
  connection: string;
  onClose: () => void;
  onInstall: () => void;
  performanceSurface?: "play" | "live";
  liveWorkspaceActive: boolean;
  onPerformanceAction: (
    action: "select-plugin" | "presets" | "live-perform" | "live-configure" | "live-save-editor" | "live-close-editor",
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
                    {liveWorkspaceActive ? (
                      <>
                        <button
                          className="nav-item"
                          onClick={() => onPerformanceAction("live-save-editor")}
                        >
                          <span className="nav-mark"><Activity aria-hidden="true" strokeWidth={1.9} /></span>
                          <span className="nav-copy">
                            <span>Save Rack</span>
                            <small>Store this portable node graph</small>
                          </span>
                        </button>
                        <button
                          className="nav-item"
                          onClick={() => onPerformanceAction("live-close-editor")}
                        >
                          <span className="nav-mark"><Blocks aria-hidden="true" strokeWidth={1.9} /></span>
                          <span className="nav-copy">
                            <span>Back to LIVE library</span>
                            <small>Close the full-screen node workspace</small>
                          </span>
                        </button>
                      </>
                    ) : null}
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
                  </>
                )}
              </nav>
            </>
          ) : null}
          <span className="mobile-menu-section">Workspace</span>
          <NavigationLinks
            items={[navItems[2], navItems[1], navItems[3], diagnosticsItem]}
            detailed
            onNavigate={requestClose}
          />
          <span className="mobile-menu-section">System</span>
          <NavigationLinks
            items={[audioMidiItem]}
            detailed
            onNavigate={requestClose}
          />
          <nav className="primary-nav mobile-menu-actions" aria-label="RackForge system actions">
            <button className="nav-item" onClick={onInstall}>
              <span className="nav-mark">
                <Download aria-hidden="true" strokeWidth={1.9} />
              </span>
              <span className="nav-copy">
                <span>Install plugin</span>
                <small>Choose a portable .rfplugin package</small>
              </span>
            </button>
          </nav>
          <NavigationLinks
            items={[installedPluginsItem, aboutItem]}
            detailed
            onNavigate={requestClose}
          />
        </div>
        <ConnectionBadge status={connection} />
      </section>
    </div>
  );
}

interface InstalledPluginResult {
  plugin_id: string;
  version: string;
  already_installed: boolean;
  activation_required: boolean;
}

const PLUGIN_ACTIVATION_TIMEOUT_MS = 45_000;

async function activateInstalledPlugin(
  result: InstalledPluginResult,
): Promise<PluginWebDescriptor> {
  const activation = await hostJson<{ status?: string }>(
    `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}/activate`,
    { method: "POST" },
  );
  if (activation.status === "active") {
    return hostJson<PluginWebDescriptor>(
      `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
    );
  }
  const startedAt = performance.now();
  let lastError: unknown;
  while (performance.now() - startedAt < PLUGIN_ACTIVATION_TIMEOUT_MS) {
    try {
      const descriptor = await hostJson<PluginWebDescriptor>(
        `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
      );
      if (descriptor.active) return descriptor;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => window.setTimeout(resolve, 250));
  }
  throw lastError instanceof Error
    ? lastError
    : new Error("RackForge installed the plugin but activation did not finish in time.");
}

const MAX_CLIENT_RESOURCE_BYTES = 512 * 1024 * 1024;

function InstallPluginDialog({ onClose }: { onClose: () => void }) {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const native = isNativeHost();
  const [browseHost, setBrowseHost] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const [installed, setInstalled] = useState<InstalledPluginResult | null>(null);
  const [activated, setActivated] = useState<PluginWebDescriptor | null>(null);
  const snapshot = useSelector((state: RootState) => state.rackforge.snapshot);
  const navigate = useNavigate();

  useEffect(() => {
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.style.overflow = previousOverflow;
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [busy, onClose]);

  const installSelection = async (selection: ResourceSelection) => {
    setStatus(`Validating ${selection.display_name}…`);
    return postResourceApi<InstalledPluginResult>("/api/v1/plugins/install", {
      selection_id: selection.selection_id,
    });
  };

  const finishInstall = async (result: InstalledPluginResult) => {
    setInstalled(result);
    setActivationError(null);
    setStatus(`Activating ${result.plugin_id}…`);
    try {
      const descriptor = await activateInstalledPlugin(result);
      setActivated(descriptor);
      hostHaptic("confirm");
    } catch (reason) {
      setActivationError(
        reason instanceof Error
          ? reason.message
          : "The plugin was installed but could not be activated.",
      );
    } finally {
      setStatus(null);
    }
  };

  const openNativePicker = async () => {
    setBusy(true);
    setError(null);
    setStatus("Opening file picker…");
    try {
      const selection = await selectNativeResource({
        kind: "file",
        extensions: [".rfplugin"],
      });
      await finishInstall(await installSelection(selection));
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not open the file picker.",
      );
    } finally {
      setBusy(false);
    }
  };

  const uploadClientFile = async (file: File) => {
    if (file.size === 0 || file.size > MAX_CLIENT_RESOURCE_BYTES) {
      setError("The package is empty or exceeds RackForge's 512 MB limit.");
      return;
    }
    setBusy(true);
    setError(null);
    setInstalled(null);
    setStatus(`Uploading ${file.name} to RackForge…`);
    try {
      const selection = await hostJson<ResourceSelection>(
        `/api/v1/resources/uploads?name=${encodeURIComponent(file.name)}`,
        {
          method: "POST",
          headers: { "content-type": "application/octet-stream" },
          body: file,
        },
      );
      await finishInstall(await installSelection(selection));
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not install this plugin.",
      );
    } finally {
      setBusy(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  const installHostEntry = async (entry: ResourceEntry) => {
    setBrowseHost(false);
    setBusy(true);
    setError(null);
    setInstalled(null);
    setStatus(`Selecting ${entry.name} on the RackForge host…`);
    try {
      const selection = await postResourceApi<ResourceSelection>(
        "/api/v1/resources/selections",
        { entry_id: entry.id },
      );
      await finishInstall(await installSelection(selection));
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not install this plugin.",
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="install-plugin-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <section
        className="install-plugin-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="install-plugin-title"
      >
        <header>
          <div>
            <span className="eyebrow">PORTABLE PACKAGE</span>
            <h2 id="install-plugin-title">Install plugin</h2>
          </div>
          <button className="icon-button" onClick={onClose} disabled={busy}>
            Close
          </button>
        </header>
        {!installed ? <p className="install-plugin-intro">
          {native
            ? "Select a portable .rfplugin package. RackForge validates it before installing anything."
            : "Choose where the .rfplugin package is located. RackForge validates it on the host before installing anything."}
        </p> : null}
        {!installed ? <div className="install-plugin-sources">
          <button
            type="button"
            className="install-source-card"
            disabled={busy}
            onClick={() =>
              native ? void openNativePicker() : fileInputRef.current?.click()
            }
          >
            <span className="install-source-icon"><FileUp aria-hidden="true" /></span>
            <span>
              <strong>{native ? "Choose plugin package" : "Upload from this device"}</strong>
              {!native ? (
                <small>Use the browser picker, then securely upload to the host</small>
              ) : null}
            </span>
          </button>
          {!native ? (
            <button
              type="button"
              className="install-source-card"
              disabled={busy}
              onClick={() => setBrowseHost(true)}
            >
              <span className="install-source-icon"><FolderOpen aria-hidden="true" /></span>
              <span>
                <strong>Browse the RackForge host</strong>
                <small>Select a package already stored on the host device</small>
              </span>
            </button>
          ) : null}
        </div> : null}
        <input
          ref={fileInputRef}
          className="visually-hidden"
          type="file"
          accept=".rfplugin,application/octet-stream"
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) void uploadClientFile(file);
          }}
        />
        {status ? <p className="install-plugin-status">{status}</p> : null}
        {error ? <p className="install-plugin-error">{error}</p> : null}
        {activationError ? (
          <p className="install-plugin-error">
            The package is installed, but activation failed: {activationError}
          </p>
        ) : null}
        {installed ? (
          <div className="install-plugin-complete">
            <div className="install-plugin-success" role="status">
              <strong>
                {activated
                  ? "Plugin installed and active"
                  : installed.already_installed
                    ? "Plugin already installed"
                    : "Plugin installed"}
              </strong>
              <span>{installed.plugin_id} v{installed.version}</span>
            </div>
            {!busy && !status ? (
              <div className="install-plugin-actions">
                {activated?.surfaces.some((surface) => surface.kind === "config") ? (
                  <button
                    className="primary-button"
                    onClick={() => {
                      const instance = snapshot?.instances.find(
                        (candidate) => candidate.plugin_id === installed.plugin_id,
                      );
                      navigate(
                        instance
                          ? `/plugins/${encodeURIComponent(instance.instance_id)}`
                          : "/plugins",
                      );
                      onClose();
                    }}
                  >
                    Open configuration
                  </button>
                ) : null}
                <button className="secondary-button" onClick={onClose}>
                  Close
                </button>
              </div>
            ) : null}
          </div>
        ) : null}
      </section>
      {browseHost ? (
        <Suspense
          fallback={
            <div className="resource-explorer-backdrop">
              <RfLoader label="RackForge storage" detail="Opening explorer…" />
            </div>
          }
        >
          <ResourceExplorerDialog
            mode="select"
            selection={{ name: "plugin package", kind: "file" }}
            onCancel={() => setBrowseHost(false)}
            onSelected={(entry) => void installHostEntry(entry)}
          />
        </Suspense>
      ) : null}
    </div>
  );
}

function AuthLoading({ message }: { message: string }) {
  return (
    <main className="pairing-shell">
      <RfLoader label="RackForge" detail={message} size="large" />
    </main>
  );
}

function PinGatePage({
  status,
  onUnlocked,
}: {
  status: WebAuthStatus;
  onUnlocked: () => void;
}) {
  const digits = status.pin_digits || 4;
  const enrolling = status.pin_state === "enrolling";
  const unclaimed = status.pin_state === "unclaimed";
  const [pin, setPin] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lockedFor, setLockedFor] = useState(status.locked_for);

  // The wait counts itself down rather than sitting on a stale number, so a
  // player standing in front of a locked device can see it clearing.
  useEffect(() => {
    if (lockedFor <= 0) return;
    const timer = window.setInterval(
      () => setLockedFor((left) => Math.max(0, left - 1)),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [lockedFor]);

  const complete =
    pin.length === digits && (!enrolling || confirmation === pin);
  const blocked = submitting || lockedFor > 0 || unclaimed;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!complete || blocked) return;
    setSubmitting(true);
    setError(null);
    const endpoint = enrolling ? "/api/v1/auth/pin" : "/api/v1/auth/unlock";
    hostJson<unknown>(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ pin }),
    })
      .then(() => onUnlocked())
      .catch((reason: Error) => {
        if (reason instanceof HostRequestError) {
          const payload = reason.payload as { locked_for?: unknown } | undefined;
          if (typeof payload?.locked_for === "number") {
            setLockedFor(payload.locked_for);
          }
        }
        setError(reason.message);
        setPin("");
        setConfirmation("");
      })
      .finally(() => setSubmitting(false));
  };

  const onlyDigits = (value: string) =>
    value.replace(/\D/g, "").slice(0, digits);

  return (
    <main className="pairing-shell">
      <section className="pairing-panel">
        <div className="pairing-brand">
          <BrandMark />
          <span>RACKFORGE</span>
        </div>
        <span className="eyebrow accent">Device access</span>
        <h1>{enrolling ? "Choose a PIN" : "Enter the PIN"}</h1>
        <p>
          {enrolling
            ? `This device has not been claimed yet. Pick a ${digits}-digit PIN and it will be needed from any browser from now on.`
            : unclaimed
              ? "No PIN has been set and this device no longer accepts one over the network. Set one from the machine itself, then reload."
              : "This device is protected by a PIN chosen when it was set up."}
        </p>
        <form onSubmit={submit}>
          <input
            value={pin}
            onChange={(event) => setPin(onlyDigits(event.target.value))}
            inputMode="numeric"
            autoComplete={enrolling ? "new-password" : "current-password"}
            placeholder={"0".repeat(digits)}
            aria-label={`${digits}-digit PIN`}
            disabled={blocked}
          />
          {enrolling && (
            <input
              value={confirmation}
              onChange={(event) =>
                setConfirmation(onlyDigits(event.target.value))
              }
              inputMode="numeric"
              autoComplete="new-password"
              placeholder="Repeat"
              aria-label="Repeat the PIN"
              disabled={blocked}
            />
          )}
          <button className="primary-button" disabled={!complete || blocked}>
            {lockedFor > 0
              ? `Wait ${lockedFor}s`
              : submitting
                ? "Checking…"
                : enrolling
                  ? "Set PIN"
                  : "Unlock"}
          </button>
        </form>
        {error && <div className="pairing-error">{error}</div>}
        <small>
          {enrolling
            ? "You can change it later from Settings."
            : "Wrong PINs are allowed a few times, then the wait grows."}
        </small>
      </section>
    </main>
  );
}

function ConnectionBadge({ status }: { status: string }) {
  const label =
    status === "online"
      ? "System ready"
      : status === "connecting"
        ? "Connecting"
        : "System offline";
  return (
    <div className={`connection-badge ${status}`}>
      <span className="status-dot" />
      <span>{label}</span>
    </div>
  );
}

function TopBar({
  snapshot,
  menuOpen,
  onMenu,
}: {
  snapshot: SessionSnapshot | null;
  menuOpen: boolean;
  onMenu: () => void;
}) {
  const active = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  );
  const selected = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
  return (
    <header className="topbar">
      <button
        className="mobile-menu-button"
        onClick={onMenu}
        aria-label="Open RackForge menu"
        aria-expanded={menuOpen}
      >
        <Menu aria-hidden="true" />
      </button>
      <div className="now-playing">
        <span className="eyebrow">Now playing</span>
        <strong>{selected?.name ?? "Waiting for Core"}</strong>
        {active && <span className="muted-inline">{active.plugin_name}</span>}
      </div>
      <div className="top-controls">
        <MasterPan value={snapshot?.master_pan ?? 0} />
        <MasterLevel value={snapshot?.master_level ?? 0} />
      </div>
    </header>
  );
}

function MasterLevel({ value }: { value: number }) {
  const [localValue, setLocalValue] = useState(value);
  const [dragging, setDragging] = useState(false);
  const displayedValue = dragging ? localValue : value;
  return (
    <label className="compact-control">
      <span>Master</span>
      <input
        type="range"
        min="0"
        max="1000"
        value={displayedValue}
        onPointerDown={() => {
          setLocalValue(value);
          setDragging(true);
        }}
        onPointerUp={() => setDragging(false)}
        onPointerCancel={() => setDragging(false)}
        onChange={(event) => {
          const level = Number(event.target.value);
          setLocalValue(level);
          dispatchCommand({ type: "set_master_level", level });
        }}
      />
      <output>{Math.round(displayedValue / 10)}%</output>
    </label>
  );
}

function MasterPan({ value }: { value: number }) {
  const [localValue, setLocalValue] = useState(value);
  const [dragging, setDragging] = useState(false);
  const displayedValue = dragging ? localValue : value;
  const display =
    displayedValue === 0
      ? "C"
      : `${displayedValue < 0 ? "L" : "R"}${Math.round(Math.abs(displayedValue) / 10)}`;
  return (
    <label className="compact-control pan-control">
      <span>Pan</span>
      <input
        type="range"
        min="-1000"
        max="1000"
        step="10"
        value={displayedValue}
        onPointerDown={() => {
          setLocalValue(value);
          setDragging(true);
        }}
        onPointerUp={() => setDragging(false)}
        onPointerCancel={() => setDragging(false)}
        onDoubleClick={() => {
          setLocalValue(0);
          dispatchCommand({ type: "set_master_pan", pan: 0 });
        }}
        onChange={(event) => {
          let pan = Number(event.target.value);
          if (Math.abs(pan) <= 70) pan = 0;
          setLocalValue(pan);
          dispatchCommand({ type: "set_master_pan", pan });
        }}
      />
      <output>{display}</output>
    </label>
  );
}

function PageHeading({
  eyebrow,
  title,
  detail,
}: {
  eyebrow: string;
  title: string;
  detail: string;
}) {
  return (
    <div className="page-heading">
      <span className="eyebrow accent">{eyebrow}</span>
      <h1>{title}</h1>
      <p>{detail}</p>
    </div>
  );
}

function HomePage({ snapshot }: { snapshot: SessionSnapshot | null }) {
  const active = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  );
  const selected = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
  const masterLevel = snapshot
    ? `${Math.round(snapshot.master_level / 10)}%`
    : "—";
  const masterPan = snapshot
    ? snapshot.master_pan === 0
      ? "CENTER"
      : `${snapshot.master_pan < 0 ? "L" : "R"}${Math.round(Math.abs(snapshot.master_pan) / 10)}`
    : "—";
  const navigate = useNavigate();
  return (
    <>
      <section className="hero-grid">
        <article className="hero-card sound-card">
          <span className="card-kicker">
            {snapshot?.active_mode.toUpperCase() ?? "OFFLINE"} MODE
          </span>
          <div>
            <h2>{selected?.name ?? "No program selected"}</h2>
            <p>{selected?.detail ?? active?.plugin_name ?? "Connect RackForge Core"}</p>
          </div>
          <button className="primary-button" onClick={() => navigate("/play")}>
            Open instrument <span>→</span>
          </button>
        </article>
        <article className="hero-card session-card">
          <span className="card-kicker">Live status</span>
          <dl className="session-stats">
            <div>
              <dt>Mode</dt>
              <dd>{snapshot?.active_mode.toUpperCase() ?? "—"}</dd>
            </div>
            <div>
              <dt>Master</dt>
              <dd>{masterLevel}</dd>
            </div>
            <div>
              <dt>Pan</dt>
              <dd>{masterPan}</dd>
            </div>
          </dl>
          <div className="signal-line">
            <span />
            <small>Controller and sound engine synchronized</small>
          </div>
        </article>
      </section>
      <section className="section-block">
        <div className="section-title-row">
          <div>
            <span className="eyebrow">Available</span>
            <h2>Instruments</h2>
          </div>
          <button className="text-button" onClick={() => navigate("/plugins")}>
            Browse →
          </button>
        </div>
        <PluginGrid instances={snapshot?.instances ?? []} />
      </section>
    </>
  );
}

function PlayPage({
  snapshot,
  overlay,
  onOverlayChange,
}: {
  snapshot: SessionSnapshot | null;
  overlay: "plugins" | "presets" | null;
  onOverlayChange: (overlay: "plugins" | "presets" | null) => void;
}) {
  const instances = snapshot?.instances ?? [];
  const active =
    instances.find(
      (instance) => instance.instance_id === snapshot?.active_instance_id,
    ) ?? instances[0];
  const pluginPickerOpen = overlay === "plugins";
  const presetsOpen = overlay === "presets";
  const [surfaceInfo, setSurfaceInfo] = useState<{
    instanceId: string;
    label: string;
    value: string;
  } | null>(null);
  const [installedPlugins, setInstalledPlugins] = useState<PluginWebDescriptor[]>([]);
  useEffect(() => {
    let cancelled = false;
    hostJson<PluginWebDescriptor[]>("/api/v1/plugins")
      .then((plugins) => {
        if (cancelled) return;
        setInstalledPlugins(plugins);
      })
      .catch(() => {
        if (!cancelled) setInstalledPlugins([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  const activeVersion = installedPlugins.find(
    (plugin) => plugin.plugin_id === active?.plugin_id,
  )?.version;
  const activeDescriptor = installedPlugins.find(
    (plugin) => plugin.plugin_id === active?.plugin_id,
  );
  const activeProgram = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
  const activeSurfaceInfo =
    surfaceInfo?.instanceId === active?.instance_id ? surfaceInfo : null;
  const activeInstanceId = active?.instance_id;
  const handleSurfaceInfo = useCallback(
    (info: { label: string; value: string } | null) => {
      if (!activeInstanceId) return;
      setSurfaceInfo(
        info
          ? { instanceId: activeInstanceId, ...info }
          : null,
      );
    },
    [activeInstanceId],
  );
  return (
    <section className="plugin-surface-shell direct-surface">
      <div className="play-plugin-toolbar">
        <button
          className={`play-header-button back${pluginPickerOpen ? " active" : ""}`}
          onClick={() => {
            onOverlayChange(pluginPickerOpen ? null : "plugins");
          }}
          aria-expanded={pluginPickerOpen}
        >
          <span aria-hidden="true">▦</span>
          <strong>Select plugin</strong>
        </button>
        <PerformanceInfoBar
          className="play-plugin-identity"
          left={{ label: "Mode", value: "PLAY" }}
          center={{
            label: activeSurfaceInfo?.label || "Program",
            value: activeSurfaceInfo?.value || activeProgram?.name || "No program",
          }}
          right={{
            label: "Plugin",
            value: active
              ? `${active.plugin_name}${formatPluginVersion(activeVersion)}`
              : "Select an instrument",
          }}
          rightAccessory={
            active ? (
              <PluginIcon
                plugin={activeDescriptor}
                name={active.plugin_name}
                className="play-plugin-icon"
              />
            ) : null
          }
        />
        <button
          className={`play-header-button presets${presetsOpen ? " active" : ""}`}
          disabled={!active}
          onClick={() => {
            onOverlayChange(presetsOpen ? null : "presets");
          }}
          aria-expanded={presetsOpen}
        >
          <span className="preset-button-mark" aria-hidden="true">P</span>
          <strong>Presets</strong>
        </button>
      </div>
      {active ? (
        <PluginFrame
          key={active.instance_id}
          instance={active}
          surface="play"
          onSurfaceInfoChange={handleSurfaceInfo}
        />
      ) : (
        <PluginSurfaceState
          title="No instrument active"
          detail="Select one of the installed RackForge plugins to start playing."
        />
      )}
      {pluginPickerOpen && (
        <PluginPickerModal
          active={active}
          instances={instances}
          plugins={installedPlugins}
          onClose={() => onOverlayChange(null)}
        />
      )}
      {presetsOpen && active && (
        <PresetModal instance={active} onClose={() => onOverlayChange(null)} />
      )}
    </section>
  );
}

function PluginPickerModal({
  active,
  instances,
  plugins,
  onClose,
}: {
  active: PluginInstance | undefined;
  instances: PluginInstance[];
  plugins: PluginWebDescriptor[];
  onClose: () => void;
}) {
  const [activatingId, setActivatingId] = useState<string | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);
  const activePluginId = active?.plugin_id;
  const orderedPlugins = [
    ...plugins.filter((plugin) => plugin.plugin_id === activePluginId),
    ...plugins.filter((plugin) => plugin.plugin_id !== activePluginId),
  ];
  const activate = async (plugin: PluginWebDescriptor) => {
    const instance = instances.find(
      (candidate) => candidate.plugin_id === plugin.plugin_id,
    );
    setActivationError(null);
    dispatchCommand({ type: "set_active_mode", mode: "play" });
    if (instance) {
      if (instance.instance_id !== active?.instance_id) {
        dispatchCommand({
          type: "select_plugin",
          instance_id: instance.instance_id,
        });
      }
      onClose();
      return;
    }
    setActivatingId(plugin.plugin_id);
    try {
      await hostJson(`/api/v1/plugins/${encodeURIComponent(plugin.plugin_id)}/activate`, {
        method: "POST",
      });
      onClose();
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not activate the plugin.",
      );
    } finally {
      setActivatingId(null);
    }
  };
  return (
    <div className="preset-modal-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section className="preset-modal plugin-picker-modal" role="dialog" aria-modal="true" aria-labelledby="plugin-picker-title">
        <header className="preset-modal-header">
          <div>
            <span className="eyebrow">PLAY · Instruments</span>
            <h2 id="plugin-picker-title">Select plugin</h2>
          </div>
          <button className="preset-modal-close" onClick={onClose} aria-label="Close plugin selector">×</button>
        </header>
        <div className="preset-modal-toolbar">
          <p>Choose the instrument you want to play. The active plugin stays first.</p>
        </div>
        {activationError && <p className="form-error">{activationError}</p>}
        <div className="play-plugin-selector modal-list" role="list" aria-label="Instrument plugins">
          {orderedPlugins.map((plugin, index) => {
            const instance = instances.find(
              (candidate) => candidate.plugin_id === plugin.plugin_id,
            );
            const selected = plugin.plugin_id === activePluginId;
            const activating = activatingId === plugin.plugin_id;
            return (
              <button
                className={`plugin-picker-card${selected ? " active" : ""}${plugin.branding ? " branded" : ""}`}
                disabled={activatingId !== null}
                key={plugin.plugin_id}
                onClick={() => void activate(plugin)}
                style={plugin.branding ? {
                  "--plugin-accent": plugin.branding.accent_color,
                  "--plugin-background": plugin.branding.background_color,
                } as CSSProperties : undefined}
              >
                {plugin.branding && (
                  <>
                    <img className="plugin-picker-banner" src={plugin.branding.banner_url} alt="" />
                    <span className="plugin-picker-shade" aria-hidden="true" />
                  </>
                )}
                <span className="play-plugin-number">{String(index + 1).padStart(2, "0")}</span>
                <PluginIcon plugin={plugin} name={plugin.plugin_name} className="plugin-picker-icon" />
                <span className="play-plugin-copy">
                  <strong>{plugin.plugin_name}{formatPluginVersion(plugin.version)}</strong>
                  <small>
                    {instance
                      ? `${instance.sounds.length} programs · Ready`
                      : "Installed · Activates on selection"}
                  </small>
                </span>
                <span className="play-plugin-status">
                  {selected ? "PLAYING" : activating ? "LOADING" : "SELECT"}
                  <i aria-hidden="true">→</i>
                </span>
              </button>
            );
          })}
          {orderedPlugins.length === 0 && (
            <PluginSurfaceState
              title="No plugins installed"
              detail="Install an .rfplugin package from the Plugins section."
            />
          )}
        </div>
      </section>
    </div>
  );
}

function formatPluginVersion(version: string | undefined) {
  if (!version) return "";
  return ` v${version.replace(/^[vV]/, "")}`;
}

function PresetModal({
  instance,
  onClose,
}: {
  instance: PluginInstance;
  onClose: () => void;
}) {
  const [presets, setPresets] = useState<HostPresetSummary[]>([]);
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameName, setRenameName] = useState("");
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const refresh = useCallback(() =>
    requestPluginPresets(instance.plugin_id)
      .then(setPresets)
      .catch((error: Error) => setMessage(error.message)), [instance.plugin_id]);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);
  const load = (preset: HostPresetSummary) => {
    setBusy(true);
    setMessage(null);
    loadPluginPreset(instance.instance_id, preset.id)
      .then(onClose)
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusy(false));
  };
  const save = (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setMessage(null);
    savePluginPreset(instance.instance_id, name.trim())
      .then((preset) => {
        setName("");
        setCreating(false);
        setMessage(`Saved ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusy(false));
  };
  const rename = (event: FormEvent, preset: HostPresetSummary) => {
    event.preventDefault();
    if (!renameName.trim()) return;
    setBusy(true);
    setMessage(null);
    renamePluginPreset(instance.plugin_id, preset.id, renameName.trim())
      .then((renamed) => {
        setRenamingId(null);
        setRenameName("");
        setMessage(`Renamed to ${renamed.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusy(false));
  };
  const remove = (preset: HostPresetSummary) => {
    setBusy(true);
    setMessage(null);
    deletePluginPreset(instance.plugin_id, preset.id)
      .then(() => {
        setDeletingId(null);
        setMessage(`Deleted ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusy(false));
  };
  return (
    <div className="preset-modal-backdrop" onMouseDown={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}>
      <section className="preset-modal" role="dialog" aria-modal="true" aria-labelledby="preset-modal-title">
        <header className="preset-modal-header">
          <div>
            <span className="eyebrow">{instance.plugin_name} · Complete states</span>
            <h2 id="preset-modal-title">Presets</h2>
          </div>
          <button className="preset-modal-close" onClick={onClose} aria-label="Close presets">×</button>
        </header>
        <div className="preset-modal-toolbar">
          <p>Load a captured state or save the instrument exactly as it sounds now.</p>
          <button className="preset-create-button" onClick={() => setCreating((value) => !value)}>
            <span aria-hidden="true">＋</span> New preset
          </button>
        </div>
        {creating && (
          <form className="preset-create-form" onSubmit={save}>
            <label>
              <span>Preset name</span>
              <input autoFocus maxLength={96} value={name} onChange={(event) => setName(event.target.value)} placeholder="Warm Strings" />
            </label>
            <button disabled={busy || !name.trim()} type="submit">{busy ? "Saving…" : "Capture state"}</button>
          </form>
        )}
        {message && <p className="preset-message">{message}</p>}
        <div className="preset-list modal-list">
          {presets.length === 0 ? (
            <div className="preset-empty"><span>00</span><strong>No presets yet</strong><small>Capture the current plugin state to create the first one.</small></div>
          ) : presets.map((preset) => (
            <article className="preset-row" key={preset.id}>
              {renamingId === preset.id ? (
                <form className="preset-rename-form" onSubmit={(event) => rename(event, preset)}>
                  <input autoFocus maxLength={96} value={renameName} onChange={(event) => setRenameName(event.target.value)} />
                  <button disabled={busy || !renameName.trim()} type="submit">Save</button>
                  <button type="button" onClick={() => setRenamingId(null)}>Cancel</button>
                </form>
              ) : (
                <>
                  <button className="preset-load-target" disabled={busy} onClick={() => load(preset)}>
                    <span><strong>{preset.name}</strong><small>State v{preset.state_version} · Plugin {preset.plugin_version}</small></span>
                    <i>LOAD →</i>
                  </button>
                  <div className="preset-row-actions">
                    <button disabled={busy} onClick={() => {
                      setDeletingId(null);
                      setRenamingId(preset.id);
                      setRenameName(preset.name);
                    }}>Rename</button>
                    <button className="danger" disabled={busy} onClick={() => setDeletingId(preset.id)}>Delete</button>
                  </div>
                </>
              )}
              {deletingId === preset.id && renamingId !== preset.id && (
                <div className="preset-delete-confirm">
                  <span>Delete “{preset.name}”?</span>
                  <button onClick={() => setDeletingId(null)}>Cancel</button>
                  <button className="danger" disabled={busy} onClick={() => remove(preset)}>{busy ? "Deleting…" : "Delete"}</button>
                </div>
              )}
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}

function PluginsPage({ snapshot }: { snapshot: SessionSnapshot | null }) {
  const [installed, setInstalled] = useState<PluginWebDescriptor[]>([]);

  useEffect(() => {
    let cancelled = false;
    hostJson<PluginWebDescriptor[]>("/api/v1/plugins")
      .then((plugins) => {
        if (!cancelled) setInstalled(plugins);
      })
      .catch(() => {
        if (!cancelled) setInstalled([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const running = snapshot?.instances ?? [];
  const runningPluginIds = new Set(running.map((instance) => instance.plugin_id));
  const available = installed.filter(
    (plugin) => !runningPluginIds.has(plugin.plugin_id),
  );

  return (
    <>
      <PageHeading
        eyebrow="Plugin configuration"
        title="Installed sound engines"
        detail="Open a running plugin to configure its libraries, resources and compatibility options. Musical controls remain in Play."
      />
      <div className="plugin-section-heading">
        <span className="card-kicker">Running now</span>
      </div>
      <PluginGrid instances={running} plugins={installed} expanded />
      {available.length > 0 && (
        <>
          <div className="plugin-section-heading">
            <span className="card-kicker">Installed</span>
            <small>Ready for a safe host activation</small>
          </div>
          <div className="plugin-grid expanded">
            {available.map((plugin, index) => (
              <article className="plugin-card installed-plugin-card" key={plugin.plugin_id}>
                <div className={`plugin-tile tile-${(index + running.length) % 4}${plugin.branding ? " branded" : ""}`}>
                  <PluginIcon plugin={plugin} name={plugin.plugin_name} />
                  {!plugin.branding && <i />}
                </div>
                <div>
                  <span className="card-kicker">
                    {plugin.active ? "Active package" : "Installed package"}
                  </span>
                  <h3>{plugin.plugin_name}</h3>
                  <p>
                    Version {plugin.version}
                    {plugin.surfaces.length === 0 ? " · No Web view" : " · Web view ready"}
                  </p>
                </div>
                <span className="plugin-installed-mark" aria-label="Installed">✓</span>
              </article>
            ))}
          </div>
        </>
      )}
    </>
  );
}

function PluginGrid({
  instances,
  plugins,
  expanded = false,
}: {
  instances: PluginInstance[];
  plugins?: PluginWebDescriptor[];
  expanded?: boolean;
}) {
  const [loadedCatalog, setLoadedCatalog] = useState<PluginWebDescriptor[]>([]);
  useEffect(() => {
    if (plugins) return;
    let cancelled = false;
    hostJson<PluginWebDescriptor[]>("/api/v1/plugins")
      .then((result) => {
        if (!cancelled) setLoadedCatalog(result);
      })
      .catch(() => {
        if (!cancelled) setLoadedCatalog([]);
      });
    return () => {
      cancelled = true;
    };
  }, [plugins]);
  const catalog = plugins ?? loadedCatalog;
  if (instances.length === 0)
    return <EmptyState title="Waiting for installed plugins" />;
  return (
    <div className={`plugin-grid${expanded ? " expanded" : ""}`}>
      {instances.map((instance, index) => {
        const plugin = catalog.find((candidate) => candidate.plugin_id === instance.plugin_id);
        const selected = instance.sounds.find(
          (sound) => sound.id === instance.selected_sound_id,
        );
        return (
          <NavLink
            className="plugin-card"
            to={`/plugins/${encodeURIComponent(instance.instance_id)}`}
            key={instance.instance_id}
          >
            <div className={`plugin-tile tile-${index % 4}${plugin?.branding ? " branded" : ""}`}>
              <PluginIcon plugin={plugin} name={instance.plugin_name} />
              {!plugin?.branding && <i />}
            </div>
            <div>
              <span className="card-kicker">Plugin</span>
              <h3>{instance.plugin_name}</h3>
              <p>{selected?.name ?? `${instance.sounds.length} programs`}</p>
            </div>
            <span className="round-arrow">→</span>
          </NavLink>
        );
      })}
    </div>
  );
}

function PluginPage({ snapshot }: { snapshot: SessionSnapshot | null }) {
  const { instanceId } = useParams();
  const instance = snapshot?.instances.find(
    (item) => item.instance_id === decodeURIComponent(instanceId ?? ""),
  );
  if (!instance)
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin not found"
          detail="This plugin instance is no longer available in the current session."
        />
      </section>
    );
  return <PluginConfigSurface instance={instance} />;
}

function PluginSurfaceState({
  title,
  detail,
}: {
  title: string;
  detail: string;
}) {
  return (
    <div className="plugin-surface-state">
      <span className="plugin-surface-state-mark">RF</span>
      <div>
        <h2>{title}</h2>
        <p>{detail}</p>
      </div>
    </div>
  );
}

function PluginConfigSurface({ instance }: { instance: PluginInstance }) {
  return (
    <section className="plugin-surface-shell">
      <div className="plugin-surface-toolbar">
        <div className="plugin-surface-identity">
          <NavLink to="/plugins" aria-label="Back to plugins">
            <span className="plugin-back-glyph" aria-hidden="true">←</span>
          </NavLink>
          <div>
            <span className="card-kicker">Plugin configuration</span>
            <strong>{instance.plugin_name}</strong>
          </div>
        </div>
      </div>
      <PluginFrame
        key={instance.instance_id}
        instance={instance}
        surface="config"
      />
    </section>
  );
}

export function PluginFrame({
  instance,
  surface,
  onSurfaceInfoChange,
  isolated = false,
  onSelectSound,
}: {
  instance: PluginInstance;
  surface: PluginWebSurfaceKind;
  onSurfaceInfoChange?: (info: { label: string; value: string } | null) => void;
  isolated?: boolean;
  onSelectSound?: (soundId: string) => Promise<unknown>;
}) {
  const [descriptor, setDescriptor] = useState<PluginWebDescriptor | null>(null);
  const [descriptorStatus, setDescriptorStatus] = useState<
    "loading" | "ready" | "unavailable" | "error"
  >("loading");
  const [frameLoaded, setFrameLoaded] = useState(false);
  const snapshot = useSelector((state: RootState) => state.rackforge.snapshot);
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const pendingResourceRequestRef = useRef<string | null>(null);
  const [resourceRequest, setResourceRequest] = useState<{
    requestId: string;
    resource: PluginResourceRequirement;
  } | null>(null);

  useEffect(
    () => () => {
      onSurfaceInfoChange?.(null);
    },
    [onSurfaceInfoChange],
  );

  const sendPluginResponse = useCallback(
    (requestId: string, ok: boolean, error?: string, result?: unknown) => {
      frameRef.current?.contentWindow?.postMessage(
        {
          protocol: "rackforge.plugin.web@1",
          kind: "response",
          request_id: requestId,
          ok,
          ...(error ? { error } : {}),
          ...(result !== undefined ? { result } : {}),
        },
        window.location.origin,
      );
    },
    [],
  );

  useEffect(() => {
    let cancelled = false;
    hostJson<PluginWebDescriptor>(
      `/api/v1/plugins/${encodeURIComponent(instance.plugin_id)}`,
    )
      .then((value) => {
        if (!cancelled) {
          setDescriptor(value);
          setDescriptorStatus(value ? "ready" : "unavailable");
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setDescriptor(null);
          setDescriptorStatus(
            error instanceof HostRequestError && error.status === 404
              ? "unavailable"
              : "error",
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, [instance.plugin_id, instance.plugin_name]);

  const selectedSurface = descriptor?.surfaces.find(
    (candidate) => candidate.kind === surface,
  );

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame || !selectedSurface) return;

    const send = (message: unknown) =>
      frame.contentWindow?.postMessage(message, window.location.origin);
    const context = {
      protocol: "rackforge.plugin.web@1",
      kind: "context",
      surface,
      instance,
      program_draft:
        snapshot?.program_draft?.instance_id === instance.instance_id
          ? snapshot.program_draft
          : null,
      audition:
        snapshot?.audition?.instance_id === instance.instance_id
          ? snapshot.audition
          : null,
      host: {
        active_mode: snapshot?.active_mode ?? "play",
        master_level: snapshot?.master_level ?? 0,
        master_pan: snapshot?.master_pan ?? 0,
      },
    };
    const onMessage = (event: MessageEvent) => {
      if (
        event.source !== frame.contentWindow ||
        event.origin !== window.location.origin ||
        !event.data ||
        event.data.protocol !== "rackforge.plugin.web@1"
      ) {
        return;
      }
      if (event.data.kind === "ready") {
        send(context);
        return;
      }
      if (
        event.data.kind !== "request" ||
        typeof event.data.request_id !== "string"
      ) {
        return;
      }
      const respond = (ok: boolean, error?: string, result?: unknown) =>
        send({
          protocol: "rackforge.plugin.web@1",
          kind: "response",
          request_id: event.data.request_id,
          ok,
          ...(error ? { error } : {}),
          ...(result !== undefined ? { result } : {}),
        });
      const params =
        event.data.params && typeof event.data.params === "object"
          ? event.data.params
          : {};
      const draft =
        snapshot?.program_draft?.instance_id === instance.instance_id
          ? snapshot.program_draft
          : undefined;
      if (
        event.data.method === "plugin.parameters" &&
        (surface === "play" || surface === "config")
      ) {
        if (isolated) {
          respond(false, "Rack Slot parameter editing needs an isolated audio edit session.");
        } else requestPluginParameters(instance.instance_id)
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not read plugin parameters.",
            ),
          );
      } else if (
        event.data.method === "plugin.set_parameter" &&
        (surface === "play" || surface === "config") &&
        Number.isInteger(params.parameter_index) &&
        typeof params.value === "number" &&
        Number.isFinite(params.value)
      ) {
        if (isolated) {
          respond(false, "Rack Slot parameter editing needs an isolated audio edit session.");
        } else setPluginParameter(
          instance.instance_id,
          Number(params.parameter_index),
          params.value,
        )
          .then((value) => respond(true, undefined, { value }))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not set plugin parameter.",
            ),
          );
      } else if (
        event.data.method === "plugin.select_sound" &&
        (surface === "play" || surface === "config") &&
        typeof params.sound_id === "string" &&
        instance.sounds.some(
          (sound) => sound.id === params.sound_id,
        )
      ) {
        if (onSelectSound) {
          onSelectSound(params.sound_id)
            .then((result) => respond(true, undefined, result))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error ? error.message : "Could not select this sound.",
              ),
            );
        } else if (isNativeHost()) {
          selectNativePluginSound({
            instance_id: instance.instance_id,
            sound_id: params.sound_id,
          })
            .then((result) => respond(true, undefined, result))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not select this program.",
              ),
            );
        } else {
          dispatchCommand({
            type: "select_sound",
            instance_id: instance.instance_id,
            sound_id: params.sound_id,
          });
          respond(true);
        }
      } else if (
        event.data.method === "plugin.select_resource" &&
        surface === "config" &&
        typeof params.resource_id === "string"
      ) {
        const resource = descriptor?.resources.find(
          (candidate) => candidate.id === params.resource_id,
        );
        if (!resource) {
          respond(false, "Resource is not declared by this plugin.");
        } else if (pendingResourceRequestRef.current) {
          respond(false, "Another resource selection is already open.");
        } else if (isNativeHost()) {
          pendingResourceRequestRef.current = event.data.request_id;
          bindNativePluginResource({
            plugin_id: instance.plugin_id,
            resource_id: resource.id,
          })
            .then((grant) => respond(true, undefined, grant))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not select this resource.",
              ),
            )
            .finally(() => {
              pendingResourceRequestRef.current = null;
            });
        } else {
          pendingResourceRequestRef.current = event.data.request_id;
          setResourceRequest({
            requestId: event.data.request_id,
            resource,
          });
        }
      } else if (
        event.data.method === "plugin.resource_bindings" &&
        surface === "config"
      ) {
        postResourceApi("/api/v1/resources/grants", {
          plugin_id: instance.plugin_id,
        })
          .then((grants) => respond(true, undefined, grants))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not read resource bindings.",
            ),
          );
      } else if (
        event.data.method === "plugin.resource_status" &&
        surface === "config"
      ) {
        postResourceApi("/api/v1/resources/status", {
          plugin_id: instance.plugin_id,
        })
          .then((status) => respond(true, undefined, status))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not read installed resources.",
            ),
          );
      } else if (
        event.data.method === "plugin.resource_entries" &&
        surface === "config" &&
        typeof params.grant_id === "string" &&
        (params.parent_id === null || params.parent_id === undefined ||
          typeof params.parent_id === "string")
      ) {
        postResourceApi("/api/v1/resources/browse", {
          plugin_id: instance.plugin_id,
          grant_id: params.grant_id,
          parent_id: typeof params.parent_id === "string" ? params.parent_id : null,
        })
          .then((entries) => respond(true, undefined, entries))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not browse this resource.",
            ),
          );
      } else if (
        (event.data.method === "plugin.load_resource" ||
          event.data.method === "plugin.install_resource") &&
        surface === "config" &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        ) &&
        typeof params.grant_id === "string" &&
        (params.entry_id === null || params.entry_id === undefined ||
          typeof params.entry_id === "string")
      ) {
        postResourceApi("/api/v1/resources/load", {
          plugin_id: instance.plugin_id,
          instance_id: instance.instance_id,
          target_resource_id: params.target_resource_id,
          grant_id: params.grant_id,
          entry_id: typeof params.entry_id === "string" ? params.entry_id : null,
          persist: event.data.method === "plugin.install_resource",
        })
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not load this resource.",
            ),
          );
      } else if (
        event.data.method === "plugin.set_surface_info" &&
        surface === "play" &&
        (params.label === undefined ||
          (typeof params.label === "string" && params.label.length <= 24)) &&
        (params.value === null ||
          params.value === undefined ||
          (typeof params.value === "string" && params.value.length <= 96))
      ) {
        const label = typeof params.label === "string" ? params.label.trim() : "";
        const value = typeof params.value === "string" ? params.value.trim() : "";
        onSurfaceInfoChange?.(value ? { label, value } : null);
        respond(true, undefined, { published: value.length > 0 });
      } else if (
        event.data.method === "plugin.begin_program_edit" &&
        (surface === "play" || surface === "config") &&
        (params.program_id === null ||
          (typeof params.program_id === "string" &&
            instance.sounds.some(
              (sound) => sound.id === params.program_id && sound.editable,
            )))
      ) {
        if (isolated) {
          respond(false, "Program document editing is unavailable in a Rack Slot session.");
        } else dispatchCommand({
          type: "begin_program_edit",
          instance_id: instance.instance_id,
          ...(typeof params.program_id === "string"
            ? { program_id: params.program_id }
            : {}),
        });
        if (!isolated) respond(true);
      } else if (
        event.data.method === "plugin.edit_program_field" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id &&
        typeof params.field_id === "string" &&
        findEditorField(draft.editor.pages, params.field_id) &&
        isProgramEditorValue(params.value)
      ) {
        dispatchCommand({
          type: "edit_program_draft_field",
          draft_id: draft.draft_id,
          field_id: params.field_id,
          value: params.value,
          preview: params.preview === true,
        });
        respond(true);
      } else if (
        event.data.method === "plugin.set_program_name" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id &&
        typeof params.name === "string" &&
        params.name.trim().length > 0 &&
        params.name.trim().length <= 64 &&
        /^[\x20-\x7e]+$/.test(params.name.trim())
      ) {
        try {
          const document = JSON.parse(draft.document_json) as Record<
            string,
            unknown
          >;
          document.name = params.name.trim();
          dispatchCommand({
            type: "replace_program_draft",
            draft_id: draft.draft_id,
            document_json: JSON.stringify(document),
          });
          respond(true);
        } catch {
          respond(false, "The active program document is invalid.");
        }
      } else if (
        event.data.method === "plugin.save_program" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id
      ) {
        dispatchCommand({
          type: "save_program_draft",
          draft_id: draft.draft_id,
        });
        respond(true);
      } else if (
        event.data.method === "plugin.cancel_program" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id
      ) {
        dispatchCommand({
          type: "cancel_program_edit",
          draft_id: draft.draft_id,
        });
        respond(true);
      } else if (
        event.data.method === "plugin.restore_program_preview" &&
        !isolated &&
        (surface === "play" || surface === "config") &&
        draft &&
        params.draft_id === draft.draft_id
      ) {
        dispatchCommand({
          type: "restore_program_draft_preview",
          draft_id: draft.draft_id,
        });
        respond(true);
      } else {
        respond(false, "Method is not available for this plugin surface.");
      }
    };
    const onLoad = () => send(context);
    window.addEventListener("message", onMessage);
    frame.addEventListener("load", onLoad);
    send(context);
    return () => {
      window.removeEventListener("message", onMessage);
      frame.removeEventListener("load", onLoad);
    };
  }, [descriptor, instance, isolated, onSelectSound, onSurfaceInfoChange, selectedSurface, snapshot, surface]);

  const editLease =
    (surface === "play" || surface === "config") &&
    snapshot?.program_draft?.instance_id === instance.instance_id &&
    snapshot.audition?.instance_id === instance.instance_id
      ? snapshot.audition.lease_id
      : null;

  useEffect(() => {
    if (editLease === null) return;
    const keepAlive = () =>
      dispatchCommand({ type: "keep_audition_alive", lease_id: editLease });
    keepAlive();
    const timer = window.setInterval(keepAlive, 5000);
    return () => window.clearInterval(timer);
  }, [editLease]);

  if (surface === "config" && !instance.config_available) {
    return (
      <PluginSurfaceState
        title="CONFIG mode unavailable"
        detail={`${instance.plugin_name} uses PLAY as its complete editor. Save and restore its state with RackForge presets.`}
      />
    );
  }

  if (descriptorStatus === "loading") {
    return (
      <RfLoader
        className="plugin-surface-loader"
        label={instance.plugin_name}
        detail="Loading plugin interface…"
        size="medium"
      />
    );
  }
  if (descriptorStatus === "error") {
    return (
      <PluginSurfaceState
        title="Plugin web view could not be loaded"
        detail="RackForge could not read this plugin's web manifest."
      />
    );
  }
  if (descriptorStatus === "unavailable" || !selectedSurface) {
    return (
      <PluginSurfaceState
        title="Web view unavailable"
        detail={
          descriptor
            ? `${instance.plugin_name} does not provide a ${surface.toUpperCase()} web view.`
            : `${instance.plugin_name} does not include a RackForge Web interface.`
        }
      />
    );
  }
  const finishResourceSelection = (
    ok: boolean,
    error?: string,
    grant?: ResourceGrant,
  ) => {
    const requestId = resourceRequest?.requestId;
    if (!requestId) return;
    sendPluginResponse(requestId, ok, error, grant);
    pendingResourceRequestRef.current = null;
    setResourceRequest(null);
  };

  return (
    <>
      <div
        className="plugin-frame-stage"
        style={descriptor?.branding?.background_color ? {
          backgroundColor: descriptor.branding.background_color,
        } : undefined}
      >
        <iframe
          key={selectedSurface.entry_url}
          ref={frameRef}
          className={`plugin-frame${frameLoaded ? " loaded" : ""}`}
          title={`${instance.plugin_name} ${surface}`}
          src={selectedSurface.entry_url}
          sandbox="allow-scripts allow-same-origin"
          referrerPolicy="same-origin"
          onLoad={() => setFrameLoaded(true)}
        />
        {!frameLoaded && (
          <div className="plugin-brand-splash" aria-label={`Loading ${instance.plugin_name}`}>
            {descriptor?.branding ? (
              <img src={descriptor.branding.splash_url} alt="" />
            ) : (
              <RfLoader label={instance.plugin_name} detail="Loading plugin interface…" size="medium" />
            )}
          </div>
        )}
      </div>
      {resourceRequest ? (
        <Suspense
          fallback={
            <div className="resource-explorer-backdrop">
              <RfLoader label="RackForge storage" detail="Opening explorer…" />
            </div>
          }
        >
          <ResourceExplorerDialog
            pluginId={instance.plugin_id}
            resource={resourceRequest.resource}
            onCancel={() =>
              finishResourceSelection(
                false,
                "Resource selection was cancelled by the user.",
              )
            }
            onBound={(grant) => finishResourceSelection(true, undefined, grant)}
          />
        </Suspense>
      ) : null}
    </>
  );
}

/// Changing the access PIN, which needs the current one.
///
/// Asking for the PIN somebody already has may look redundant inside a session
/// that is already authorised. It is not: a browser left open on a borrowed
/// laptop should not be enough to take the device over, and every other
/// session is dropped when the PIN changes, so this is also how somebody who
/// should not be here is put out.
function ChangePinCard() {
  // Asked rather than assumed. The same interface is served by a network host
  // that decides access by PIN and by a desktop window that has no such
  // notion, and offering to change a PIN that does nothing is worse than
  // offering nothing at all.
  const [managed, setManaged] = useState<boolean | null>(null);
  useEffect(() => {
    let cancelled = false;
    hostJson<WebAuthStatus>("/api/v1/auth/status")
      .then((status: WebAuthStatus) => {
        if (!cancelled) setManaged(status.pin_managed === true);
      })
      .catch(() => {
        if (!cancelled) setManaged(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [repeat, setRepeat] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);

  const digits = 4;
  const clean = (value: string) => value.replace(/\D/g, "").slice(0, digits);
  const ready =
    current.length === digits && next.length === digits && next === repeat;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setNote(null);
    hostJson<unknown>("/api/v1/auth/pin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ pin: next, current_pin: current }),
    })
      .then(() => {
        setNote({
          ok: true,
          text: "PIN changed. Every other browser has been signed out.",
        });
        setCurrent("");
        setNext("");
        setRepeat("");
      })
      .catch((reason: Error) => setNote({ ok: false, text: reason.message }))
      .finally(() => setBusy(false));
  };

  if (managed !== true) {
    return null;
  }

  return (
    <article className="settings-card pairing-card">
      <div className="settings-icon">••</div>
      <div className="settings-copy">
        <span className="card-kicker">Security</span>
        <h2>Access PIN</h2>
        <p>
          Asked for whenever this interface is opened from another machine.
          Changing it signs out every other browser.
        </p>
      </div>
      <form className="pin-form" onSubmit={submit}>
        <input
          value={current}
          onChange={(event) => setCurrent(clean(event.target.value))}
          inputMode="numeric"
          autoComplete="current-password"
          placeholder="Current"
          aria-label="Current PIN"
          disabled={busy}
        />
        <input
          value={next}
          onChange={(event) => setNext(clean(event.target.value))}
          inputMode="numeric"
          autoComplete="new-password"
          placeholder="New"
          aria-label="New PIN"
          disabled={busy}
        />
        <input
          value={repeat}
          onChange={(event) => setRepeat(clean(event.target.value))}
          inputMode="numeric"
          autoComplete="new-password"
          placeholder="Repeat"
          aria-label="Repeat the new PIN"
          disabled={busy}
        />
        <button className="secondary-button" disabled={!ready || busy}>
          {busy ? "Changing…" : "Change PIN"}
        </button>
      </form>
      {note && (
        <div className={note.ok ? "pin-note" : "pairing-error"}>{note.text}</div>
      )}
    </article>
  );
}

interface HostSettingsBootstrap {
  config: WebPublicConfig | null;
  repositoryFile: PluginRepositoryFile | null;
  audioSettings: HostAudioSettings | null;
}

let hostSettingsBootstrapPromise: Promise<HostSettingsBootstrap> | null = null;

function requestHostSettingsBootstrap() {
  hostSettingsBootstrapPromise ??= Promise.all([
    hostJson<WebPublicConfig>("/api/v1/config").catch(() => null),
    hostJson<PluginRepositoryFile>("/api/v1/repositories").catch(() => null),
    hostJson<HostAudioSettings>("/api/v1/host/audio").catch(() => null),
  ]).then(([config, repositoryFile, audioSettings]) => ({
    config,
    repositoryFile,
    audioSettings,
  }));
  return hostSettingsBootstrapPromise;
}

function SettingsPage({
  initial,
  onConfigChange,
  onRepositoriesChange,
  onAudioChange,
}: {
  initial: HostSettingsBootstrap;
  onConfigChange: (config: WebPublicConfig) => void;
  onRepositoriesChange: (repositoryFile: PluginRepositoryFile) => void;
  onAudioChange: (audioSettings: HostAudioSettings) => void;
}) {
  const [config, setConfig] = useState<WebPublicConfig | null>(initial.config);
  const [webDraft, setWebDraft] = useState<{ enabled: boolean; port: number } | null>(
    initial.config ? { enabled: initial.config.enabled, port: initial.config.port } : null,
  );
  const [webBusy, setWebBusy] = useState(false);
  const [webMessage, setWebMessage] = useState<string | null>(null);
  const [repositoryFile, setRepositoryFile] =
    useState<PluginRepositoryFile | null>(initial.repositoryFile);
  const [catalog, setCatalog] = useState<StoreCatalogResponse | null>(null);
  const [storeBusy, setStoreBusy] = useState(false);
  const [storeMessage, setStoreMessage] = useState<string | null>(null);
  const [audioSettings, setAudioSettings] = useState<HostAudioSettings | null>(initial.audioSettings);
  const [audioDraft, setAudioDraft] = useState<HostAudioPreferences | null>(initial.audioSettings?.preferences ?? null);
  const [audioBusy, setAudioBusy] = useState(false);
  const [audioMessage, setAudioMessage] = useState<string | null>(null);
  const loadAudioSettings = useCallback(async () => {
    setAudioBusy(true);
    setAudioMessage(null);
    try {
      const settings = await hostJson<HostAudioSettings>("/api/v1/host/audio");
      setAudioSettings(settings);
      setAudioDraft(settings.preferences);
      onAudioChange(settings);
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "Device refresh failed.");
    } finally {
      setAudioBusy(false);
    }
  }, [onAudioChange]);

  const selectAudioDriver = (driver: string) => {
    if (!audioSettings || !audioDraft) return;
    const output = audioSettings.inventory.outputs.find(
      (candidate) => candidate.driver === driver && candidate.is_default,
    ) ?? audioSettings.inventory.outputs.find((candidate) => candidate.driver === driver);
    if (!output) return;
    setAudioDraft({
      ...audioDraft,
      driver,
      output_device: output.name,
      sample_rate_hz: output.default_sample_rate,
      buffer_frames: undefined,
    });
  };
  const selectAudioOutput = (name: string) => {
    if (!audioSettings || !audioDraft) return;
    const output = audioSettings.inventory.outputs.find(
      (candidate) => candidate.driver === audioDraft.driver && candidate.name === name,
    );
    if (!output) return;
    setAudioDraft({
      ...audioDraft,
      output_device: name,
      sample_rate_hz: output.sample_rates.includes(audioDraft.sample_rate_hz)
        ? audioDraft.sample_rate_hz
        : output.default_sample_rate,
      buffer_frames: output.buffer_frames.includes(audioDraft.buffer_frames ?? -1)
        ? audioDraft.buffer_frames
        : undefined,
    });
  };
  const saveAudioSettings = async () => {
    if (!audioDraft) return;
    setAudioBusy(true);
    setAudioMessage(null);
    try {
      const settings = await hostJson<HostAudioSettings>("/api/v1/host/audio", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(audioDraft),
      });
      setAudioSettings(settings);
      setAudioDraft(settings.preferences);
      onAudioChange(settings);
      setAudioMessage("Audio and MIDI settings applied.");
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "Audio settings failed.");
    } finally {
      setAudioBusy(false);
    }
  };
  const testAudio = async () => {
    setAudioBusy(true);
    setAudioMessage(null);
    try {
      await hostJson("/api/v1/host/audio/test", { method: "POST" });
      setAudioMessage("Playing test note.");
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "Audio test failed.");
    } finally {
      setAudioBusy(false);
    }
  };

  const updateRepository = (
    index: number,
    patch: Partial<PluginRepositoryConfig>,
  ) => {
    setRepositoryFile((current) => {
      if (!current) return current;
      const repositories = [...current.repositories];
      repositories[index] = { ...repositories[index], ...patch };
      return { ...current, repositories };
    });
  };

  const saveWebSettings = async () => {
    if (!webDraft) return;
    setWebBusy(true);
    setWebMessage(null);
    try {
      const next = await hostJson<WebPublicConfig & { message?: string }>("/api/v1/config", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ schema_version: 1, ...webDraft }),
      });
      setConfig(next);
      setWebDraft({ enabled: next.enabled, port: next.port });
      onConfigChange(next);
      setWebMessage(next.message ?? "HTTP server settings applied.");
    } catch (error) {
      setWebMessage(error instanceof Error ? error.message : "HTTP settings failed.");
    } finally {
      setWebBusy(false);
    }
  };

  const saveRepositories = async () => {
    if (!repositoryFile) return;
    setStoreBusy(true);
    setStoreMessage(null);
    try {
      const body = await hostJson<{
        status: "ok";
        config: PluginRepositoryFile;
      }>("/api/v1/repositories", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(repositoryFile),
      });
      setRepositoryFile(body.config);
      onRepositoriesChange(body.config);
      setStoreMessage("Repository configuration saved.");
    } catch (error) {
      setStoreMessage(error instanceof Error ? error.message : "Save failed.");
    } finally {
      setStoreBusy(false);
    }
  };

  const refreshCatalog = async () => {
    setStoreBusy(true);
    setStoreMessage(null);
    try {
      const body = await hostJson<StoreCatalogResponse>("/api/v1/store/catalog");
      setCatalog(body);
      setStoreMessage("Signed catalogs refreshed.");
    } catch (error) {
      setStoreMessage(error instanceof Error ? error.message : "Refresh failed.");
    } finally {
      setStoreBusy(false);
    }
  };

  const installPlugin = async (repositoryId: string, pluginId: string) => {
    setStoreBusy(true);
    setStoreMessage(`Installing ${pluginId}…`);
    try {
      const body = await hostJson<{
        plugin_id: string;
        version: string;
        already_installed?: boolean;
        activation_required?: boolean;
      }>("/api/v1/store/install", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ repository_id: repositoryId, plugin_id: pluginId }),
      });
      setStoreMessage(`Activating ${body.plugin_id}…`);
      await activateInstalledPlugin({
        plugin_id: body.plugin_id,
        version: body.version,
        already_installed: body.already_installed ?? false,
        activation_required: body.activation_required ?? true,
      });
      setStoreMessage(
        `${body.plugin_id} ${body.version} installed and active.`,
      );
    } catch (error) {
      setStoreMessage(error instanceof Error ? error.message : "Installation failed.");
    } finally {
      setStoreBusy(false);
    }
  };

  return (
    <>
      <PageHeading
        eyebrow="RackForge"
        title="Settings"
        detail="Host-wide configuration. Plugin-specific controls live inside each plugin."
      />
      <section className="settings-grid">
        {audioSettings && audioDraft ? (
          <article className="settings-card host-audio-settings-card">
            <div className="settings-icon">♫</div>
            <div className="settings-copy">
              <span className="card-kicker">{audioSettings.host} host</span>
              <h2>Audio & MIDI</h2>
              <p>Available controls are provided by this device and applied by its native audio runtime.</p>
            </div>
            <div className="host-audio-form">
              <label>
                <span>Driver</span>
                <select value={audioDraft.driver} onChange={(event) => selectAudioDriver(event.target.value)}>
                  {audioSettings.inventory.drivers.map((driver) => (
                    <option key={driver.name} value={driver.name} disabled={!driver.available}>
                      {driver.name}{driver.available ? "" : " (unavailable)"}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>Output device</span>
                <select value={audioDraft.output_device} onChange={(event) => selectAudioOutput(event.target.value)}>
                  {audioSettings.inventory.outputs
                    .filter((output) => output.driver === audioDraft.driver)
                    .map((output) => (
                      <option key={output.name} value={output.name}>
                        {output.name}{output.is_default ? " (default)" : ""}
                      </option>
                    ))}
                </select>
              </label>
              {(() => {
                const output = audioSettings.inventory.outputs.find(
                  (candidate) => candidate.driver === audioDraft.driver && candidate.name === audioDraft.output_device,
                );
                return output ? (
                  <>
                    <label>
                      <span>Sample rate</span>
                      <select value={audioDraft.sample_rate_hz} onChange={(event) => setAudioDraft({ ...audioDraft, sample_rate_hz: Number(event.target.value) })}>
                        {output.sample_rates.map((rate) => <option key={rate} value={rate}>{rate} Hz</option>)}
                      </select>
                    </label>
                    <label>
                      <span>Buffer</span>
                      <select value={audioDraft.buffer_frames ?? ""} onChange={(event) => setAudioDraft({ ...audioDraft, buffer_frames: event.target.value ? Number(event.target.value) : undefined })}>
                        <option value="">System default</option>
                        {output.buffer_frames.map((frames) => <option key={frames} value={frames}>{frames} frames · {(frames * 1000 / audioDraft.sample_rate_hz).toFixed(1)} ms</option>)}
                      </select>
                    </label>
                  </>
                ) : null;
              })()}
              <label>
                <span>Output gain</span>
                <select value={audioDraft.output_gain_db} onChange={(event) => setAudioDraft({ ...audioDraft, output_gain_db: Number(event.target.value) })}>
                  {[0, 3, 6, 9, 12].map((gain) => <option key={gain} value={gain}>+{gain} dB</option>)}
                </select>
              </label>
              <fieldset>
                <legend>MIDI inputs</legend>
                {audioSettings.inventory.midi_inputs.length ? audioSettings.inventory.midi_inputs.map((input) => (
                  <label className="host-audio-check" key={input}>
                    <input
                      type="checkbox"
                      checked={audioDraft.midi_inputs.includes(input)}
                      onChange={(event) => setAudioDraft({
                        ...audioDraft,
                        midi_inputs: event.target.checked
                          ? [...audioDraft.midi_inputs, input].sort()
                          : audioDraft.midi_inputs.filter((candidate) => candidate !== input),
                      })}
                    />
                    <span>{input}</span>
                  </label>
                )) : <p>No MIDI inputs detected.</p>}
              </fieldset>
              <pre>{audioSettings.runtime_status}</pre>
              <div className="host-audio-actions">
                <button className="secondary-button" disabled={audioBusy} onClick={() => void loadAudioSettings()}>Refresh devices</button>
                <button className="secondary-button" disabled={audioBusy} onClick={() => void testAudio()}>Test note</button>
                <button className="primary-button" disabled={audioBusy} onClick={() => void saveAudioSettings()}>{audioBusy ? "Working…" : "Apply"}</button>
              </div>
              {audioMessage ? <p className="repository-message">{audioMessage}</p> : null}
            </div>
          </article>
        ) : null}
        <article className="settings-card">
          <div className="settings-icon">⌁</div>
          <div className="settings-copy">
            <span className="card-kicker">Web interface</span>
            <h2>Local access</h2>
            <p>
              Where the interface can be reached from. Only publish RackForge
              on a network you trust.
            </p>
          </div>
          <dl className="settings-values">
            <div>
              <dt>Status</dt>
              <dd className="status-value">
                <span />
                {config?.enabled === false ? "Disabled" : "Enabled"}
              </dd>
            </div>
            <div>
              <dt>Access</dt>
              <dd>{config?.access ?? "local"}</dd>
            </div>
            <div>
              <dt>Port</dt>
              <dd>{config?.port ?? "8787"}</dd>
            </div>
          </dl>
          {config?.configurable && webDraft ? (
            <div className="web-server-settings-form">
              <label className="repository-check">
                <input type="checkbox" checked={webDraft.enabled} onChange={(event) => setWebDraft({ ...webDraft, enabled: event.target.checked })} />
                <span>Enable HTTP server</span>
              </label>
              <label>
                <span>Port</span>
                <input type="number" min="1024" max="65535" disabled={!webDraft.enabled} value={webDraft.port} onChange={(event) => setWebDraft({ ...webDraft, port: Number(event.target.value) })} />
              </label>
              <button className="secondary-button" disabled={webBusy || webDraft.port < 1024 || webDraft.port > 65535} onClick={() => void saveWebSettings()}>{webBusy ? "Applying…" : "Apply server settings"}</button>
              {webMessage ? <p>{webMessage}</p> : null}
            </div>
          ) : null}
        </article>
        <ChangePinCard />
        <article className="settings-card repository-settings-card">
          <div className="settings-icon">⬡</div>
          <div className="settings-copy">
            <span className="card-kicker">Plugin stores</span>
            <h2>Signed repositories</h2>
            <p>
              Catalog signatures and package hashes are checked before any
              native plugin is installed.
            </p>
          </div>
          <div className="repository-editor">
            {repositoryFile?.repositories.map((repository, index) => (
              <div className="repository-form" key={`${repository.id}-${index}`}>
                <label>
                  <span>Name</span>
                  <input
                    value={repository.name}
                    onChange={(event) =>
                      updateRepository(index, { name: event.target.value })
                    }
                  />
                </label>
                <label>
                  <span>Repository ID</span>
                  <input
                    value={repository.id}
                    onChange={(event) =>
                      updateRepository(index, { id: event.target.value })
                    }
                  />
                </label>
                <label className="repository-wide-field">
                  <span>Base URL</span>
                  <input
                    value={repository.base_url}
                    onChange={(event) =>
                      updateRepository(index, { base_url: event.target.value })
                    }
                  />
                </label>
                <label className="repository-wide-field">
                  <span>Ed25519 public key</span>
                  <input
                    value={repository.public_key}
                    onChange={(event) =>
                      updateRepository(index, { public_key: event.target.value })
                    }
                    spellCheck={false}
                  />
                </label>
                <label className="repository-check">
                  <input
                    type="checkbox"
                    checked={repository.enabled}
                    onChange={(event) =>
                      updateRepository(index, { enabled: event.target.checked })
                    }
                  />
                  <span>Enabled</span>
                </label>
                <label className="repository-check">
                  <input
                    type="checkbox"
                    checked={repository.allow_insecure_http}
                    onChange={(event) =>
                      updateRepository(index, {
                        allow_insecure_http: event.target.checked,
                      })
                    }
                  />
                  <span>Allow HTTP for this LAN</span>
                </label>
                <button
                  className="text-button danger-text-button"
                  disabled={storeBusy}
                  onClick={() =>
                    setRepositoryFile((current) =>
                      current
                        ? {
                            ...current,
                            repositories: current.repositories.filter(
                              (_, candidate) => candidate !== index,
                            ),
                          }
                        : current,
                    )
                  }
                >
                  Remove
                </button>
              </div>
            ))}
            {repositoryFile?.repositories.length === 0 && (
              <p className="repository-empty">No plugin repository configured.</p>
            )}
            <div className="repository-actions">
              <button
                className="secondary-button"
                disabled={!repositoryFile || storeBusy}
                onClick={() =>
                  setRepositoryFile((current) =>
                    current
                      ? {
                          ...current,
                          repositories: [
                            ...current.repositories,
                            {
                              id: "org.example.community",
                              name: "Community Store",
                              base_url: "https://",
                              public_key: "",
                              enabled: true,
                              allow_insecure_http: false,
                            },
                          ],
                        }
                      : current,
                  )
                }
              >
                Add repository
              </button>
              <button
                className="secondary-button"
                disabled={!repositoryFile || storeBusy}
                onClick={saveRepositories}
              >
                Save
              </button>
              <button
                className="primary-button"
                disabled={storeBusy}
                onClick={refreshCatalog}
              >
                {storeBusy ? "Working…" : "Refresh stores"}
              </button>
            </div>
            {storeMessage && <p className="repository-message">{storeMessage}</p>}
          </div>
        </article>
        {catalog?.repositories.map((repository) => (
          <article className="settings-card store-catalog-card" key={repository.repository_id}>
            <div className="settings-copy">
              <span className="card-kicker">{repository.status}</span>
              <h2>{repository.name}</h2>
              {repository.error && <p>{repository.error}</p>}
            </div>
            <div className="store-plugin-list">
              {repository.catalog?.plugins.map((plugin) => (
                <div className="store-plugin" key={plugin.id}>
                  <div>
                    <strong>{plugin.name}</strong>
                    <span>{plugin.latest_version ?? "No release"}</span>
                    <p>{plugin.summary}</p>
                    <small>
                      {plugin.license}
                      {plugin.active_version
                        ? ` · Active ${plugin.active_version}`
                        : plugin.installed
                          ? ` · Installed ${plugin.installed_versions.join(", ")}`
                          : ""}
                    </small>
                  </div>
                  <button
                    className="secondary-button"
                    disabled={
                      storeBusy || (plugin.installed && !plugin.update_available)
                    }
                    onClick={() => installPlugin(repository.repository_id, plugin.id)}
                  >
                    {plugin.update_available
                      ? "Update"
                      : plugin.installed
                        ? "Installed"
                        : "Install"}
                  </button>
                </div>
              ))}
            </div>
          </article>
        ))}
      </section>
    </>
  );
}

interface HostDiagnostics {
  platform: string;
  version: string;
  audio_running: boolean;
  selected_audio_output: string;
  audio_status: Record<string, number>;
  audio_outputs: Array<{ id: number; name: string; detail: string }>;
  midi_devices: Array<{ name: string; detail: string }>;
  usb_devices: Array<{ name: string; detail: string }>;
}

function DiagnosticsPage() {
  const [diagnostics, setDiagnostics] = useState<HostDiagnostics | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      setDiagnostics(await hostJson<HostDiagnostics>("/api/v1/diagnostics"));
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "Diagnostics are unavailable.");
    } finally {
      setRefreshing(false);
    }
  }, []);
  useEffect(() => {
    let cancelled = false;
    hostJson<HostDiagnostics>("/api/v1/diagnostics")
      .then((result) => {
        if (!cancelled) {
          setDiagnostics(result);
          setError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(reason instanceof Error ? reason.message : "Diagnostics are unavailable.");
        }
      })
      .finally(() => {
        if (!cancelled) setRefreshing(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  const status = diagnostics?.audio_status ?? {};
  return (
    <>
      <PageHeading
        eyebrow="Runtime status"
        title="Diagnostics"
        detail="Live information from the native audio, MIDI and USB host."
      />
      <div className="diagnostics-actions">
        <button className="primary-button" disabled={refreshing} onClick={() => void refresh()}>
          {refreshing ? "Refreshing…" : "Refresh devices"}
        </button>
      </div>
      {error ? <p className="form-error">{error}</p> : null}
      {diagnostics ? (
        <section className="settings-grid diagnostics-grid">
          <article className="settings-card">
            <div className="settings-icon">◉</div>
            <div className="settings-copy">
              <span className="card-kicker">Native runtime</span>
              <h2>{diagnostics.platform}</h2>
              <p>RackForge {diagnostics.version} · {diagnostics.audio_running ? "Audio running" : "Audio stopped"}</p>
            </div>
            <dl className="settings-values">
              <div><dt>Output</dt><dd>{diagnostics.selected_audio_output}</dd></div>
              <div><dt>Sample rate</dt><dd>{status.sample_rate ?? 0} Hz</dd></div>
              <div><dt>Buffer</dt><dd>{status.buffer_size_frames ?? 0} frames</dd></div>
              <div><dt>Xruns</dt><dd>{status.xruns ?? 0}</dd></div>
            </dl>
          </article>
          <DiagnosticDeviceCard title="Audio outputs" items={diagnostics.audio_outputs} />
          <DiagnosticDeviceCard title="MIDI devices" items={diagnostics.midi_devices} />
          <DiagnosticDeviceCard title="USB devices" items={diagnostics.usb_devices} />
        </section>
      ) : null}
    </>
  );
}

function DiagnosticDeviceCard({
  title,
  items,
}: {
  title: string;
  items: Array<{ name: string; detail: string }>;
}) {
  return (
    <article className="settings-card diagnostic-device-card">
      <div className="settings-copy">
        <span className="card-kicker">Connected · {items.length}</span>
        <h2>{title}</h2>
      </div>
      <div className="diagnostic-device-list">
        {items.map((item, index) => (
          <div key={`${item.name}-${index}`}>
            <strong>{item.name}</strong>
            <small>{item.detail}</small>
          </div>
        ))}
        {items.length === 0 ? <p>No devices detected.</p> : null}
      </div>
    </article>
  );
}

function AboutPage() {
  return (
    <>
      <PageHeading
        eyebrow="RackForge"
        title="About"
        detail="A portable instrument host built around one shared interface and native real-time runtimes."
      />
      <section className="settings-grid">
        <article className="settings-card about-card">
          <BrandMark />
          <div className="settings-copy">
            <span className="card-kicker">Runtime protocol</span>
            <h2>rackforge.host@1</h2>
            <p>Portable .rfplugin runtime · Rust core · native audio and MIDI.</p>
          </div>
        </article>
      </section>
    </>
  );
}

function EmptyState({ title }: { title: string }) {
  return (
    <div className="empty-state">
      <span>RF</span>
      <h2>{title}</h2>
      <p>RackForge will update this view as soon as Core is available.</p>
    </div>
  );
}
