import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useId,
  useMemo,
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
  Info,
  LogOut,
  Menu,
  Keyboard,
  MonitorSmartphone,
  Piano,
  Play,
  RadioTower,
  Settings2,
  Sliders,
  Trash2,
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
  useSearchParams,
} from "react-router";
import {
  connectGateway,
  deletePluginPreset,
  exportPluginPreset,
  importPluginPreset,
  inspectPluginPreset,
  dispatchCommand,
  dispatchCommandAwait,
  loadPluginPreset,
  materializePluginState,
  requestPluginParameters,
  requestPluginStateParameters,
  requestPluginPresets,
  requestSessionSnapshot,
  renamePluginPreset,
  savePluginPreset,
  setPluginParameter,
  setPluginStateParameter,
  stopGateway,
  sendVirtualMidi,
  subscribeOutputMeter,
} from "./gateway";
import {
  type LightingMode,
  readLighting,
  storeLighting,
} from "./lighting";
import { readScreenGlass, storeScreenGlass, type ScreenGlass } from "./screen";
import { RfLoader } from "./components/RfLoader";
import { AsyncActionLabel, AsyncSpinner } from "./components/AsyncSpinner";
import { PluginRuntimeStatus } from "./components/PluginRuntimeStatus";
import { PerformanceInfoBar } from "./components/PerformanceInfoBar";
import { ModalDialog } from "./components/ModalDialog";
import { ParameterLinkHost } from "./components/ParameterLinkHost";
import { ToggleSwitch } from "./components/ToggleSwitch";
import { AsyncNotice, AsyncStateBoundary } from "./components/AsyncStateBoundary";
import { RfButton } from "./ui/RfButton";
import { useSurfaceTransition } from "./ui/useSurfaceTransition";
import {
  bindNativePluginResource,
  hostHaptic,
  hostJson,
  HostRequestError,
  IS_BROWSER_HOST,
  isDesktopHost,
  isNativeHost,
  isRemoteWebClient,
  isVstHost,
  readNativeTextFile,
  savePortableTextFile,
  selectNativePluginSound,
  selectNativeResource,
  syncNativeRoute,
} from "./host";
import {
  beginPluginOperation,
  invalidatePluginCatalog,
  refreshPluginCatalog,
  synchronizePluginRuntime,
  usePluginCatalog,
  usePluginDescriptor,
} from "./pluginCatalog";
import { pluginContextInstance } from "./pluginContext";
import {
  commitPlayPluginSelection,
  preflightPlayPluginSelection,
} from "./playPluginSelection";
import {
  defaultInstrument,
  firstRunView,
  markFirstRunCompleted,
  readFirstRunCompleted,
  shouldRunFirstRun,
} from "./firstRun";
import { FirstRunScreen } from "./FirstRunScreen";
import { LivePage, type PerformanceGraphWorkspace } from "./LivePage";
import { TouchControllerPage } from "./TouchControllerPage";
import {
  controllerPresentationTransition,
  controllerIsAvailable,
  controllerIsDockable,
  IMMERSIVE_CONTROLLER_QUERY,
} from "./controllerPresentation";
import type { RootState } from "./store";
import type {
  PluginInstance,
  OutputMeterSnapshot,
  ConnectionStatus,
  HostPresetSummary,
  HostAudioPreferences,
  HostAudioSettings,
  ProgramEditorField,
  ProgramEditorPage,
  ProgramEditorValue,
  PluginWebDescriptor,
  PluginWebSurfaceKind,
  PluginResourceRequirement,
  PluginStateReference,
  ResourceEntry,
  ResourceGrant,
  ResourceSelection,
  SessionSnapshot,
  WebAuthStatus,
  WebPublicConfig,
  PresetImportConflictPolicy,
  RfPresetFile,
  RfPresetImportPreview,
} from "./types";

const ResourceExplorerDialog = lazy(() =>
  import("./ResourceExplorerDialog").then((module) => ({
    default: module.ResourceExplorerDialog,
  })),
);

// Inline rather than an <img> so the mark reads the lighting tokens: an image
// is its own document and cannot see the faceplate's palette.
//
// The paint order is the drawing. The leg is stroked first and the bowl of the
// R over it, so the bar truncates the leg at its lower edge instead of letting
// the diagonal tip run into the red; the node at the crossbar goes last because
// it covers the seam where three strokes meet. Reordering these is a visual
// change, not a refactor.
function BrandMark() {
  // Two node holes are punched through the strokes underneath, so the knockout
  // has to be a mask. Ids must not collide between the rail and the about card.
  const maskId = `rf-mark-${useId().replace(/:/g, "")}`;
  return (
    <span className="brand-mark" aria-hidden="true">
      <svg viewBox="0 0 704 308" xmlns="http://www.w3.org/2000/svg">
        <defs>
          <mask id={maskId} maskUnits="userSpaceOnUse" x="0" y="0" width="800" height="400">
            <rect width="800" height="400" fill="#fff" />
            <circle cx="80" cy="200" r="10" fill="#000" />
            <circle cx="550" cy="200" r="12" fill="#000" />
          </mask>
        </defs>
        <g
          mask={`url(#${maskId})`}
          transform="translate(-58 -51)"
          fill="none"
          strokeWidth="34"
          strokeLinecap="butt"
          strokeLinejoin="round"
        >
          <path d="M305 200L430 342H505Q550 342 550 297V200" stroke="var(--mark-leg)" />
          <path d="M550 200V111Q550 68 595 68H720" stroke="var(--mark-arm)" />
          <path d="M550 200H731" stroke="var(--mark-arm)" />
          <path
            d="M80 200H360Q410 200 410 150V110Q410 68 365 68H155V145"
            stroke="var(--mark-bowl)"
          />
          <circle cx="80" cy="200" r="22" fill="var(--mark-bowl)" stroke="none" />
          <circle cx="550" cy="200" r="24" fill="var(--mark-arm)" stroke="none" />
          <path d="M720 174L762 200L720 226Z" fill="var(--mark-arm)" stroke="none" />
        </g>
      </svg>
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

function pluginKindPresentation(kind: PluginWebDescriptor["kind"] | undefined) {
  switch (kind) {
    case "effect":
      return { label: "Effect", className: "effect" };
    case "midi_processor":
      return { label: "MIDI Processor", className: "midi-processor" };
    default:
      return { label: "Instrument", className: "instrument" };
  }
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

const liveNavItem = {
    path: "/live",
    label: "Live",
    detail: "Performance racks, songs and setlists",
    section: "workspace",
    icon: RadioTower,
    tint: "live",
  } as const;
const playNavItem = {
    path: "/play",
    label: "Play",
    detail: "Play and edit the active instrument",
    section: "workspace",
    icon: Play,
    tint: "play",
  } as const;
const touchControllerNavItem = {
    path: "/controller",
    label: "Touch Controller",
    detail: "On-screen keyboard and pads",
    section: "workspace",
    icon: Piano,
    tint: "controller",
  } as const;
const pluginManagerNavItem = {
    path: "/plugins",
    label: "Plugin Manager",
    detail: "Install, manage and configure instruments",
    section: "system",
    icon: Blocks,
    tint: "system",
  } as const;
const settingsNavItem = {
    path: "/settings",
    label: "Settings",
    detail: "Audio, MIDI and host configuration",
    section: "system",
    icon: Settings2,
    tint: "system",
  } as const;

const aboutItem = {
  path: "/about",
  label: "About RackForge",
  detail: "Version and runtime information",
  section: "system",
  icon: Info,
  tint: "system",
} as const;

/* No Home. RackForge is for playing, so you are either in LIVE or in PLAY —
   a dashboard that restated the topbar's readout and the rail's own list was
   a page about the machine rather than a place to work. The compact layout on
   a hardware controller still has a root to navigate from; that lives in the
   `little@1` contract, where four soft keys genuinely need somewhere to start
   from, not here where the rail is always on screen. */
const workspaceNavItems = [
  liveNavItem,
  playNavItem,
  touchControllerNavItem,
];
const systemNavItems = [pluginManagerNavItem, settingsNavItem, aboutItem];
const navItems = [...workspaceNavItems, ...systemNavItems];
const vstWorkspaceNavItems = [playNavItem];
const vstSystemNavItems = [pluginManagerNavItem, aboutItem];
const vstNavItems = [...vstWorkspaceNavItems, ...vstSystemNavItems];

function useMediaQuery(query: string) {
  const [matches, setMatches] = useState(() => window.matchMedia(query).matches);

  useEffect(() => {
    const media = window.matchMedia(query);
    const update = () => setMatches(media.matches);
    update();
    media.addEventListener("change", update);
    window.addEventListener("resize", update);
    window.addEventListener("orientationchange", update);
    window.visualViewport?.addEventListener("resize", update);
    return () => {
      media.removeEventListener("change", update);
      window.removeEventListener("resize", update);
      window.removeEventListener("orientationchange", update);
      window.visualViewport?.removeEventListener("resize", update);
    };
  }, [query]);

  return matches;
}

export function App() {
  const [auth, setAuth] = useState<WebAuthStatus | null>(null);
  const [authError, setAuthError] = useState<string | null>(null);

  useEffect(() => {
    const preventNativeContextMenu = (event: MouseEvent) => event.preventDefault();
    const attachedDocuments = new Set<Document>();
    const attach = (target: Document) => {
      if (attachedDocuments.has(target)) return;
      attachedDocuments.add(target);
      target.addEventListener("contextmenu", preventNativeContextMenu, true);
    };
    const scanFrames = () => {
      attach(document);
      for (const frame of document.querySelectorAll("iframe")) {
        try {
          if (frame.contentDocument) attach(frame.contentDocument);
        } catch {
          // Cross-origin content keeps its own browser policy. RackForge and
          // installed plugin surfaces are intentionally same-origin.
        }
      }
    };
    const frameLoaded = (event: Event) => {
      if (event.target instanceof HTMLIFrameElement) scanFrames();
    };
    scanFrames();
    document.addEventListener("load", frameLoaded, true);
    const observer = new MutationObserver(scanFrames);
    observer.observe(document.documentElement, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      document.removeEventListener("load", frameLoaded, true);
      for (const target of attachedDocuments) {
        try {
          target.removeEventListener("contextmenu", preventNativeContextMenu, true);
        } catch {
          // A navigated or removed iframe no longer needs cleanup.
        }
      }
    };
  }, []);

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

/**
 * The lighting condition the interface is actually rendering in.
 *
 * Plugins are told the condition, never the preference. "Auto" is a statement
 * about *who decides*, which is the host's business; a plugin surface only
 * needs to know which of the two it is being drawn next to. So this resolves
 * auto and reports "day" or "stage".
 *
 * Two things can change it — the player throwing the switch, which stamps the
 * document, and the operating system flipping underneath an unstamped one —
 * so both are watched.
 */
function useResolvedLighting(): "day" | "stage" {
  const systemDark = useMediaQuery("(prefers-color-scheme: dark)");
  const [stamp, setStamp] = useState<string | null>(() =>
    document.documentElement.getAttribute("data-theme"),
  );
  useEffect(() => {
    const root = document.documentElement;
    const observer = new MutationObserver(() =>
      setStamp(root.getAttribute("data-theme")),
    );
    observer.observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);
  if (stamp === "dark") return "stage";
  if (stamp === "light") return "day";
  return systemDark ? "stage" : "day";
}

/**
 * The first run, driven by what the host actually reports.
 *
 * It waits for the catalogue rather than for a clock, opens the default
 * instrument through the same path the plugin picker uses, and remembers
 * that it did. Every exit — success, no instruments, a failed activation, or
 * a host that never answers — ends with the interface uncovered.
 */
function useFirstRun({
  catalogStatus,
  plugins,
  sessionKnown,
  activeInstanceId,
  navigate,
}: {
  catalogStatus: "idle" | "loading" | "ready" | "error";
  plugins: PluginWebDescriptor[];
  sessionKnown: boolean;
  activeInstanceId: string | undefined;
  navigate: ReturnType<typeof useNavigate>;
}) {
  const [completed, setCompleted] = useState(readFirstRunCompleted);
  const [activationFailure, setActivationFailure] = useState<string | null>(null);
  const [timedOut, setTimedOut] = useState(false);
  const started = useRef(false);
  const active = shouldRunFirstRun({
    completed,
    sessionKnown,
    hasActiveInstance: Boolean(activeInstanceId),
  });

  // Read, not held: a catalogue that cannot be read, or one with nothing in
  // it, is a state of the host rather than of this screen.
  const failure =
    activationFailure ??
    (catalogStatus === "error"
      ? "RackForge could not read its plugin catalogue."
      : catalogStatus === "ready" && defaultInstrument(plugins) === null
        ? "This RackForge has no instruments installed yet."
        : timedOut
          ? "RackForge is taking longer than usual to start."
          : null);

  const finish = useCallback(() => {
    markFirstRunCompleted();
    setCompleted(true);
  }, []);

  // A machine that already has an instrument playing has been used before,
  // whatever this browser remembers. `active` is already false by then; this
  // only writes it down so the screen stays away.
  useEffect(() => {
    if (activeInstanceId) markFirstRunCompleted();
  }, [activeInstanceId]);

  useEffect(() => {
    if (!active || started.current) return;
    if (catalogStatus !== "ready") return;
    const target = defaultInstrument(plugins);
    if (!target) return;
    started.current = true;
    void (async () => {
      try {
        await commitPlayPluginSelection(
          {
            target: { pluginId: target.plugin_id, pluginName: target.plugin_name },
          },
          {
            dispatch: dispatchCommandAwait,
            activate: (pluginId) =>
              hostJson(`/api/v1/plugins/${encodeURIComponent(pluginId)}/activate`, {
                method: "POST",
              }),
            synchronize: synchronizePluginEnvironment,
          },
        );
        navigate("/play");
        finish();
      } catch (error) {
        setActivationFailure(
          error instanceof Error
            ? error.message
            : "RackForge could not open your instrument.",
        );
      }
    })();
  }, [active, catalogStatus, plugins, navigate, finish]);

  // And a host that never answers at all still hands the interface over.
  useEffect(() => {
    if (!active) return;
    const timer = window.setTimeout(() => setTimedOut(true), 25_000);
    return () => window.clearTimeout(timer);
  }, [active]);

  const view = useMemo(
    () => firstRunView({ catalogStatus, plugins, failure }),
    [catalogStatus, plugins, failure],
  );

  return { active, view, failure, dismiss: finish };
}

function RackForgeApp() {
  const { connection, snapshot, performance, performancePending, error } = useSelector(
    (state: RootState) => state.rackforge,
  );
  const pluginCatalog = usePluginCatalog();
  const location = useLocation();
  const navigate = useNavigate();
  const firstRun = useFirstRun({
    catalogStatus: pluginCatalog.status,
    plugins: pluginCatalog.plugins,
    sessionKnown: Boolean(snapshot),
    activeInstanceId: snapshot?.active_instance_id,
    navigate,
  });
  const vstHost = isVstHost();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [installPluginOpen, setInstallPluginOpen] = useState(false);
  const [playOverlay, setPlayOverlay] = useState<"plugins" | "presets" | null>(null);
  const [liveSurface, setLiveSurface] = useState<"perform" | "configure">("perform");
  const [liveWorkspace, setLiveWorkspace] = useState<PerformanceGraphWorkspace | null>(null);
  const [rackGraphOverlayOpen, setRackGraphOverlayOpen] = useState(false);
  const [playTransitionOpen, setPlayTransitionOpen] = useState(false);
  const [preferredPlayInstanceId, setPreferredPlayInstanceId] = useState<string | null>(null);
  const pendingPlayInstance = useRef<PluginInstance | null>(null);
  const [settingsBootstrap, setSettingsBootstrap] = useState<HostSettingsBootstrap | null>(null);
  const [controllerDockOpen, setControllerDockOpen] = useState(false);
  const immersiveController = useMediaQuery(IMMERSIVE_CONTROLLER_QUERY);
  /* Where the Touch Controller lives is decided in `controllerPresentation`,
     so the rule can be read and tested on its own. The two `useMemo`s look
     like ceremony around a boolean, but the React compiler will not fold a
     call it cannot see into, and without them it stops preserving the
     memoisation of the play-navigation callbacks further down. */
  const controllerAvailable = useMemo(() => controllerIsAvailable(vstHost), [vstHost]);
  const dockableController = useMemo(
    () => controllerIsDockable({ vstHost, immersive: immersiveController }),
    [vstHost, immersiveController],
  );
  const lastContentRoute = useRef(location.pathname === "/controller" ? "/play" : location.pathname);
  useEffect(() => {
    const updateOverlay = (event: Event) => {
      const detail = (event as CustomEvent<{ open?: boolean }>).detail;
      setRackGraphOverlayOpen(detail?.open === true);
    };
    window.addEventListener("rackforge:rack-graph-overlay", updateOverlay);
    return () => window.removeEventListener("rackforge:rack-graph-overlay", updateOverlay);
  }, []);
  /* Where the controller is not available at all the route sends the player
     to PLAY; until it does, the path still reads `/controller`, and calling
     that a controller surface would blank the topbar for a frame. */
  const isControllerSurface =
    location.pathname === "/controller" && controllerAvailable && !dockableController;
  const isPluginSurface =
    location.pathname === "/play" ||
    location.pathname.startsWith("/plugins/");
  const isPerformanceSurface =
    location.pathname === "/play" || location.pathname === "/live";
  const isLiveSurface = location.pathname === "/live";
  const routeSurfaceRef = useRef<HTMLDivElement | null>(null);
  useSurfaceTransition(routeSurfaceRef, location.key, isControllerSurface);
  const renderRackSlotPluginSurface = useCallback(
    ({
      instance,
      state,
      onStateChange,
      onSelectSound,
      parameterLinkInstanceId,
    }: {
      instance: PluginInstance;
      state?: PluginStateReference;
      onStateChange: (state: PluginStateReference) => void;
      onSelectSound: (soundId: string) => Promise<unknown>;
      parameterLinkInstanceId: string;
    }) => (
      <PluginFrame
        instance={instance}
        surface="play"
        isolated
        isolatedState={state}
        onIsolatedStateChange={onStateChange}
        onSelectSound={onSelectSound}
        parameterLinkInstanceId={parameterLinkInstanceId}
      />
    ),
    [],
  );

  useEffect(() => {
    connectGateway();
    void refreshPluginCatalog().catch(() => undefined);
    return stopGateway;
  }, []);

  useEffect(() => {
    synchronizePluginRuntime(snapshot, connection);
  }, [connection, snapshot]);

  useEffect(() => {
    if (vstHost) return;
    let active = true;
    requestHostSettingsBootstrap().then((bootstrap) => {
      if (active) setSettingsBootstrap(bootstrap);
    });
    return () => {
      active = false;
    };
  }, [vstHost]);

  useEffect(() => {
    syncNativeRoute(location.pathname);
  }, [location.pathname]);

  useEffect(() => {
    if (location.pathname !== "/controller") {
      lastContentRoute.current = location.pathname;
    }
    const transition = controllerPresentationTransition({
      dockable: dockableController,
      dockOpen: controllerDockOpen,
      pathname: location.pathname,
      lastContentRoute: lastContentRoute.current,
    });
    if (!transition) return;
    let active = true;
    window.queueMicrotask(() => {
      if (!active) return;
      if (transition.openDock) {
        setControllerDockOpen(true);
      }
      navigate(transition.navigateTo, { replace: true });
    });
    return () => {
      active = false;
    };
  }, [controllerDockOpen, dockableController, location.pathname, navigate]);

  const showControllerDock = dockableController && controllerDockOpen;
  const completePlayNavigation = useCallback(async (instance?: PluginInstance) => {
    setPreferredPlayInstanceId(instance?.instance_id ?? null);
    if (instance) {
      try {
        await dispatchCommandAwait({ type: "set_active_mode", mode: "play" });
        await dispatchCommandAwait({
          type: "select_plugin",
          instance_id: instance.instance_id,
        });
      } catch {
        // Command failures are published through the shared RackForge banner.
        return;
      }
    }
    navigate("/play");
  }, [navigate]);

  const requestPlayNavigation = useCallback((instance?: PluginInstance) => {
    setMobileMenuOpen(false);
    if (location.pathname === "/play" && !instance) return;
    const liveOutputActive = snapshot?.active_mode === "live" && (
      snapshot.live.active !== undefined || liveWorkspace !== null
    );
    if (liveOutputActive) {
      pendingPlayInstance.current = instance ?? null;
      setPlayTransitionOpen(true);
      return;
    }
    void completePlayNavigation(instance);
  }, [completePlayNavigation, liveWorkspace, location.pathname, snapshot]);

  const pendingPreferredPlayInstanceId =
    preferredPlayInstanceId &&
    snapshot?.active_mode === "play" &&
    snapshot.active_instance_id === preferredPlayInstanceId
      ? null
      : preferredPlayInstanceId;

  return (
    <div className={`app-shell${vstHost ? " vst-host" : ""}${isPluginSurface ? " plugin-surface-active" : ""}${
      isControllerSurface ? " controller-surface-active" : ""
    }${isPerformanceSurface ? " performance-surface-active" : ""}${
      isLiveSurface ? " live-surface-active" : ""
    }${liveWorkspace ? " graph-workspace-active" : ""
    }${showControllerDock ? " controller-dock-active" : ""
    }`}>
      <aside className="rail">
        <div className="brand-lockup" aria-label="RackForge">
          <BrandMark />
          <span className="brand-name">RACKFORGE</span>
        </div>
        {/* The rail lists destinations. The Touch Controller is not one — it
            pulls a dock out over whatever surface you are already on — so it
            sits with the chassis furniture at the foot instead, and only where
            it is actually a dock. On a phone it stays in the drawer's list,
            where it really is a route. */}
        <NavigationLinks
          items={(vstHost ? vstNavItems : navItems).filter(
            (item) => !(dockableController && item.path === "/controller"),
          )}
          onPlayRequest={requestPlayNavigation}
        />
        {liveWorkspace ? (
          <nav className="rail-context-actions" aria-label="Graph editor actions">
            <span className="rail-context-label">
              {liveWorkspace.kind === "rack" ? "Rack editor" : "Song Part editor"}
            </span>
            <button
              type="button"
              className="nav-item"
              onClick={() => window.dispatchEvent(new Event("rackforge:save-graph-workspace"))}
            >
              <span className="nav-mark"><Activity aria-hidden="true" strokeWidth={1.9} /></span>
              <span className="nav-copy">
                <span>{liveWorkspace.kind === "rack" ? "Save Rack" : "Save Song Part"}</span>
              </span>
            </button>
            <button
              type="button"
              className="nav-item"
              onClick={() => window.dispatchEvent(new Event("rackforge:close-graph-workspace"))}
            >
              <span className="nav-mark"><Blocks aria-hidden="true" strokeWidth={1.9} /></span>
              <span className="nav-copy">
                <span>{liveWorkspace.kind === "rack" ? "Back to LIVE" : "Back to Song"}</span>
              </span>
            </button>
          </nav>
        ) : null}
        {dockableController ? (
          <ControllerDockToggle
            open={showControllerDock}
            onToggle={() => setControllerDockOpen((open) => !open)}
          />
        ) : null}
        <LightingSwitch />
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
            showGraphDetails={liveWorkspace !== null}
            graphDetailsButtonVisible={liveWorkspace !== null && !rackGraphOverlayOpen}
          />
        ) : null}
        {error && <div className="error-banner">{error}</div>}
        <div
          ref={routeSurfaceRef}
          className={`page${isPluginSurface ? " plugin-host-page" : ""}${
            isControllerSurface ? " controller-host-page" : ""
          }${
            isLiveSurface ? " live-host-page" : ""
          }`}
        >
          <Routes>
            <Route path="/" element={<Navigate to="/play" replace />} />
            <Route
              path="/live"
              element={vstHost ? <Navigate to="/play" replace /> :
                <LivePage
                  session={snapshot}
                  performance={performance}
                  plugins={pluginCatalog.plugins}
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
                  preferredInstanceId={pendingPreferredPlayInstanceId}
                />
              }
            />
            <Route
              path="/controller"
              element={vstHost ? <Navigate to="/play" replace /> :
                <TouchControllerPage
                  snapshot={snapshot}
                  connection={connection}
                  onOpenNavigation={() => setMobileMenuOpen(true)}
                  onExit={requestPlayNavigation}
                />
              }
            />
            <Route
              path="/plugins"
              element={
                <PluginsPage
                  snapshot={snapshot}
                  onInstall={() => setInstallPluginOpen(true)}
                  showControllers={!vstHost}
                />
              }
            />
            <Route
              path="/plugins/:instanceId"
              element={<PluginPage snapshot={snapshot} connection={connection} />}
            />
            <Route path="/controllers/:controllerId" element={<ControllerPage />} />
            <Route
              path="/settings"
              element={vstHost ? <Navigate to="/plugins" replace /> : settingsBootstrap ? (
                <SettingsPage
                  initial={settingsBootstrap}
                  onConfigChange={(config) => setSettingsBootstrap((current) =>
                    current ? { ...current, config } : current
                  )}
                  onAudioChange={(audioSettings) => setSettingsBootstrap((current) =>
                    current ? { ...current, audioSettings } : current
                  )}
                />
              ) : (
                <RfLoader label="Settings" detail="Reading host capabilities…" size="medium" />
              )}
            />
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
      <TypingKeyboardListener />
      {mobileMenuOpen ? (
        <MobileNavigation
          vstHost={vstHost}
          connection={connection}
          onClose={() => setMobileMenuOpen(false)}
          performanceSurface={
            location.pathname === "/play"
              ? "play"
              : location.pathname === "/live"
                ? "live"
                : undefined
          }
          liveWorkspaceActive={liveWorkspace !== null}
          liveWorkspaceKind={liveWorkspace?.kind}
          dockableController={dockableController}
          controllerDockOpen={showControllerDock}
          onControllerToggle={() => setControllerDockOpen((open) => !open)}
          liveSetlistSelected={
            liveSurface === "perform" &&
            performance?.live.mode === "setlist" &&
            performance.live.setlist?.kind === "setlist"
          }
          onPlayRequest={requestPlayNavigation}
          onPerformanceAction={(action) => {
            setMobileMenuOpen(false);
            if (action === "select-plugin") setPlayOverlay("plugins");
            if (action === "presets") setPlayOverlay("presets");
            if (action === "live-perform") setLiveSurface("perform");
            if (action === "live-configure") setLiveSurface("configure");
            if (action === "live-exit-setlist") {
              setLiveSurface("perform");
              window.dispatchEvent(new Event("rackforge:exit-live-setlist"));
            }
            if (action === "live-save-editor") {
              window.dispatchEvent(new Event("rackforge:save-graph-workspace"));
            }
            if (action === "live-close-editor") {
              window.dispatchEvent(new Event("rackforge:close-graph-workspace"));
            }
          }}
        />
      ) : null}
      {installPluginOpen ? (
        <InstallPluginDialog onClose={() => setInstallPluginOpen(false)} />
      ) : null}
      {firstRun.active ? (
        <FirstRunScreen
          view={firstRun.view}
          failure={firstRun.failure}
          onDismiss={firstRun.dismiss}
        />
      ) : null}
      {playTransitionOpen ? (
        <PlayModeTransitionDialog
          onCancel={() => {
            pendingPlayInstance.current = null;
            setPlayTransitionOpen(false);
          }}
          onConfirm={() => {
            const instance = pendingPlayInstance.current ?? undefined;
            pendingPlayInstance.current = null;
            setPlayTransitionOpen(false);
            void completePlayNavigation(instance);
          }}
        />
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
  showGraphDetails,
  graphDetailsButtonVisible,
}: {
  menuOpen: boolean;
  onOpen: () => void;
  showGraphDetails: boolean;
  graphDetailsButtonVisible: boolean;
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
    const verticalOffset = showGraphDetails ? 110 : 60;
    const availableY = Math.max(1, shell.height - verticalOffset);
    const nextPosition = {
      x: Math.min(1, Math.max(0, (clientX - shell.left - gesture.grabX - 8) / availableX)),
      y: Math.min(1, Math.max(0, (clientY - shell.top - gesture.grabY - 8) / availableY)),
    };
    positionRef.current = nextPosition;
    setPosition(nextPosition);
  }, [showGraphDetails]);

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

  const anchorLeft = `calc(8px + ${position.x * 100}% - ${position.x * 60}px)`;
  const anchorTopOffset = position.y * (showGraphDetails ? 110 : 60);
  const anchorTop = `calc(8px + ${position.y * 100}% - ${anchorTopOffset}px)`;

  return (
    <>
    <button
      ref={buttonRef}
      className={`performance-menu-button${dragging ? " dragging" : ""}`}
      style={{
        left: anchorLeft,
        top: anchorTop,
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
    {graphDetailsButtonVisible ? (
      <button
        type="button"
        className="rack-details-floating-button"
        style={{
          left: anchorLeft,
          top: `calc(60px + ${position.y * 100}% - ${anchorTopOffset}px)`,
        }}
        aria-label="Open workspace details"
        onClick={() => window.dispatchEvent(new Event("rackforge:open-graph-details"))}
      >
        <Settings2 aria-hidden="true" strokeWidth={1.9} />
      </button>
    ) : null}
    </>
  );
}


const LIGHTING_CHOICES: ReadonlyArray<{
  value: LightingMode;
  key: string;
  title: string;
}> = [
  { value: "auto", key: "AUTO", title: "Follow the system setting" },
  { value: "light", key: "DAY", title: "DAYLIGHT — bench and rehearsal" },
  { value: "dark", key: "STAGE", title: "STAGE — house lights down" },
];

/**
 * Slides the on-screen keyboard out over the current surface.
 *
 * Deliberately quieter than a nav key: it is a latching switch on the chassis,
 * not a place you go, and it sits with the lighting selector rather than among
 * the destinations it opens on top of.
 */
function ControllerDockToggle({
  open,
  onToggle,
}: {
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className={`controller-dock-toggle${open ? " open" : ""}`}
      aria-pressed={open}
      onClick={onToggle}
    >
      <Piano aria-hidden="true" strokeWidth={1.9} />
      <span>Touch Controller</span>
    </button>
  );
}

/** Three-position lighting selector, mounted on the chassis rail. */
function LightingSwitch() {
  const [mode, setMode] = useState<LightingMode>(() => readLighting());

  const choose = (next: LightingMode) => {
    setMode(next);
    storeLighting(next);
  };

  return (
    <div className="lighting-switch" role="group" aria-label="Lighting condition">
      <span className="lighting-switch-label">Lighting</span>
      <div className="lighting-switch-keys">
        {LIGHTING_CHOICES.map((choice) => (
          <button
            key={choice.value}
            type="button"
            className={`lighting-key${mode === choice.value ? " active" : ""}`}
            aria-pressed={mode === choice.value}
            title={choice.title}
            onClick={() => choose(choice.value)}
          >
            {choice.key}
          </button>
        ))}
      </div>
    </div>
  );
}

function NavigationLinks({
  items = navItems,
  detailed = false,
  onNavigate,
  onPlayRequest,
  controllerDockOpen = false,
  onControllerToggle,
}: {
  items?: typeof navItems;
  detailed?: boolean;
  onNavigate?: () => void;
  onPlayRequest?: () => void;
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
              className={`nav-item nav-tint-${item.tint} controller-toggle${
                controllerDockOpen ? " dock-open" : ""
              }`}
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
            end={false}
            onClick={(event) => {
              if (item.path === "/play" && onPlayRequest) {
                event.preventDefault();
                onNavigate?.();
                onPlayRequest();
                return;
              }
              onNavigate?.();
            }}
            className={({ isActive }) =>
              `nav-item nav-tint-${item.tint}${isActive ? " active" : ""}`
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
  vstHost,
  connection,
  onClose,
  performanceSurface,
  liveWorkspaceActive,
  liveWorkspaceKind,
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
  liveWorkspaceActive: boolean;
  liveWorkspaceKind?: PerformanceGraphWorkspace["kind"];
  liveSetlistSelected: boolean;
  onPlayRequest: () => void;
  dockableController: boolean;
  controllerDockOpen: boolean;
  onControllerToggle: () => void;
  onPerformanceAction: (
    action: "select-plugin" | "presets" | "live-perform" | "live-configure" | "live-exit-setlist" | "live-save-editor" | "live-close-editor",
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
                            <span>Save workspace</span>
                            <small>Store this portable node graph</small>
                          </span>
                        </button>
                        <button
                          className="nav-item"
                          onClick={() => onPerformanceAction("live-close-editor")}
                        >
                          <span className="nav-mark"><Blocks aria-hidden="true" strokeWidth={1.9} /></span>
                          <span className="nav-copy">
                            <span>{liveWorkspaceKind === "song_part" ? "Back to Song" : "Back to LIVE library"}</span>
                            <small>{liveWorkspaceKind === "song_part" ? "Close the Song Part graph" : "Close the full-screen Rack workspace"}</small>
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

interface HostHealth {
  revision?: string;
  ui_revision?: string;
  host?: string;
}

/** What the host binary says about itself. Absent until it answers. */
function useHostHealth() {
  const [host, setHost] = useState<HostHealth | null>(null);
  useEffect(() => {
    let cancelled = false;
    hostJson<HostHealth>("/api/v1/health")
      .then((health) => {
        if (!cancelled) setHost(health);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  return host;
}

// Drift made visible: the revision this interface was built from, beside
// the revision the host binary reports. When they disagree, someone shipped
// half a deploy, and the mismatch says so before a behavior difference does.
function RevisionFooter() {
  const host = useHostHealth();
  const mismatch =
    host?.ui_revision !== undefined &&
    host.ui_revision !== "unknown" &&
    host.ui_revision !== __UI_REVISION__;
  const stale = host?.revision !== undefined && host.revision !== __UI_REVISION__;
  return (
    <p className={`revision-footer${mismatch || stale ? " drift" : ""}`}>
      UI {__UI_REVISION__}
      {host?.revision ? ` · host ${host.revision}` : ""}
      {mismatch || stale ? " · out of sync" : ""}
    </p>
  );
}

function PlayModeTransitionDialog({
  onCancel,
  onConfirm,
}: {
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <ModalDialog
      eyebrow="LIVE output active"
      title="Switch to PLAY?"
      role="alertdialog"
      onClose={onCancel}
      closeLabel="Stay in LIVE"
      backdropClassName="play-mode-transition-backdrop"
      className="play-mode-transition-dialog"
      message={
        <div className="play-mode-transition-copy">
          <p>The current LIVE Rack will stop sounding and RackForge will activate the selected PLAY instrument.</p>
          <p>Continue only if you intend to leave the live performance.</p>
        </div>
      }
      actions={
        <>
          <button className="secondary-button" onClick={onCancel}>Stay in LIVE</button>
          <button className="primary-button" onClick={onConfirm}>Switch to PLAY</button>
        </>
      }
    />
  );
}

interface InstalledPluginResult {
  plugin_id: string;
  version: string;
  already_installed: boolean;
  activation_required: boolean;
}

interface PluginInstallPreview {
  selection_id: string;
  plugin_id: string;
  plugin_name: string;
  vendor: string;
  version: string;
  description?: string | null;
  kind: string;
  platform: string;
  portable: boolean;
  archive_bytes: number;
  branding?: {
    banner_data_url: string;
    background_color?: string | null;
    accent_color?: string | null;
  } | null;
}

const PLUGIN_ACTIVATION_TIMEOUT_MS = 45_000;

async function synchronizePluginEnvironment() {
  await Promise.allSettled([
    invalidatePluginCatalog(),
    requestSessionSnapshot(),
  ]);
}

async function activateInstalledPlugin(
  result: InstalledPluginResult,
): Promise<PluginWebDescriptor> {
  const finishOperation = beginPluginOperation(
    result.plugin_id,
    "activate",
    "Activating plugin…",
  );
  try {
    const activation = await hostJson<{ status?: string }>(
      `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}/activate`,
      { method: "POST" },
    );
    if (activation.status === "active") {
      const descriptor = await hostJson<PluginWebDescriptor>(
        `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
      );
      await synchronizePluginEnvironment();
      return descriptor;
    }
    const startedAt = performance.now();
    let lastError: unknown;
    while (performance.now() - startedAt < PLUGIN_ACTIVATION_TIMEOUT_MS) {
      try {
        const descriptor = await hostJson<PluginWebDescriptor>(
          `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
        );
        if (descriptor.active) {
          await synchronizePluginEnvironment();
          return descriptor;
        }
      } catch (error) {
        lastError = error;
      }
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
    throw lastError instanceof Error
      ? lastError
      : new Error("RackForge installed the plugin but activation did not finish in time.");
  } finally {
    finishOperation();
  }
}

async function setInstalledPluginActive(pluginId: string, active: boolean) {
  const action = active ? "activate" : "deactivate";
  const finishOperation = beginPluginOperation(
    pluginId,
    action,
    active ? "Activating plugin…" : "Deactivating plugin…",
  );
  try {
    const response = await hostJson<{ status?: string; plugin_id?: string }>(
      `/api/v1/plugins/${encodeURIComponent(pluginId)}/${action}`,
      { method: "POST" },
    );
    const startedAt = performance.now();
    let lastError: unknown;
    while (performance.now() - startedAt < PLUGIN_ACTIVATION_TIMEOUT_MS) {
      try {
        const descriptor = await hostJson<PluginWebDescriptor>(
          `/api/v1/plugins/${encodeURIComponent(pluginId)}`,
        );
        if (descriptor.active === active && !descriptor.transitioning) {
          await synchronizePluginEnvironment();
          return response;
        }
      } catch (error) {
        lastError = error;
      }
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
    throw lastError instanceof Error
      ? lastError
      : new Error(`RackForge did not finish ${active ? "activating" : "deactivating"} the plugin.`);
  } finally {
    finishOperation();
  }
}

const MAX_CLIENT_RESOURCE_BYTES = 512 * 1024 * 1024;

function InstallPluginDialog({ onClose }: { onClose: () => void }) {
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const native = isNativeHost();
  const desktop = isDesktopHost();
  // Browsing the host's own storage needs a host with storage to browse. A
  // page carrying its own RackForge has none, so it offers upload only.
  const remoteWeb = isRemoteWebClient();
  const [browseHost, setBrowseHost] = useState(false);
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const cancellationRequestedRef = useRef(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<PluginInstallPreview | null>(null);
  const [installed, setInstalled] = useState<InstalledPluginResult | null>(null);
  const [installedDescriptor, setInstalledDescriptor] = useState<PluginWebDescriptor | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const navigate = useNavigate();

  const releaseSelection = useCallback(async (selectionId: string) => {
    await postResourceApi("/api/v1/resources/selections/release", {
      selection_id: selectionId,
    });
  }, []);

  const closeDialog = useCallback(() => {
    if (preview && !installed) {
      void releaseSelection(preview.selection_id).catch(() => undefined);
    }
    onClose();
  }, [installed, onClose, preview, releaseSelection]);

  const inspectSelection = async (selection: ResourceSelection) => {
    setStatus(`Validating ${selection.display_name}…`);
    try {
      const inspection = await postResourceApi<PluginInstallPreview>(
        "/api/v1/plugins/inspect",
        { selection_id: selection.selection_id },
      );
      setPreview(inspection);
      setStatus(null);
    } catch (reason) {
      await releaseSelection(selection.selection_id).catch(() => undefined);
      throw reason;
    }
  };

  const finishInstall = async (result: InstalledPluginResult) => {
    setInstalled(result);
    setStatus("Refreshing the plugin library…");
    try {
      await invalidatePluginCatalog();
      const descriptor = await hostJson<PluginWebDescriptor>(
        `/api/v1/plugins/${encodeURIComponent(result.plugin_id)}`,
      );
      setInstalledDescriptor(descriptor);
      hostHaptic("confirm");
    } catch (reason) {
      setError(
        `The package is installed, but RackForge could not refresh its actions: ${
          reason instanceof Error ? reason.message : "unknown catalog error"
        }`,
      );
    } finally {
      setStatus(null);
    }
  };

  const installPreview = async () => {
    if (!preview) return;
    const selectionId = preview.selection_id;
    const finishOperation = beginPluginOperation(
      preview.plugin_id,
      "install",
      `Installing ${preview.plugin_name}…`,
    );
    cancellationRequestedRef.current = false;
    setCancelled(false);
    setCancelling(false);
    setBusy(true);
    setError(null);
    setStatus(`Installing ${preview.plugin_name}…`);
    try {
      const result = await postResourceApi<InstalledPluginResult>("/api/v1/plugins/install", {
        selection_id: selectionId,
      });
      setPreview(null);
      await finishInstall(result);
    } catch (reason) {
      setPreview(null);
      setStatus(null);
      if (
        cancellationRequestedRef.current ||
        (reason instanceof Error && reason.message.toLowerCase().includes("cancel"))
      ) {
        setCancelled(true);
        setError(null);
        void invalidatePluginCatalog().catch(() => undefined);
      } else {
        setError(reason instanceof Error ? reason.message : "Could not install this plugin.");
      }
    } finally {
      finishOperation();
      setBusy(false);
      setCancelling(false);
    }
  };

  const cancelInstallation = async () => {
    if (!preview || !busy || cancelling) return;
    cancellationRequestedRef.current = true;
    setCancelling(true);
    setStatus("Cancelling installation safely…");
    try {
      await postResourceApi("/api/v1/plugins/install/cancel", {
        selection_id: preview.selection_id,
      });
    } catch (reason) {
      cancellationRequestedRef.current = false;
      setCancelling(false);
      setStatus(`Installing ${preview.plugin_name}…`);
      setError(
        reason instanceof Error
          ? reason.message
          : "RackForge could not cancel the installation.",
      );
    }
  };

  const openInstalledPlugin = async (destination: "play" | "config") => {
    if (!installed) return;
    setBusy(true);
    setError(null);
    setStatus(
      destination === "play"
        ? `Opening ${installed.plugin_id} in PLAY…`
        : `Opening ${installed.plugin_id} configuration…`,
    );
    try {
      await activateInstalledPlugin(installed);
      const refreshed = await requestSessionSnapshot();
      const instance = refreshed.instances.find(
        (candidate) => candidate.plugin_id === installed.plugin_id,
      );
      if (destination === "config" && !instance) {
        throw new Error("RackForge activated the plugin but did not publish its configuration instance.");
      }
      navigate(
        destination === "play"
          ? "/play"
          : `/plugins/${encodeURIComponent(instance!.instance_id)}`,
      );
      onClose();
    } catch (reason) {
      setError(
        reason instanceof Error ? reason.message : "Could not open the installed plugin.",
      );
      setStatus(null);
    } finally {
      setBusy(false);
    }
  };

  const cancelPreview = () => {
    if (!preview || busy) return;
    const selectionId = preview.selection_id;
    setPreview(null);
    void releaseSelection(selectionId).catch(() => undefined);
    onClose();
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
      await inspectSelection(selection);
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
      await inspectSelection(selection);
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
      await inspectSelection(selection);
    } catch (reason) {
      setStatus(null);
      setError(
        reason instanceof Error ? reason.message : "Could not install this plugin.",
      );
    } finally {
      setBusy(false);
    }
  };

  const previewDescription = preview?.description?.trim() || (preview
    ? `${preview.kind === "instrument" ? "Instrument" : "Plugin"} by ${preview.vendor}, packaged for RackForge.`
    : "");
  const previewStyle = preview?.branding ? ({
    "--preview-accent": preview.branding.accent_color || "#55e7ff",
    "--preview-background": preview.branding.background_color || "#07131c",
  } as CSSProperties) : undefined;
  const packageSize = preview
    ? preview.archive_bytes >= 1024 * 1024
      ? `${(preview.archive_bytes / (1024 * 1024)).toFixed(1)} MB`
      : `${Math.max(1, Math.round(preview.archive_bytes / 1024))} KB`
    : "";
  const installing = busy && preview !== null && installed === null;
  const canConfigure = installedDescriptor?.surfaces.some(
    (surface) => surface.kind === "config",
  ) ?? false;
  const dialogTitle = installed
    ? installed.already_installed ? "Plugin already installed" : "Plugin installed"
    : cancelled
      ? "Installation cancelled"
      : preview
        ? installing ? "Installing plugin" : "Review plugin"
        : "Install plugin";
  const dialogActions = installing ? (
    <button
      type="button"
      className="secondary-button"
      disabled={cancelling}
      onClick={() => void cancelInstallation()}
    >
      <AsyncActionLabel active={cancelling} activeLabel="Cancelling…">
        Cancel installation
      </AsyncActionLabel>
    </button>
  ) : preview ? (
    <>
      <button className="secondary-button" onClick={cancelPreview} disabled={busy}>
        Cancel
      </button>
      <button className="primary-button" onClick={() => void installPreview()} disabled={busy}>
        Install
      </button>
    </>
  ) : installed ? (
    <>
      <button className="secondary-button" onClick={closeDialog} disabled={busy}>
        Close
      </button>
      {canConfigure ? (
        <button
          className="secondary-button"
          onClick={() => void openInstalledPlugin("config")}
          disabled={busy}
        >
          Open configuration
        </button>
      ) : null}
      <button
        className="primary-button"
        onClick={() => void openInstalledPlugin("play")}
        disabled={busy}
      >
        <AsyncActionLabel active={busy} activeLabel="Opening…">
          Open in PLAY
        </AsyncActionLabel>
      </button>
    </>
  ) : cancelled || error ? (
    <button className="secondary-button" onClick={closeDialog} disabled={busy}>
      Close
    </button>
  ) : undefined;

  return (
    <>
      <ModalDialog
        eyebrow="Portable package"
        title={dialogTitle}
        onClose={closeDialog}
        dismissible={!busy && !browseHost}
        showClose={!installing}
        closeLabel="Close plugin installer"
        backdropClassName="install-plugin-backdrop"
        className="install-plugin-dialog"
        actions={dialogActions}
      >
        {!installed && !preview ? <p className="install-plugin-intro">
          {native || desktop
            ? "Select a portable .rfplugin package. RackForge validates it before installing anything."
            : "Choose where the .rfplugin package is located. RackForge validates it on the host before installing anything."}
        </p> : null}
        {!installed && !preview ? <div className="install-plugin-sources">
          <button
            type="button"
            className="install-source-card"
            disabled={busy}
            onClick={() => {
              if (native || desktop) void openNativePicker();
              else fileInputRef.current?.click();
            }}
          >
            <span className="install-source-icon">
              {desktop ? <FolderOpen aria-hidden="true" /> : <FileUp aria-hidden="true" />}
            </span>
            <span>
              <strong>{native || desktop ? "Choose plugin package" : "Upload from this device"}</strong>
              {remoteWeb ? (
                <small>Use the browser picker, then securely upload to the host</small>
              ) : null}
            </span>
          </button>
          {remoteWeb ? (
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
        {preview ? (
          <div className="plugin-install-preview" style={previewStyle}>
            <div className={`plugin-install-preview-banner${preview.branding ? " branded" : ""}`}>
              {preview.branding ? (
                <img src={preview.branding.banner_data_url} alt={`${preview.plugin_name} banner`} />
              ) : (
                <span aria-hidden="true">RF</span>
              )}
            </div>
            <div className="plugin-install-preview-copy">
              <span className="eyebrow">READY TO INSTALL</span>
              <h3>{preview.plugin_name} <small>v{preview.version}</small></h3>
              <p>{previewDescription}</p>
              <div className="plugin-install-preview-meta" aria-label="Package details">
                <span>{preview.vendor}</span>
                <span>{preview.kind}</span>
                <span>{preview.portable ? "Portable" : preview.platform}</span>
                <span>{packageSize}</span>
              </div>
            </div>
          </div>
        ) : null}
        {status ? (
          <p className="install-plugin-status async-status-line">
            <AsyncSpinner label={status} />
            <span>{status}</span>
          </p>
        ) : null}
        {error ? <p className="install-plugin-error">{error}</p> : null}
        {cancelled ? (
          <p className="install-plugin-cancelled" role="status">
            RackForge stopped before committing the package. No plugin was activated.
          </p>
        ) : null}
        {installed ? (
          <div className="install-plugin-complete">
            <div className="install-plugin-success" role="status">
              <strong>
                {installed.already_installed ? "Ready to open" : "Installation complete"}
              </strong>
              <span>{installed.plugin_id} v{installed.version}</span>
            </div>
            <p className="install-plugin-next-step">
              The package is installed but inactive. Open it in PLAY, configure it, or close this dialog.
            </p>
          </div>
        ) : null}
      </ModalDialog>
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
    </>
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
            {lockedFor > 0 ? (
              `Wait ${lockedFor}s`
            ) : (
              <AsyncActionLabel
                  active={submitting}
                  activeLabel={enrolling ? "Saving PIN…" : "Checking…"}
              >
                {enrolling ? "Set PIN" : "Unlock"}
              </AsyncActionLabel>
            )}
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
      : status === "idle"
        ? "No plugin installed"
        : status === "connecting"
          ? "Connecting"
          : "System offline";
  return (
    <div className={`connection-badge ${status}`}>
      {status === "connecting" ? (
        <AsyncSpinner label="Connecting to RackForge Core…" />
      ) : (
        <span className="status-dot" />
      )}
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
        {!isVstHost() ? <MasterPan value={snapshot?.master_pan ?? 0} /> : null}
        <MasterLevel value={snapshot?.master_level ?? 0} />
        {!isVstHost() ? <MasterOutputMeter /> : null}
      </div>
    </header>
  );
}

const METER_FLOOR_DB = -60;

function amplitudeToMeterDb(value: number) {
  if (!Number.isFinite(value) || value <= 0) return METER_FLOOR_DB;
  return Math.max(METER_FLOOR_DB, Math.min(3, 20 * Math.log10(value)));
}

function meterPercent(db: number) {
  return Math.max(0, Math.min(100, ((db - METER_FLOOR_DB) / -METER_FLOOR_DB) * 100));
}

function MasterOutputMeter() {
  const [levels, setLevels] = useState<[number, number]>([METER_FLOOR_DB, METER_FLOOR_DB]);
  const [holds, setHolds] = useState<[number, number]>([METER_FLOOR_DB, METER_FLOOR_DB]);
  const holdUntil = useRef<[number, number]>([0, 0]);

  useEffect(() => subscribeOutputMeter((meter: OutputMeterSnapshot) => {
    const incoming = [
      amplitudeToMeterDb(meter.left_peak),
      amplitudeToMeterDb(meter.right_peak),
    ] as const;
    const now = performance.now();
    setLevels((previous) => [
      Math.max(incoming[0], previous[0] - 2.4),
      Math.max(incoming[1], previous[1] - 2.4),
    ]);
    setHolds((previous) => previous.map((held, channel) => {
      const next = incoming[channel];
      if (next >= held) {
        holdUntil.current[channel] = now + 800;
        return next;
      }
      return now < holdUntil.current[channel]
        ? held
        : Math.max(next, held - 1.2);
    }) as [number, number]);
  }), []);

  const maximum = Math.max(...levels);
  const readable = maximum <= METER_FLOOR_DB
    ? "silent"
    : `${maximum.toFixed(1)} dBFS${maximum >= 0 ? ", clipping" : ""}`;
  return (
    <div className="master-output-meter" role="meter" aria-label={`Master output ${readable}`}>
      <span className="master-output-meter-label">Out</span>
      <span className="master-output-meter-bars" aria-hidden="true">
        {levels.map((level, channel) => (
          <i className="master-output-meter-track" key={channel}>
            <b style={{ height: `${meterPercent(level)}%` }} />
            <em style={{ bottom: `${meterPercent(holds[channel])}%` }} />
          </i>
        ))}
      </span>
      <span className="master-output-meter-channels" aria-hidden="true">L R</span>
    </div>
  );
}

function MasterLevel({ value }: { value: number }) {
  const [localValue, setLocalValue] = useState(value);
  const [dragging, setDragging] = useState(false);
  const displayedValue = dragging ? localValue : value;
  return (
    <MasterFader
      label="Volume"
      ariaLabel="Master volume"
      minimum={0}
      maximum={1000}
      value={displayedValue}
      output={`${Math.round(displayedValue / 10)}%`}
      onPointerStart={() => {
        setLocalValue(value);
        setDragging(true);
      }}
      onPointerEnd={() => setDragging(false)}
      onChange={(level) => {
        setLocalValue(level);
        dispatchCommand({ type: "set_master_level", level });
      }}
    />
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
    <MasterFader
      label="Pan"
      ariaLabel="Master pan"
      minimum={-1000}
      maximum={1000}
      step={10}
      value={displayedValue}
      output={display}
      centered
      onPointerStart={() => {
        setLocalValue(value);
        setDragging(true);
      }}
      onPointerEnd={() => setDragging(false)}
      onDoubleClick={() => {
        setLocalValue(0);
        dispatchCommand({ type: "set_master_pan", pan: 0 });
      }}
      onChange={(nextPan) => {
        const pan = Math.abs(nextPan) <= 70 ? 0 : nextPan;
        setLocalValue(pan);
        dispatchCommand({ type: "set_master_pan", pan });
      }}
    />
  );
}

function MasterFader({
  label,
  ariaLabel,
  minimum,
  maximum,
  step = 1,
  value,
  output,
  centered = false,
  onPointerStart,
  onPointerEnd,
  onDoubleClick,
  onChange,
}: {
  label: string;
  ariaLabel: string;
  minimum: number;
  maximum: number;
  step?: number;
  value: number;
  output: string;
  centered?: boolean;
  onPointerStart: () => void;
  onPointerEnd: () => void;
  onDoubleClick?: () => void;
  onChange: (value: number) => void;
}) {
  const position = ((value - minimum) / (maximum - minimum)) * 100;
  const start = centered ? Math.min(50, position) : 0;
  const span = centered ? Math.abs(position - 50) : position;
  const style = {
    "--fader-position": `${position}%`,
    "--fader-start": `${start}%`,
    "--fader-span": `${span}%`,
  } as CSSProperties;
  return (
    <label className={`compact-control${centered ? " is-centered pan-control" : ""}`}>
      <span className="compact-control-label">{label}</span>
      <span className="compact-fader" style={style}>
        <i className="compact-fader-rail" aria-hidden="true">
          <b className="compact-fader-fill" />
          {centered ? <b className="compact-fader-center" /> : null}
        </i>
        <input
          type="range"
          aria-label={ariaLabel}
          min={minimum}
          max={maximum}
          step={step}
          value={value}
          onPointerDown={onPointerStart}
          onPointerUp={onPointerEnd}
          onPointerCancel={onPointerEnd}
          onLostPointerCapture={onPointerEnd}
          onBlur={onPointerEnd}
          onDoubleClick={onDoubleClick}
          onChange={(event) => onChange(Number(event.target.value))}
        />
      </span>
      <output>{output}</output>
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

function PlayPage({
  snapshot,
  overlay,
  onOverlayChange,
  preferredInstanceId,
}: {
  snapshot: SessionSnapshot | null;
  overlay: "plugins" | "presets" | null;
  onOverlayChange: (overlay: "plugins" | "presets" | null) => void;
  preferredInstanceId?: string | null;
}) {
  const instances = snapshot?.instances ?? [];
  const active =
    instances.find((instance) => instance.instance_id === preferredInstanceId) ??
    instances.find(
      (instance) => instance.instance_id === snapshot?.active_instance_id,
    ) ?? instances[0];
  const playActivationStartedRef = useRef(false);
  useEffect(() => {
    if (
      playActivationStartedRef.current ||
      !snapshot ||
      (snapshot.active_mode === "play" && snapshot.live.active === undefined)
    ) return;
    playActivationStartedRef.current = true;
    const instanceId = active?.instance_id;
    void (async () => {
      try {
        // Rack preview cleanup is dispatched while this route mounts. Awaiting
        // the PLAY transition here makes it the final owner of the audio path,
        // then re-applies the already selected standalone instrument state.
        const applied = await dispatchCommandAwait({
          type: "set_active_mode",
          mode: "play",
        });
        // Leaving LIVE hands the voice back: the host puts the instrument
        // PLAY was holding before a Rack borrowed it. This route read
        // `instanceId` while LIVE still owned the voice, so re-asserting it
        // here is how the Rack's instrument followed the player home. If the
        // host already chose, that choice is the newer truth.
        const hostChose = applied.events.some(
          (event) =>
            typeof event === "object"
            && event !== null
            && (event as { event?: { type?: string } }).event?.type
              === "active_instance_changed",
        );
        if (hostChose || !instanceId) return;
        await dispatchCommandAwait({ type: "select_plugin", instance_id: instanceId });
        // Deliberately not re-selecting the program. `soundId` was read from
        // the session, so re-applying it can only ever restate what the host
        // already holds — and selecting a program loads its preset, which
        // overwrites every parameter the player has touched since. Editing a
        // control, going to the on-screen keyboard and coming back reset the
        // instrument to the preset. Neither `set_active_mode` nor
        // `select_plugin` disturbs the selection, so nothing here needs it.
      } catch {
        // The gateway publishes command failures through the shared error
        // banner. A later explicit program selection remains a safe retry.
      }
    })();
  }, [active, snapshot]);
  const pluginPickerOpen = overlay === "plugins";
  const presetsOpen = overlay === "presets";
  const [surfaceInfo, setSurfaceInfo] = useState<{
    instanceId: string;
    label: string;
    value: string;
  } | null>(null);
  const pluginCatalog = usePluginCatalog();
  const { plugins: installedPlugins } = pluginCatalog;
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
      ) : pluginCatalog.status === "idle" || pluginCatalog.status === "loading" ? (
        <RfLoader
          className="plugin-play-loader"
          label="Loading PLAY instruments"
          detail="Waiting for the plugin catalog and audio runtime…"
          size="large"
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
          programDraft={snapshot?.program_draft}
          onClose={() => onOverlayChange(null)}
        />
      )}
      {presetsOpen && active && (
        <PresetModal
          key={active.instance_id}
          instance={active}
          onClose={() => onOverlayChange(null)}
        />
      )}
    </section>
  );
}

function PluginPickerModal({
  active,
  instances,
  plugins,
  programDraft,
  onClose,
}: {
  active: PluginInstance | undefined;
  instances: PluginInstance[];
  plugins: PluginWebDescriptor[];
  programDraft: SessionSnapshot["program_draft"];
  onClose: () => void;
}) {
  const { status: catalogStatus, error: catalogError, runtime } = usePluginCatalog();
  const [activatingId, setActivatingId] = useState<string | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const [pendingPlugin, setPendingPlugin] = useState<PluginWebDescriptor | null>(null);
  const [pendingActivation, setPendingActivation] = useState<PluginWebDescriptor | null>(null);
  const activePluginId = active?.plugin_id;
  const orderedPlugins = [
    ...plugins.filter((plugin) => plugin.plugin_id === activePluginId),
    ...plugins.filter((plugin) => plugin.plugin_id !== activePluginId),
  ];
  const activate = async (
    plugin: PluginWebDescriptor,
    { discardDraft = false }: { discardDraft?: boolean } = {},
  ) => {
    const instance = instances.find(
      (candidate) => candidate.plugin_id === plugin.plugin_id,
    );
    const request = {
      target: {
        pluginId: plugin.plugin_id,
        pluginName: plugin.plugin_name,
        instanceId: instance?.instance_id,
      },
      activeInstanceId: active?.instance_id,
      programDraft: programDraft
        ? { draftId: programDraft.draft_id, dirty: programDraft.dirty }
        : undefined,
      discardDraft,
    };
    setActivationError(null);
    const preflight = preflightPlayPluginSelection(request);
    if (preflight.status === "already_active") {
      onClose();
      return;
    }
    if (preflight.status === "confirmation_required") {
      setPendingPlugin(plugin);
      return;
    }
    setPendingPlugin(null);
    setActivatingId(plugin.plugin_id);
    const finishOperation = beginPluginOperation(
      plugin.plugin_id,
      "open",
      `Opening ${plugin.plugin_name} in PLAY…`,
    );
    try {
      await commitPlayPluginSelection(request, {
        dispatch: dispatchCommandAwait,
        activate: (pluginId) =>
          hostJson(`/api/v1/plugins/${encodeURIComponent(pluginId)}/activate`, {
            method: "POST",
          }),
        synchronize: synchronizePluginEnvironment,
      });
      onClose();
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not activate the plugin.",
      );
    } finally {
      finishOperation();
      setActivatingId(null);
    }
  };
  const requestActivation = (plugin: PluginWebDescriptor) => {
    if (!plugin.active) {
      setActivationError(null);
      setPendingActivation(plugin);
      return;
    }
    void activate(plugin);
  };
  return (
    <>
      <ModalDialog
        eyebrow="PLAY · Instruments"
        title="Select plugin"
        onClose={onClose}
        dismissible={activatingId === null && pendingActivation === null}
        closeLabel="Close plugin selector"
        className="plugin-picker-modal"
      >
        <div className="preset-modal-toolbar">
          <p>Choose the instrument you want to play. The active plugin stays first.</p>
        </div>
        {pendingPlugin && programDraft && (
          <section className="plugin-switch-confirm" role="alert">
            <div>
              <strong>
                {programDraft.dirty
                  ? "Discard unsaved program changes?"
                  : "Close the current program editor?"}
              </strong>
              <p>
                RackForge must close the active edit session before switching to {" "}
                {pendingPlugin.plugin_name}.
              </p>
            </div>
            <div className="plugin-switch-confirm-actions">
              <button type="button" onClick={() => setPendingPlugin(null)}>
                Keep editing
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => void activate(pendingPlugin, { discardDraft: true })}
              >
                <AsyncActionLabel
                  active={activatingId === pendingPlugin.plugin_id}
                  activeLabel="Switching…"
                >
                  Discard and switch
                </AsyncActionLabel>
              </button>
            </div>
          </section>
        )}
        {activationError ? (
          <AsyncNotice tone="error" title="Could not open the plugin">
            {activationError}
          </AsyncNotice>
        ) : null}
        <AsyncStateBoundary
          className="plugin-picker-boundary"
          status={catalogStatus}
          hasContent={plugins.length > 0}
          loadingLabel="Loading plugins"
          loadingDetail="Discovering installed instruments and checking their runtimes…"
          errorTitle="Plugin library unavailable"
          errorDetail={catalogError ?? "RackForge could not load the plugin catalog."}
          onRetry={() => void invalidatePluginCatalog()}
        >
          <div className="play-plugin-selector modal-list" role="list" aria-label="Playable plugins">
            {orderedPlugins.map((plugin, index) => {
              const selected = plugin.plugin_id === activePluginId;
              const activating = activatingId === plugin.plugin_id;
              return (
                <button
                  className={`plugin-picker-card${selected ? " active" : ""}${!plugin.active ? " inactive" : ""}${plugin.branding ? " branded" : ""}`}
                  disabled={activatingId !== null}
                  key={plugin.plugin_id}
                  onClick={() => requestActivation(plugin)}
                  aria-disabled={!plugin.active}
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
                    <PluginRuntimeStatus status={runtime[plugin.plugin_id]} />
                  </span>
                  <span className="play-plugin-status">
                    {activating ? (
                      <AsyncActionLabel active activeLabel="Loading…">SELECT</AsyncActionLabel>
                    ) : (
                      <>{selected ? "PLAYING" : plugin.active ? "SELECT" : "INACTIVE"}<i aria-hidden="true">→</i></>
                    )}
                  </span>
                </button>
              );
            })}
            {orderedPlugins.length === 0 ? (
              <PluginSurfaceState
                title="No plugins installed"
                detail="Install an .rfplugin package from the Plugins section."
              />
            ) : null}
          </div>
        </AsyncStateBoundary>
      </ModalDialog>
      {pendingActivation ? (
        <ModalDialog
          eyebrow="Plugin activation"
          title={`Activate ${pendingActivation.plugin_name}?`}
          onClose={() => setPendingActivation(null)}
          dismissible={activatingId === null}
          closeLabel="Cancel plugin activation"
          className="plugin-activation-dialog"
          actions={
            <>
              <RfButton
                variant="secondary"
                onClick={() => setPendingActivation(null)}
                disabled={activatingId !== null}
              >
                Cancel
              </RfButton>
              <RfButton
                variant="primary"
                onClick={() => {
                  const plugin = pendingActivation;
                  setPendingActivation(null);
                  void activate(plugin);
                }}
                disabled={activatingId !== null}
              >
                <AsyncActionLabel
                  active={activatingId === pendingActivation.plugin_id}
                  activeLabel="Activating…"
                >
                  Activate plugin
                </AsyncActionLabel>
              </RfButton>
            </>
          }
        >
          <div className="plugin-activation-copy">
            <p>This plugin is installed but inactive.</p>
            <p>RackForge must activate it before it can be used in PLAY.</p>
          </div>
        </ModalDialog>
      ) : null}
    </>
  );
}

function formatPluginVersion(version: string | undefined) {
  if (!version) return "";
  return ` v${version.replace(/^[vV]/, "")}`;
}

function formatFileSize(bytes: number) {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} bytes`;
}

function PresetModal({
  instance,
  onClose,
}: {
  instance: PluginInstance;
  onClose: () => void;
}) {
  const [presets, setPresets] = useState<HostPresetSummary[]>([]);
  const [loadingPresets, setLoadingPresets] = useState(true);
  const [name, setName] = useState("");
  const [creating, setCreating] = useState(false);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameName, setRenameName] = useState("");
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const importInputRef = useRef<HTMLInputElement | null>(null);
  const [importCandidate, setImportCandidate] = useState<{
    fileName: string;
    file: RfPresetFile;
    preview: RfPresetImportPreview;
  } | null>(null);
  const [busyAction, setBusyAction] = useState<{
    kind: "load" | "save" | "rename" | "delete" | "export" | "inspect" | "import";
    presetId?: string;
  } | null>(null);
  const busy = busyAction !== null;
  const [message, setMessage] = useState<string | null>(null);
  const refresh = useCallback(() => {
    setLoadingPresets(true);
    return requestPluginPresets(instance.plugin_id)
      .then(setPresets)
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setLoadingPresets(false));
  }, [instance.plugin_id]);
  useEffect(() => {
    let cancelled = false;
    requestPluginPresets(instance.plugin_id)
      .then((nextPresets) => {
        if (!cancelled) setPresets(nextPresets);
      })
      .catch((error: Error) => {
        if (!cancelled) setMessage(error.message);
      })
      .finally(() => {
        if (!cancelled) setLoadingPresets(false);
      });
    return () => {
      cancelled = true;
    };
  }, [instance.plugin_id]);
  const load = (preset: HostPresetSummary) => {
    setBusyAction({ kind: "load", presetId: preset.id });
    setMessage(null);
    loadPluginPreset(instance.instance_id, preset.id)
      .then(onClose)
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const save = (event: FormEvent) => {
    event.preventDefault();
    if (!name.trim()) return;
    setBusyAction({ kind: "save" });
    setMessage(null);
    savePluginPreset(instance.instance_id, name.trim())
      .then((preset) => {
        setName("");
        setCreating(false);
        setMessage(`Saved ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const rename = (event: FormEvent, preset: HostPresetSummary) => {
    event.preventDefault();
    if (!renameName.trim()) return;
    setBusyAction({ kind: "rename", presetId: preset.id });
    setMessage(null);
    renamePluginPreset(instance.plugin_id, preset.id, renameName.trim())
      .then((renamed) => {
        setRenamingId(null);
        setRenameName("");
        setMessage(`Renamed to ${renamed.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const remove = (preset: HostPresetSummary) => {
    setBusyAction({ kind: "delete", presetId: preset.id });
    setMessage(null);
    deletePluginPreset(instance.plugin_id, preset.id)
      .then(() => {
        setDeletingId(null);
        setMessage(`Deleted ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const inspectImportText = async (fileName: string, text: string) => {
    setBusyAction({ kind: "inspect" });
    setMessage(null);
    try {
      if (!fileName.toLowerCase().endsWith(".rfpreset")) {
        throw new Error("Choose a .rfpreset file.");
      }
      if (!text || new TextEncoder().encode(text).byteLength > 2 * 1024 * 1024) {
        throw new Error("The preset file is empty or larger than 2 MiB.");
      }
      const file = JSON.parse(text) as RfPresetFile;
      const preview = await inspectPluginPreset(instance.plugin_id, file);
      setImportCandidate({ fileName, file, preview });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Could not validate the preset file.");
    } finally {
      setBusyAction(null);
    }
  };
  const chooseImport = () => {
    if (isNativeHost() || isDesktopHost()) {
      setBusyAction({ kind: "inspect" });
      setMessage(null);
      readNativeTextFile({ extensions: ["rfpreset"], maximum_bytes: 2 * 1024 * 1024 })
        .then(({ file_name, text }) => inspectImportText(file_name, text))
        .catch((error: Error) => setMessage(error.message))
        .finally(() => setBusyAction((current) => current?.kind === "inspect" ? null : current));
      return;
    }
    importInputRef.current?.click();
  };
  const exportPresetFile = (preset: HostPresetSummary) => {
    setBusyAction({ kind: "export", presetId: preset.id });
    setMessage(null);
    exportPluginPreset(instance.plugin_id, preset.id)
      .then(({ file_name, file }) => savePortableTextFile({
        file_name,
        mime_type: "application/vnd.rackforge.preset+json",
        text: `${JSON.stringify(file, null, 2)}\n`,
      }))
      .then(() => setMessage(`Exported ${preset.name}.`))
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  const commitImport = (policy: PresetImportConflictPolicy) => {
    if (!importCandidate) return;
    setBusyAction({ kind: "import" });
    setMessage(null);
    importPluginPreset(instance.plugin_id, importCandidate.file, policy)
      .then((preset) => {
        setImportCandidate(null);
        setMessage(`Imported ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusyAction(null));
  };
  return (
    <ModalDialog
      eyebrow={`${instance.plugin_name} · Complete states`}
      title="Presets"
      onClose={onClose}
      dismissible={!busy}
      closeLabel="Close presets"
    >
      {importCandidate ? (
        <section className="preset-import-stage" aria-label="Portable preset preview">
          <header className="preset-import-stage-header">
            <div>
              <span>Portable preset · Preview</span>
              <h3>Import {importCandidate.preview.preset.name}?</h3>
            </div>
            <button
              className="preset-modal-close modal-dialog-close"
              type="button"
              aria-label="Cancel preset import"
              disabled={busy}
              onClick={() => setImportCandidate(null)}
            >
              <X aria-hidden="true" />
            </button>
          </header>
          <div className="preset-import-summary">
            <dl>
              <div><dt>Plugin</dt><dd>{importCandidate.preview.preset.plugin_id}</dd></div>
              <div><dt>Plugin version</dt><dd>v{importCandidate.preview.preset.plugin_version}</dd></div>
              <div><dt>State format</dt><dd>v{importCandidate.preview.preset.state_version}</dd></div>
              <div><dt>State size</dt><dd>{formatFileSize(importCandidate.preview.byte_length)}</dd></div>
              <div><dt>File</dt><dd>{importCandidate.fileName}</dd></div>
            </dl>
            {importCandidate.preview.conflict ? (
              <p className="preset-import-conflict">A local preset already uses this name or identity.</p>
            ) : null}
            {importCandidate.preview.warnings.map((warning) => (
              <p className="preset-import-warning" key={warning}>{warning}</p>
            ))}
          </div>
          <footer className="preset-import-actions">
            <button className="secondary-button" disabled={busy} onClick={() => setImportCandidate(null)}>
              Cancel
            </button>
            {importCandidate.preview.conflict ? (
              <button className="secondary-button" disabled={busy || !importCandidate.preview.compatible} onClick={() => commitImport("keep_both")}>
                Import as copy
              </button>
            ) : null}
            {importCandidate.preview.conflict !== "ambiguous" ? (
              <button
                className="primary-button"
                disabled={busy || !importCandidate.preview.compatible}
                onClick={() => commitImport(importCandidate.preview.conflict ? "replace" : "reject")}
              >
                <AsyncActionLabel active={busyAction?.kind === "import"} activeLabel="Importing…">
                  {importCandidate.preview.conflict ? "Replace existing" : "Import preset"}
                </AsyncActionLabel>
              </button>
            ) : null}
          </footer>
        </section>
      ) : (
        <>
        <div className="preset-modal-toolbar">
          <p>Load a captured state or save the instrument exactly as it sounds now.</p>
          <div className="preset-toolbar-actions">
            <button className="preset-import-button" disabled={busy} onClick={chooseImport}>
              <FileUp aria-hidden="true" />
              <AsyncActionLabel active={busyAction?.kind === "inspect"} activeLabel="Validating…">
                Import .rfpreset
              </AsyncActionLabel>
            </button>
            <button className="preset-create-button" disabled={busy} onClick={() => setCreating((value) => !value)}>
              <span aria-hidden="true">＋</span> New preset
            </button>
          </div>
          <input
            ref={importInputRef}
            className="visually-hidden"
            type="file"
            accept=".rfpreset,application/vnd.rackforge.preset+json,application/json"
            onChange={(event) => {
              const selected = event.currentTarget.files?.[0];
              event.currentTarget.value = "";
              if (!selected) return;
              void selected.text()
                .then((text) => inspectImportText(selected.name, text))
                .catch((error: Error) => setMessage(error.message));
            }}
          />
        </div>
        {creating && (
          <form className="preset-create-form" onSubmit={save}>
            <label>
              <span>Preset name</span>
              <input autoFocus maxLength={96} value={name} onChange={(event) => setName(event.target.value)} placeholder="Warm Strings" />
            </label>
            <button disabled={busy || !name.trim()} type="submit">
              <AsyncActionLabel active={busyAction?.kind === "save"} activeLabel="Saving…">
                Capture state
              </AsyncActionLabel>
            </button>
          </form>
        )}
        {message && <p className="preset-message">{message}</p>}
        <div className="preset-list modal-list">
          {loadingPresets && presets.length === 0 ? (
            <RfLoader
              label="Loading presets"
              detail="Reading complete instrument states…"
              size="compact"
            />
          ) : presets.length === 0 ? (
            <div className="preset-empty"><span>00</span><strong>No presets yet</strong><small>Capture the current plugin state to create the first one.</small></div>
          ) : presets.map((preset) => (
            <article className="preset-row" key={preset.id}>
              {renamingId === preset.id ? (
                <form className="preset-rename-form" onSubmit={(event) => rename(event, preset)}>
                  <input autoFocus maxLength={96} value={renameName} onChange={(event) => setRenameName(event.target.value)} />
                  <button disabled={busy || !renameName.trim()} type="submit">
                    <AsyncActionLabel
                      active={busyAction?.kind === "rename" && busyAction.presetId === preset.id}
                      activeLabel="Renaming…"
                    >
                      Save
                    </AsyncActionLabel>
                  </button>
                  <button type="button" onClick={() => setRenamingId(null)}>Cancel</button>
                </form>
              ) : (
                <>
                  <button className="preset-load-target" disabled={busy} onClick={() => load(preset)}>
                    <span><strong>{preset.name}</strong><small>State v{preset.state_version} · Plugin {preset.plugin_version}</small></span>
                    <i>
                      <AsyncActionLabel
                        active={busyAction?.kind === "load" && busyAction.presetId === preset.id}
                        activeLabel="Loading…"
                      >
                        LOAD →
                      </AsyncActionLabel>
                    </i>
                  </button>
                  <div className="preset-row-actions">
                    <button disabled={busy} onClick={() => exportPresetFile(preset)}>
                      <AsyncActionLabel
                        active={busyAction?.kind === "export" && busyAction.presetId === preset.id}
                        activeLabel="Exporting…"
                      >
                        Export
                      </AsyncActionLabel>
                    </button>
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
                  <button className="danger" disabled={busy} onClick={() => remove(preset)}>
                    <AsyncActionLabel
                      active={busyAction?.kind === "delete" && busyAction.presetId === preset.id}
                      activeLabel="Deleting…"
                    >
                      Delete
                    </AsyncActionLabel>
                  </button>
                </div>
              )}
            </article>
          ))}
        </div>
        </>
      )}
    </ModalDialog>
  );
}

interface PluginRemovalOptions {
  delete_presets: boolean;
  delete_plugin_data: boolean;
}

interface PluginRemovalResult {
  cleanup_pending?: boolean;
  presets_deleted?: boolean;
  plugin_data_deleted?: boolean;
  user_data_cleanup_warning?: string | null;
}

function pluginRemovalSummary(result: PluginRemovalResult) {
  const parts = ["Plugin package removed."];
  parts.push(result.presets_deleted ? "Presets deleted." : "Presets preserved.");
  parts.push(
    result.plugin_data_deleted
      ? "Imported resources and private plugin data deleted."
      : "Imported resources and private plugin data preserved.",
  );
  if (result.cleanup_pending) {
    parts.push("Locked package files will be cleaned after RackForge closes.");
  }
  if (result.user_data_cleanup_warning) {
    parts.push(`Some selected user data could not be removed: ${result.user_data_cleanup_warning}`);
  }
  return parts.join(" ");
}

function PluginRemovalDialog({
  pluginName,
  active,
  removing,
  error,
  onClose,
  onConfirm,
}: {
  pluginName: string;
  active: boolean;
  removing: boolean;
  error?: string | null;
  onClose: () => void;
  onConfirm: (options: PluginRemovalOptions) => Promise<void>;
}) {
  const [deletePresets, setDeletePresets] = useState(false);
  const [deletePluginData, setDeletePluginData] = useState(false);

  return (
    <ModalDialog
      eyebrow="Installed plugin"
      title={`Remove ${pluginName}?`}
      role="alertdialog"
      onClose={onClose}
      dismissible={!removing}
      closeLabel="Close plugin removal"
      backdropClassName="plugin-remove-backdrop"
      className="plugin-remove-dialog"
      actions={
        <>
          <button className="secondary-button" disabled={removing} onClick={onClose}>Cancel</button>
          <button
            className="danger-button"
            disabled={removing}
            onClick={() => void onConfirm({
              delete_presets: deletePresets,
              delete_plugin_data: deletePluginData,
            })}
          >
            <AsyncActionLabel active={removing} activeLabel="Removing plugin…">
              Remove plugin
            </AsyncActionLabel>
          </button>
        </>
      }
    >
        <div className="plugin-remove-copy">
          <p>RackForge will always remove every installed version of this plugin package.</p>
          <fieldset className="plugin-remove-options" disabled={removing}>
            <legend>Also remove user data</legend>
            <label>
              <input
                type="checkbox"
                checked={deletePresets}
                onChange={(event) => setDeletePresets(event.target.checked)}
              />
              <span>
                <strong>Delete RackForge presets</strong>
                <small>Named presets are removed. State still referenced by racks or songs remains safe.</small>
              </span>
            </label>
            <label>
              <input
                type="checkbox"
                checked={deletePluginData}
                onChange={(event) => setDeletePluginData(event.target.checked)}
              />
              <span>
                <strong>Delete imported resources and plugin data</strong>
                <small>Removes extracted firmware or ROMs, custom programs and private caches.</small>
              </span>
            </label>
          </fieldset>
          <p className="plugin-remove-note">The archive selected for an import is temporary and is already discarded after a successful import.</p>
          {active ? <p>It is currently active, so sound will stop briefly while RackForge selects another available instrument.</p> : null}
          {error ? <p className="form-error">{error}</p> : null}
        </div>
    </ModalDialog>
  );
}

type ControllerSettingSummary = {
  id: string;
  name: string;
  kind: string;
  default: string;
  page: string | null;
  value: string;
};

type ControllerSummary = {
  id: string;
  name: string;
  version: string;
  enabled: boolean;
  trust: string;
  runtime: string;
  devices: number;
  settings: ControllerSettingSummary[];
};

function ControllerPage() {
  const { controllerId } = useParams();
  const id = decodeURIComponent(controllerId ?? "");
  const [controller, setController] = useState<ControllerSummary | null>(null);
  const [status, setStatus] = useState<string>("");
  const saveTimer = useRef<number | null>(null);
  useEffect(() => {
    let cancelled = false;
    hostJson<{ controllers: ControllerSummary[] }>("/api/v1/controllers")
      .then((response) => {
        if (cancelled) return;
        setController(
          response.controllers.find((candidate) => candidate.id === id) ?? null,
        );
      })
      .catch(() => setStatus("Could not read the controller."));
    return () => {
      cancelled = true;
    };
  }, [id]);

  const applyValue = (settingId: string, value: string) => {
    setController((current) =>
      current
        ? {
            ...current,
            settings: current.settings.map((setting) =>
              setting.id === settingId ? { ...setting, value } : setting,
            ),
          }
        : current,
    );
    // Color pickers stream values while dragging; the hardware repaint is
    // ~44 SysEx messages, so settle for 200 ms of quiet before saving.
    if (saveTimer.current !== null) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      hostJson(`/api/v1/controllers/${encodeURIComponent(id)}/settings`, {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ values: { [settingId]: value } }),
      })
        .then(() => setStatus("Saved · the hardware follows within a second"))
        .catch((error) =>
          setStatus(error instanceof Error ? error.message : "Could not save."),
        );
    }, 200);
  };

  if (!controller) {
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title={status || "Loading controller…"}
          detail={status ? "The controller may have been removed." : id}
        />
      </section>
    );
  }
  return (
    <section className="controller-config">
      <PageHeading
        eyebrow="Controller"
        title={controller.name}
        detail={`Version ${controller.version} · ${controller.trust} · ${controller.runtime} · settings apply live`}
      />
      <div className="controller-settings">
        {controller.settings.length === 0 && (
          <p className="controller-no-settings">
            This controller does not expose settings yet.
          </p>
        )}
        {controller.settings.map((setting) => (
          <label className="controller-setting" key={setting.id}>
            <span>
              <strong>{setting.name}</strong>
              {setting.page ? <small> · {setting.page}</small> : null}
            </span>
            {setting.kind === "color" ? (
              <input
                type="color"
                value={setting.value}
                onChange={(event) => applyValue(setting.id, event.target.value)}
              />
            ) : (
              <input
                type="text"
                value={setting.value}
                onChange={(event) => applyValue(setting.id, event.target.value)}
              />
            )}
          </label>
        ))}
      </div>
      {status ? <p className="controller-status">{status}</p> : null}
    </section>
  );
}

function PluginsPage({
  snapshot,
  onInstall,
  showControllers = true,
}: {
  snapshot: SessionSnapshot | null;
  onInstall: () => void;
  showControllers?: boolean;
}) {
  const location = useLocation();
  const navigate = useNavigate();
  const pluginCatalog = usePluginCatalog();
  const { plugins: installed } = pluginCatalog;
  const [controllers, setControllers] = useState<ControllerSummary[]>([]);
  const [controllersStatus, setControllersStatus] = useState<"loading" | "ready" | "error">(
    showControllers ? "loading" : "ready",
  );
  const [controllerRefreshRevision, setControllerRefreshRevision] = useState(0);
  useEffect(() => {
    if (!showControllers) return;
    let cancelled = false;
    hostJson<{ controllers: ControllerSummary[] }>("/api/v1/controllers")
      .then((response) => {
        if (!cancelled) {
          setControllers(response.controllers ?? []);
          setControllersStatus("ready");
        }
      })
      .catch(() => {
        if (!cancelled) setControllersStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [controllerRefreshRevision, showControllers]);
  const [pendingRemoval, setPendingRemoval] = useState<PluginWebDescriptor | null>(null);
  const [removing, setRemoving] = useState(false);
  const [changingPluginId, setChangingPluginId] = useState<string | null>(null);
  const [activationError, setActivationError] = useState<string | null>(null);
  const [removalError, setRemovalError] = useState<string | null>(null);
  const [removalMessage, setRemovalMessage] = useState<string | null>(() => {
    const state = location.state as { pluginRemovalMessage?: unknown } | null;
    return typeof state?.pluginRemovalMessage === "string"
      ? state.pluginRemovalMessage
      : null;
  });

  const removePlugin = async (options: PluginRemovalOptions) => {
    if (!pendingRemoval) return;
    const finishOperation = beginPluginOperation(
      pendingRemoval.plugin_id,
      "remove",
      `Removing ${pendingRemoval.plugin_name}…`,
    );
    setRemoving(true);
    setRemovalError(null);
    setRemovalMessage(null);
    try {
      const result = await hostJson<PluginRemovalResult>(
        `/api/v1/plugins/${encodeURIComponent(pendingRemoval.plugin_id)}`,
        {
          method: "DELETE",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(options),
        },
      );
      setPendingRemoval(null);
      await synchronizePluginEnvironment();
      setRemovalMessage(pluginRemovalSummary(result));
    } catch (error) {
      setRemovalError(
        error instanceof Error ? error.message : "Could not remove the plugin.",
      );
    } finally {
      finishOperation();
      setRemoving(false);
    }
  };

  const running = snapshot?.instances ?? [];
  const requestRemoval = (plugin: PluginWebDescriptor) => {
    setRemovalError(null);
    setPendingRemoval(plugin);
  };
  const changeActivation = async (plugin: PluginWebDescriptor) => {
    if (!plugin.managed && plugin.active) return;
    setChangingPluginId(plugin.plugin_id);
    setActivationError(null);
    setRemovalMessage(null);
    try {
      await setInstalledPluginActive(plugin.plugin_id, !plugin.active);
      setRemovalMessage(
        plugin.active
          ? `${plugin.plugin_name} is now inactive.`
          : `${plugin.plugin_name} is active and ready to use.`,
      );
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not change plugin activation.",
      );
    } finally {
      setChangingPluginId(null);
    }
  };
  const openInPlay = async (plugin: PluginWebDescriptor) => {
    if (!plugin.active) return;
    const finishOperation = beginPluginOperation(
      plugin.plugin_id,
      "open",
      `Opening ${plugin.plugin_name} in PLAY…`,
    );
    setChangingPluginId(plugin.plugin_id);
    setActivationError(null);
    try {
      const instance = running.find(
        (candidate) => candidate.plugin_id === plugin.plugin_id,
      );
      const request = {
        target: {
          pluginId: plugin.plugin_id,
          pluginName: plugin.plugin_name,
          instanceId: instance?.instance_id,
        },
        activeInstanceId: snapshot?.active_instance_id,
      };
      if (preflightPlayPluginSelection(request).status !== "already_active") {
        await commitPlayPluginSelection(request, {
          dispatch: dispatchCommandAwait,
          activate: (pluginId) =>
            hostJson(`/api/v1/plugins/${encodeURIComponent(pluginId)}/activate`, {
              method: "POST",
            }),
          synchronize: synchronizePluginEnvironment,
        });
      }
      navigate("/play");
    } catch (error) {
      setActivationError(
        error instanceof Error ? error.message : "Could not open the plugin in PLAY.",
      );
    } finally {
      finishOperation();
      setChangingPluginId(null);
    }
  };

  return (
    <>
      <div className="plugin-manager-heading">
        <PageHeading
          eyebrow="Plugin library"
          title="Plugin Manager"
          detail={showControllers
            ? "Install, configure and remove RackForge plugins: instruments and controllers. Musical controls remain in Play."
            : "Choose and manage the instruments available to this RackForge VST3 instance."}
        />
        <RfButton variant="primary" className="plugin-install-button" onClick={onInstall}>
          <Download aria-hidden="true" />
          Install plugin
        </RfButton>
      </div>
      <div className="plugin-section-heading">
        <span className="card-kicker">Audio plugins</span>
        <small>Installation and runtime activation are managed separately</small>
      </div>
      <div className="rf-floating-notice-stack">
        {removalMessage ? (
          <AsyncNotice
            tone="success"
            title="Plugin library updated"
            onDismiss={() => setRemovalMessage(null)}
          >
            {removalMessage}
          </AsyncNotice>
        ) : null}
        {activationError ? (
          <AsyncNotice
            tone="error"
            title="Plugin operation failed"
            onDismiss={() => setActivationError(null)}
          >
            {activationError}
          </AsyncNotice>
        ) : null}
        {controllersStatus === "error" ? (
          <AsyncNotice tone="error" title="Controller packages unavailable">
            Could not read the installed hardware profiles.
            <RfButton
              size="compact"
              haptic="none"
              onClick={() => {
                setControllersStatus("loading");
                setControllerRefreshRevision((current) => current + 1);
              }}
            >
              Retry
            </RfButton>
          </AsyncNotice>
        ) : null}
      </div>
      <AsyncStateBoundary
        className="plugin-manager-boundary"
        status={pluginCatalog.status}
        hasContent={installed.length > 0}
        loadingLabel="Loading Plugin Manager"
        loadingDetail="Discovering packages and checking the audio runtime…"
        errorTitle="Plugin library unavailable"
        errorDetail={pluginCatalog.error ?? "RackForge could not load installed plugins."}
        onRetry={() => void invalidatePluginCatalog()}
        loaderSize="large"
      >
        <div className="plugin-grid expanded plugin-manager-grid">
        {installed.map((plugin, index) => {
          const instance = running.find((candidate) => candidate.plugin_id === plugin.plugin_id);
          const busy = changingPluginId === plugin.plugin_id;
          const configAvailable = plugin.surfaces.some((surface) => surface.kind === "config");
          const kind = pluginKindPresentation(plugin.kind);
          return (
            <article
              className={`plugin-card installed-plugin-card plugin-manager-card${plugin.active ? "" : " inactive"}`}
              key={plugin.plugin_id}
            >
              <div className={`plugin-tile tile-${index % 4}${plugin.branding ? " branded" : ""}`}>
                <PluginIcon plugin={plugin} name={plugin.plugin_name} />
                {!plugin.branding && <i />}
              </div>
              <div className="plugin-manager-card-copy">
                <span className="card-kicker">
                  {plugin.active ? "Active" : "Inactive"}{" "}
                  <span className={`plugin-kind-tag ${kind.className}`}>{kind.label}</span>
                </span>
                <h3>{plugin.plugin_name}{formatPluginVersion(plugin.version)}</h3>
                <PluginRuntimeStatus status={pluginCatalog.runtime[plugin.plugin_id]} />
                <p>{plugin.surfaces.length === 0 ? "No Web interface" : "Web interface ready"}</p>
              </div>
              <div className="plugin-manager-card-actions" aria-label={`${plugin.plugin_name} actions`}>
                <RfButton
                  variant={plugin.active ? "secondary" : "primary"}
                  disabled={busy || (!plugin.managed && plugin.active)}
                  onClick={() => void changeActivation(plugin)}
                >
                  <AsyncActionLabel
                    active={busy}
                    activeLabel={plugin.active ? "Deactivating…" : "Activating…"}
                  >
                    {plugin.active
                      ? plugin.managed ? "Deactivate" : "Built-in active"
                      : "Activate"}
                  </AsyncActionLabel>
                </RfButton>
                <RfButton
                  variant="secondary"
                  disabled={!plugin.active || busy}
                  onClick={() => void openInPlay(plugin)}
                >
                  Go to PLAY
                </RfButton>
                <RfButton
                  variant="secondary"
                  disabled={!plugin.active || !configAvailable || !instance || busy}
                  onClick={() => navigate(`/plugins/${encodeURIComponent(instance!.instance_id)}`)}
                >
                  Config
                </RfButton>
                <RfButton
                  variant="danger"
                  className="plugin-manager-remove"
                  disabled={!plugin.managed || busy}
                  onClick={() => requestRemoval(plugin)}
                >
                  Remove
                </RfButton>
              </div>
            </article>
          );
        })}
      </div>
      {installed.length === 0 ? (
        <EmptyState title="No plugins installed" />
      ) : null}
        </AsyncStateBoundary>
      {controllersStatus === "loading" ? (
        <RfLoader
          className="plugin-controller-loader"
          label="Loading controllers"
          detail="Discovering installed hardware profiles…"
          size="compact"
        />
      ) : null}
      {controllers.length > 0 && (
        <>
          <div className="plugin-section-heading">
            <span className="card-kicker">Controllers</span>
            <small>Hardware surfaces installed as packages</small>
          </div>
          <div className="plugin-grid expanded">
            {controllers.map((controller) => (
              <NavLink
                className="plugin-card installed-plugin-card"
                key={controller.id}
                to={`/controllers/${encodeURIComponent(controller.id)}`}
              >
                <div className="plugin-tile controller-tile">
                  <span className="controller-tile-mark" aria-hidden="true">
                    <Sliders aria-hidden="true" />
                  </span>
                </div>
                <div>
                  <span className="card-kicker">
                    {controller.enabled ? "Active package" : "Disabled package"}{" "}
                    <span className="plugin-kind-tag controller">Controller</span>
                  </span>
                  <h3>{controller.name}</h3>
                  <p>
                    Version {controller.version} · {controller.trust}
                    {controller.devices > 0 ? ` · ${controller.devices} device profile(s)` : ""}
                    {` · ${controller.runtime}`}
                  </p>
                </div>
              </NavLink>
            ))}
          </div>
        </>
      )}
      {pendingRemoval ? (
        <PluginRemovalDialog
          pluginName={pendingRemoval.plugin_name}
          active={pendingRemoval.active}
          removing={removing}
          error={removalError}
          onClose={() => setPendingRemoval(null)}
          onConfirm={removePlugin}
        />
      ) : null}
    </>
  );
}

function PluginPage({
  snapshot,
  connection,
}: {
  snapshot: SessionSnapshot | null;
  connection: ConnectionStatus;
}) {
  const { instanceId } = useParams();
  const { plugins, status: catalogStatus, error: catalogError } = usePluginCatalog();
  const instance = snapshot?.instances.find(
    (item) => item.instance_id === decodeURIComponent(instanceId ?? ""),
  );
  if (
    !instance &&
    (connection === "connecting" || catalogStatus === "idle" || catalogStatus === "loading")
  ) {
    return (
      <section className="plugin-surface-shell direct-surface plugin-surface-loading">
        <RfLoader
          label="Opening plugin configuration"
          detail="Loading the plugin catalog and runtime instance…"
          size="large"
        />
      </section>
    );
  }
  if (!instance)
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin not found"
          detail="This plugin instance is no longer available in the current session."
        />
      </section>
    );
  if (catalogStatus === "idle" || catalogStatus === "loading") {
    return (
      <section className="plugin-surface-shell direct-surface plugin-surface-loading">
        <RfLoader
          label="Checking plugin activation"
          detail="RackForge is verifying the plugin before opening Config…"
          size="large"
        />
      </section>
    );
  }
  if (catalogStatus === "error") {
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin library unavailable"
          detail={catalogError ?? "RackForge could not verify the plugin catalog."}
        />
      </section>
    );
  }
  const descriptor = plugins.find((plugin) => plugin.plugin_id === instance.plugin_id);
  if (!descriptor?.active) {
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="Plugin inactive"
          detail="Activate this plugin from Plugin Manager before opening its configuration."
        />
      </section>
    );
  }
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

function PluginConfigSurface({ instance }: { instance: PluginInstance }) {
  const navigate = useNavigate();
  const { descriptor } = usePluginDescriptor(instance.plugin_id);
  const [showRemove, setShowRemove] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);

  const removePlugin = async (options: PluginRemovalOptions) => {
    if (!descriptor) return;
    const finishOperation = beginPluginOperation(
      descriptor.plugin_id,
      "remove",
      `Removing ${descriptor.plugin_name}…`,
    );
    setRemoving(true);
    setRemoveError(null);
    try {
      const result = await hostJson<PluginRemovalResult>(
        `/api/v1/plugins/${encodeURIComponent(descriptor.plugin_id)}`,
        {
          method: "DELETE",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(options),
        },
      );
      await synchronizePluginEnvironment();
      navigate("/plugins", {
        replace: true,
        state: { pluginRemovalMessage: pluginRemovalSummary(result) },
      });
    } catch (error) {
      setRemoveError(
        error instanceof Error ? error.message : "Could not remove the plugin.",
      );
    } finally {
      finishOperation();
      setRemoving(false);
    }
  };

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
        {descriptor?.managed ? (
          <button
            className="plugin-detail-remove-button"
            onClick={() => {
              setRemoveError(null);
              setShowRemove(true);
            }}
          >
            <Trash2 aria-hidden="true" />
            <span>Remove plugin</span>
          </button>
        ) : null}
      </div>
      <PluginFrame
        key={instance.instance_id}
        instance={instance}
        surface="config"
      />
      {showRemove && descriptor ? (
        <PluginRemovalDialog
          pluginName={descriptor.plugin_name}
          active
          removing={removing}
          error={removeError}
          onClose={() => setShowRemove(false)}
          onConfirm={removePlugin}
        />
      ) : null}
    </section>
  );
}

const ISOLATED_PARAMETER_DEBOUNCE_MS = 48;
const LIVE_PARAMETER_SYNC_MS = 100;

interface PendingIsolatedParameterWrite {
  value: number;
  requestIds: string[];
  timer: number;
}

export function PluginFrame({
  instance,
  surface,
  onSurfaceInfoChange,
  isolated = false,
  isolatedState,
  onIsolatedStateChange,
  onSelectSound,
  parameterLinkInstanceId,
}: {
  instance: PluginInstance;
  surface: PluginWebSurfaceKind;
  onSurfaceInfoChange?: (info: { label: string; value: string } | null) => void;
  isolated?: boolean;
  isolatedState?: PluginStateReference;
  onIsolatedStateChange?: (state: PluginStateReference) => void;
  onSelectSound?: (soundId: string) => Promise<unknown>;
  parameterLinkInstanceId?: string;
}) {
  const catalogDescriptor = usePluginDescriptor(instance.plugin_id);
  const descriptor = catalogDescriptor.descriptor;
  const descriptorStatus = catalogDescriptor.status === "error"
    ? "error"
    : catalogDescriptor.status === "ready"
      ? descriptor ? "ready" : "unavailable"
      : "loading";
  const selectedSurface = descriptor?.surfaces.find(
    (candidate) => candidate.kind === surface,
  );
  const surfaceIdentity = [
    instance.plugin_id,
    descriptor?.version ?? "loading",
    selectedSurface?.entry_url ?? surface,
  ].join(":");
  const [loadedFrameIdentity, setLoadedFrameIdentity] = useState<string | null>(null);
  const frameLoaded = loadedFrameIdentity === surfaceIdentity;
  const [frameDocumentGeneration, setFrameDocumentGeneration] = useState(0);
  // The splash's own lifecycle: the icon fill reaches the top, THEN the
  // whole overlay fades, THEN it unmounts. Removing it on iframe load was
  // an abrupt cut.
  const [completedSplashIdentity, setCompletedSplashIdentity] = useState<string | null>(null);
  const [hiddenSplashIdentity, setHiddenSplashIdentity] = useState<string | null>(null);
  const splashDone = completedSplashIdentity === surfaceIdentity;
  const splashGone = hiddenSplashIdentity === surfaceIdentity;
  const splashLitRef = useRef<HTMLImageElement | null>(null);
  const frameLoadedRef = useRef(false);
  const [resourceBusy, setResourceBusy] = useState<string | null>(null);
  const snapshot = useSelector((state: RootState) => state.rackforge.snapshot);
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const liveParameterValuesRef = useRef<Map<number, number>>(new Map());
  const pendingResourceRequestRef = useRef<string | null>(null);
  const isolatedStateRef = useRef<PluginStateReference | undefined>(isolatedState);
  const [isolatedContextState, setIsolatedContextState] = useState<
    PluginStateReference | undefined
  >(isolatedState);
  const isolatedStateInputKey = isolatedState
    ? [
        isolatedState.plugin_id,
        isolatedState.plugin_version,
        isolatedState.state_version,
        isolatedState.blob_sha256,
        isolatedState.selected_sound_id ?? "",
      ].join(":")
    : "";
  const [previousIsolatedStateInputKey, setPreviousIsolatedStateInputKey] =
    useState(isolatedStateInputKey);
  const onIsolatedStateChangeRef = useRef(onIsolatedStateChange);
  const isolatedMaterializeRef = useRef<Promise<PluginStateReference> | null>(null);
  const isolatedWriteChainRef = useRef<Promise<void>>(Promise.resolve());
  const isolatedWritesRef = useRef<Map<number, PendingIsolatedParameterWrite>>(new Map());
  const [resourceRequest, setResourceRequest] = useState<{
    requestId: string;
    resource: PluginResourceRequirement;
  } | null>(null);
  const [isolatedBootstrapError, setIsolatedBootstrapError] = useState<string | null>(null);

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

  if (previousIsolatedStateInputKey !== isolatedStateInputKey) {
    setPreviousIsolatedStateInputKey(isolatedStateInputKey);
    setIsolatedContextState(isolatedState);
  }

  useEffect(() => {
    isolatedStateRef.current = isolatedContextState;
  }, [isolatedContextState]);

  useEffect(() => {
    onIsolatedStateChangeRef.current = onIsolatedStateChange;
  }, [onIsolatedStateChange]);

  useEffect(() => {
    frameLoadedRef.current = frameLoaded;
  }, [frameLoaded]);

  const publishIsolatedState = useCallback((state: PluginStateReference) => {
    isolatedStateRef.current = state;
    // Keep the iframe context authoritative even when the surrounding Rack
    // draft has not rerendered yet. Program selection depends on this field.
    setIsolatedContextState(state);
    onIsolatedStateChangeRef.current?.(state);
  }, []);

  const ensureIsolatedState = useCallback((): Promise<PluginStateReference> => {
    if (isolatedStateRef.current) return Promise.resolve(isolatedStateRef.current);
    if (!isolatedMaterializeRef.current) {
      const pending = materializePluginState(instance.plugin_id)
        .then((state) => {
          publishIsolatedState(state);
          return state;
        })
        .finally(() => {
          if (isolatedMaterializeRef.current === pending) {
            isolatedMaterializeRef.current = null;
          }
        });
      isolatedMaterializeRef.current = pending;
    }
    return isolatedMaterializeRef.current;
  }, [instance.plugin_id, publishIsolatedState]);

  // An isolated Rack Slot is not the global PLAY instance. Build its initial
  // immutable state before publishing the iframe context; otherwise plugins
  // that require a selected program can reject the incomplete context and
  // remain on their static boot screen forever.
  useEffect(() => {
    if (!isolated || isolatedStateRef.current) return;
    let cancelled = false;
    setIsolatedBootstrapError(null);
    void ensureIsolatedState().catch((error: unknown) => {
      if (cancelled) return;
      setIsolatedBootstrapError(
        error instanceof Error
          ? error.message
          : "Could not initialize the Rack Slot instrument.",
      );
    });
    return () => {
      cancelled = true;
    };
  }, [ensureIsolatedState, isolated]);

  const loadParameterSchemaForLink = useCallback(async () => {
    if (isolated) {
      const state = await ensureIsolatedState();
      return requestPluginStateParameters(state);
    }
    return requestPluginParameters(instance.instance_id);
  }, [ensureIsolatedState, instance.instance_id, isolated]);

  const flushIsolatedParameterWrite = useCallback((
    parameterIndex: number,
    pending: PendingIsolatedParameterWrite,
  ) => {
    isolatedWritesRef.current.delete(parameterIndex);
    const run = async () => {
      for (let attempt = 0; attempt < 3; attempt += 1) {
        const base = await ensureIsolatedState();
        const result = await setPluginStateParameter(base, parameterIndex, pending.value);
        const current = isolatedStateRef.current;
        if (
          current &&
          current.blob_sha256 !== base.blob_sha256 &&
          current.blob_sha256 !== result.state.blob_sha256
        ) {
          continue;
        }
        publishIsolatedState(result.state);
        return result.value;
      }
      throw new Error("Rack Slot state kept changing while the parameter was edited.");
    };
    const operation = isolatedWriteChainRef.current.then(run, run);
    isolatedWriteChainRef.current = operation.then(() => undefined, () => undefined);
    operation
      .then((canonical) => {
        for (const requestId of pending.requestIds) {
          sendPluginResponse(requestId, true, undefined, { value: canonical });
        }
      })
      .catch((error: unknown) => {
        const message = error instanceof Error
          ? error.message
          : "Could not set Rack Slot parameter.";
        for (const requestId of pending.requestIds) {
          sendPluginResponse(requestId, false, message);
        }
      });
  }, [ensureIsolatedState, publishIsolatedState, sendPluginResponse]);

  const queueIsolatedParameterWrite = useCallback((
    requestId: string,
    parameterIndex: number,
    value: number,
  ) => {
    let pending = isolatedWritesRef.current.get(parameterIndex);
    if (pending) {
      window.clearTimeout(pending.timer);
      pending.value = value;
      pending.requestIds.push(requestId);
    } else {
      pending = { value, requestIds: [requestId], timer: 0 };
      isolatedWritesRef.current.set(parameterIndex, pending);
    }
    const queued = pending;
    queued.timer = window.setTimeout(
      () => flushIsolatedParameterWrite(parameterIndex, queued),
      ISOLATED_PARAMETER_DEBOUNCE_MS,
    );
  }, [flushIsolatedParameterWrite]);

  useEffect(() => () => {
    for (const pending of isolatedWritesRef.current.values()) {
      window.clearTimeout(pending.timer);
      for (const requestId of pending.requestIds) {
        sendPluginResponse(requestId, false, "Rack Slot editor was closed.");
      }
    }
    isolatedWritesRef.current.clear();
  }, [sendPluginResponse]);

  const resetParameterForLink = useCallback(async (parameterIndex: number) => {
    const current = await loadParameterSchemaForLink();
    const parameter = current.schema.parameters.find(
      (candidate) => candidate.index === parameterIndex,
    );
    if (!parameter) throw new Error(`Plugin parameter ${parameterIndex} no longer exists.`);
    if (parameter.flags.read_only || parameter.kind.type === "meter") {
      throw new Error(`${parameter.name} is read-only and cannot be reset.`);
    }

    const selectedSoundId = isolated
      ? isolatedContextState?.selected_sound_id
      : instance.selected_sound_id;
    let resetValue: number | undefined;
    if (selectedSoundId) {
      const programState = await materializePluginState(instance.plugin_id, selectedSoundId);
      const programParameters = await requestPluginStateParameters(programState);
      resetValue = programParameters.values.find(
        (candidate) => candidate.index === parameterIndex,
      )?.value;
    }
    if (resetValue === undefined) {
      if ("default" in parameter.kind) {
        resetValue = Number(parameter.kind.default);
      } else if (parameter.kind.type === "trigger") {
        resetValue = 0;
      }
    }
    if (resetValue === undefined || !Number.isFinite(resetValue)) {
      throw new Error(`No program value is available for ${parameter.name}.`);
    }

    let canonicalValue: number;
    if (isolated) {
      const pending = isolatedWritesRef.current.get(parameterIndex);
      if (pending) {
        window.clearTimeout(pending.timer);
        flushIsolatedParameterWrite(parameterIndex, pending);
      }
      await isolatedWriteChainRef.current;
      const base = await ensureIsolatedState();
      const result = await setPluginStateParameter(base, parameterIndex, resetValue);
      publishIsolatedState(result.state);
      canonicalValue = result.value;
    } else {
      canonicalValue = await setPluginParameter(
        instance.instance_id,
        parameterIndex,
        resetValue,
      );
    }

    frameRef.current?.contentWindow?.postMessage(
      {
        protocol: "rackforge.plugin.web@1",
        kind: "parameter_changed",
        parameter_index: parameterIndex,
        value: canonicalValue,
      },
      window.location.origin,
    );
  }, [
    ensureIsolatedState,
    flushIsolatedParameterWrite,
    instance.instance_id,
    instance.plugin_id,
    instance.selected_sound_id,
    isolated,
    isolatedContextState?.selected_sound_id,
    loadParameterSchemaForLink,
    publishIsolatedState,
  ]);

  const pluginContextReady = !isolated || isolatedContextState !== undefined;
  const lighting = useResolvedLighting();
  const pluginContext = useMemo(() => {
    const contextInstance = pluginContextInstance(
      instance,
      isolated,
      isolatedContextState,
    );
    return {
      protocol: "rackforge.plugin.web@1",
      kind: "context",
      surface,
      instance: contextInstance,
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
        // A plugin draws its own surface and may want to sit in the same light
        // as the machine around it. Advisory: nothing obliges a plugin to read
        // this, and a plugin that ignores it is not broken.
        lighting,
      },
    };
  }, [instance, isolated, isolatedContextState, lighting, snapshot, surface]);

  // Parameter changes can originate outside the iframe (MIDI Learn links,
  // semantic .rfcontroller profiles, automation, or another RackForge
  // surface). Keep the visible plugin UI synchronized with the canonical
  // audio instance and publish only values that actually changed. The
  // request is serialized and visibility-aware so slow bridges cannot build
  // an unbounded polling backlog.
  useEffect(() => {
    const parameterValues = liveParameterValuesRef.current;
    parameterValues.clear();
    if (!frameLoaded || isolated || !selectedSurface) return;

    let cancelled = false;
    let timer = 0;
    const synchronize = async () => {
      if (cancelled) return;
      if (document.visibilityState !== "hidden") {
        try {
          const result = await requestPluginParameters(instance.instance_id);
          if (cancelled) return;
          for (const parameter of result.values) {
            if (parameterValues.get(parameter.index) === parameter.value) continue;
            parameterValues.set(parameter.index, parameter.value);
            frameRef.current?.contentWindow?.postMessage(
              {
                protocol: "rackforge.plugin.web@1",
                kind: "parameter_changed",
                parameter_index: parameter.index,
                value: parameter.value,
              },
              window.location.origin,
            );
          }
        } catch {
          // The regular plugin request path owns user-facing errors. A
          // transient reconnect during background synchronization must not
          // replace a useful plugin UI with repeated warnings.
        }
      }
      if (!cancelled) {
        timer = window.setTimeout(synchronize, LIVE_PARAMETER_SYNC_MS);
      }
    };
    void synchronize();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      parameterValues.clear();
    };
  }, [
    frameDocumentGeneration,
    frameLoaded,
    instance.instance_id,
    isolated,
    selectedSurface,
  ]);

  // The icon reveal: a dim copy of the plugin icon sits under a full-color
  // copy clipped from the top, and the clip retreats bottom-to-top. The
  // iframe gives no real progress, so the fill eases toward ~90% on its
  // own clock and completes the moment the frame reports loaded. The DOM
  // node is driven directly from the animation frame -- rendering React
  // sixty times a second for a clip-path would be its own jank.
  useEffect(() => {
    if (splashGone) return;
    let raf = 0;
    let progress = 0;
    const start = performance.now();
    const step = (now: number) => {
      const lit = splashLitRef.current;
      if (lit) {
        const seconds = (now - start) / 1000;
        const target = frameLoadedRef.current
          ? 1
          : 0.9 * (1 - Math.exp(-seconds / 0.9));
        progress += (Math.max(target, progress) - progress) * 0.12;
        lit.style.clipPath = `inset(${((1 - progress) * 100).toFixed(2)}% 0 0 0)`;
        if (frameLoadedRef.current && progress > 0.995) {
          lit.style.clipPath = "inset(0 0 0 0)";
          setCompletedSplashIdentity(surfaceIdentity);
          return;
        }
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [splashGone, surfaceIdentity]);

  // Insurance for the reveal: animation frames stop in a hidden window
  // (minimized, background tab), and the splash must never outlive the
  // interface it was covering. Once the frame is loaded, a plain timer
  // completes the splash even if no frame ever fires.
  useEffect(() => {
    if (!frameLoaded || splashDone) return;
    const timer = window.setTimeout(
      () => setCompletedSplashIdentity(surfaceIdentity),
      1800,
    );
    return () => window.clearTimeout(timer);
  }, [frameLoaded, splashDone, surfaceIdentity]);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame || !selectedSurface) return;

    const send = (message: unknown) =>
      frame.contentWindow?.postMessage(message, window.location.origin);
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
        setLoadedFrameIdentity(surfaceIdentity);
        setFrameDocumentGeneration((generation) => generation + 1);
        if (pluginContextReady) send(pluginContext);
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
          ensureIsolatedState()
            .then(requestPluginStateParameters)
            .then((result) => respond(true, undefined, result))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not read Rack Slot parameters.",
              ),
            );
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
          queueIsolatedParameterWrite(
            event.data.request_id,
            Number(params.parameter_index),
            params.value,
          );
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
            .then((result) => {
              if (
                isolated &&
                result &&
                typeof result === "object" &&
                "state" in result
              ) {
                publishIsolatedState(
                  (result as { state: PluginStateReference }).state,
                );
              }
              respond(true, undefined, result);
            })
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
          const soundId = params.sound_id;
          dispatchCommandAwait({
            type: "select_sound",
            instance_id: instance.instance_id,
            sound_id: soundId,
          })
            .then(() => respond(true, undefined, { sound_id: soundId }))
            .catch((error: unknown) =>
              respond(
                false,
                error instanceof Error
                  ? error.message
                  : "Could not select this program.",
              ),
            );
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
        } else if (isNativeHost() || isDesktopHost()) {
          pendingResourceRequestRef.current = event.data.request_id;
          const extensions = Array.isArray(params.extensions)
            ? params.extensions
                .filter(
                  (extension: unknown): extension is string =>
                    typeof extension === "string" &&
                    /^\.?[a-z0-9]+$/i.test(extension),
                )
                .slice(0, 16)
            : undefined;
          bindNativePluginResource({
            plugin_id: instance.plugin_id,
            resource_id: resource.id,
            kind: resource.kind,
            extensions,
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
        (surface === "play" || surface === "config")
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
        event.data.method === "plugin.preview_resource" &&
        surface === "config" &&
        params.edit_mode === 1 &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        ) &&
        typeof params.file_name === "string" &&
        params.file_name.length > 0 &&
        params.file_name.length <= 160 &&
        params.bytes instanceof ArrayBuffer &&
        params.bytes.byteLength > 0 &&
        params.bytes.byteLength <= 128 * 1024 * 1024
      ) {
        setResourceBusy("Updating builder audition…");
        const fileName = params.file_name.replace(/[^a-z0-9._ -]+/gi, "-");
        hostJson<ResourceSelection>(
          `/api/v1/resources/uploads?name=${encodeURIComponent(fileName)}`,
          {
            method: "POST",
            headers: { "content-type": "application/octet-stream" },
            body: new Blob([params.bytes], { type: "application/vnd.rackforge.bank+zip" }),
          },
        )
          .then((selection) => postResourceApi<ResourceGrant>(
            "/api/v1/resources/bind-selection",
            {
              plugin_id: instance.plugin_id,
              resource_id: params.target_resource_id,
              selection_id: selection.selection_id,
            },
          ))
          .then((grant) => postResourceApi("/api/v1/resources/load", {
            plugin_id: instance.plugin_id,
            instance_id: instance.instance_id,
            target_resource_id: params.target_resource_id,
            grant_id: grant.grant_id,
            entry_id: null,
            persist: false,
            preview: true,
            bundle: null,
          }))
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not audition this resource.",
            ),
          )
          .finally(() => setResourceBusy(null));
      } else if (
        (((event.data.method === "plugin.load_resource" ||
          event.data.method === "plugin.install_resource") && surface === "config") ||
          (event.data.method === "plugin.activate_resource" && surface === "play")) &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        ) &&
        typeof params.grant_id === "string" &&
        (params.entry_id === null || params.entry_id === undefined ||
          typeof params.entry_id === "string")
      ) {
        const operationLabel = event.data.method === "plugin.activate_resource"
          ? "Activating resource…"
          : event.data.method === "plugin.install_resource"
            ? "Installing resource…"
            : "Loading resource…";
        setResourceBusy(operationLabel);
        postResourceApi("/api/v1/resources/load", {
          plugin_id: instance.plugin_id,
          instance_id: instance.instance_id,
          target_resource_id: params.target_resource_id,
          grant_id: params.grant_id,
          entry_id: typeof params.entry_id === "string" ? params.entry_id : null,
          persist: event.data.method !== "plugin.load_resource",
          preview: false,
          bundle: params.bundle === "nki_dependencies" || params.bundle === "sfz_dependencies"
            ? params.bundle
            : null,
        })
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not load this resource.",
            ),
          )
          .finally(() => setResourceBusy(null));
      } else if (
        event.data.method === "plugin.clear_resource" &&
        surface === "config" &&
        typeof params.target_resource_id === "string" &&
        descriptor?.resources.some(
          (resource) =>
            resource.id === params.target_resource_id && resource.kind === "file",
        )
      ) {
        setResourceBusy("Clearing resource…");
        postResourceApi("/api/v1/resources/clear", {
          plugin_id: instance.plugin_id,
          instance_id: instance.instance_id,
          target_resource_id: params.target_resource_id,
        })
          .then((result) => respond(true, undefined, result))
          .catch((error: unknown) =>
            respond(
              false,
              error instanceof Error ? error.message : "Could not clear this resource.",
            ),
          )
          .finally(() => setResourceBusy(null));
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
    const onLoad = () => {
      if (pluginContextReady) send(pluginContext);
    };
    window.addEventListener("message", onMessage);
    frame.addEventListener("load", onLoad);
    if (pluginContextReady) send(pluginContext);
    return () => {
      window.removeEventListener("message", onMessage);
      frame.removeEventListener("load", onLoad);
    };
  }, [
    descriptor,
    ensureIsolatedState,
    instance,
    isolated,
    isolatedContextState,
    onSelectSound,
    onSurfaceInfoChange,
    publishIsolatedState,
    queueIsolatedParameterWrite,
    selectedSurface,
    snapshot,
    surface,
    surfaceIdentity,
    pluginContext,
    pluginContextReady,
  ]);

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
          sandbox="allow-scripts allow-same-origin allow-downloads"
          referrerPolicy="same-origin"
          onLoad={() => {
            setLoadedFrameIdentity(surfaceIdentity);
            setFrameDocumentGeneration((generation) => generation + 1);
            // Plugin surfaces are same-origin, sandboxed documents. Give them
            // RackForge's low-specificity scrollbar defaults while allowing a
            // plugin stylesheet to replace the theme deliberately.
            try {
              const frameDocument = frameRef.current?.contentDocument;
              if (
                frameDocument?.head &&
                !frameDocument.querySelector("link[data-rackforge-scrollbars]")
              ) {
                const link = frameDocument.createElement("link");
                link.rel = "stylesheet";
                link.href = new URL(
                  "rackforge-scrollbars.css",
                  window.document.baseURI,
                ).href;
                link.dataset.rackforgeScrollbars = "true";
                frameDocument.head.append(link);
              }
            } catch {
              // A plugin that intentionally navigates away from the host
              // origin remains isolated and simply keeps its own scrollbar.
            }
            // A cached plugin can post `ready` before React's effect installs
            // the message listener. The load event happens after the plugin
            // has installed its own listener, so publishing the idempotent
            // context here closes that race without plugin-specific timing.
            if (pluginContextReady) {
              frameRef.current?.contentWindow?.postMessage(
                pluginContext,
                window.location.origin,
              );
            }
          }}
        />
        {!splashGone && (
          <div
            className={`plugin-brand-splash${splashDone ? " done" : ""}`}
            aria-label={`Loading ${instance.plugin_name}`}
            onTransitionEnd={(event) => {
              if (event.target === event.currentTarget && splashDone) {
                setHiddenSplashIdentity(surfaceIdentity);
              }
            }}
          >
            {descriptor?.branding ? (
              <>
                <img className="splash-bg" src={descriptor.branding.splash_url} alt="" />
                <div className="splash-icon" aria-hidden="true">
                  <img className="splash-icon-dim" src={descriptor.branding.icon_url} alt="" />
                  <img
                    ref={splashLitRef}
                    className="splash-icon-lit"
                    src={descriptor.branding.icon_url}
                    alt=""
                  />
                </div>
              </>
            ) : (
              <RfLoader label={instance.plugin_name} detail="Loading plugin interface…" size="medium" />
            )}
          </div>
        )}
        {resourceBusy ? (
          <div className="plugin-operation-overlay" role="status" aria-live="polite">
            <AsyncSpinner label={resourceBusy} size="large" />
            <strong>{resourceBusy}</strong>
            <small>RackForge is validating and applying the selected file.</small>
          </div>
        ) : null}
        {isolatedBootstrapError ? (
          <div className="plugin-operation-overlay plugin-operation-error" role="alert">
            <strong>Rack Slot instrument could not start</strong>
            <small>{isolatedBootstrapError}</small>
          </div>
        ) : null}
      </div>
      <ParameterLinkHost
        frameRef={frameRef}
        frameLoaded={frameLoaded}
        frameDocumentGeneration={frameDocumentGeneration}
        instanceId={parameterLinkInstanceId ?? instance.instance_id}
        links={snapshot?.parameter_links ?? []}
        loadParameters={loadParameterSchemaForLink}
        resetParameter={resetParameterForLink}
      />
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
          <AsyncActionLabel active={busy} activeLabel="Changing PIN…">
            Change PIN
          </AsyncActionLabel>
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
  audioSettings: HostAudioSettings | null;
}

let hostSettingsBootstrapPromise: Promise<HostSettingsBootstrap> | null = null;

function requestHostSettingsBootstrap() {
  hostSettingsBootstrapPromise ??= Promise.all([
    hostJson<WebPublicConfig>("/api/v1/config").catch(() => null),
    hostJson<HostAudioSettings>("/api/v1/host/audio").catch(() => null),
  ]).then(([config, audioSettings]) => ({
    config,
    audioSettings,
  }));
  return hostSettingsBootstrapPromise;
}

// The typing keyboard: play notes from the computer keys, FL Studio's
// layout -- the Z row is one octave (Z = C3), the Q row the next
// (Q = middle C), sharps on the row above each. Disabled by default and
// enabled from Settings > Input; text fields always win.
const TYPING_KEYBOARD_STORAGE = "rackforge.typing-keyboard.enabled";
const TYPING_KEY_NOTES: Record<string, number> = {
  KeyZ: 48, KeyS: 49, KeyX: 50, KeyD: 51, KeyC: 52, KeyV: 53, KeyG: 54,
  KeyB: 55, KeyH: 56, KeyN: 57, KeyJ: 58, KeyM: 59, Comma: 60, KeyL: 61,
  Period: 62, Semicolon: 63, Slash: 64,
  KeyQ: 60, Digit2: 61, KeyW: 62, Digit3: 63, KeyE: 64, KeyR: 65,
  Digit5: 66, KeyT: 67, Digit6: 68, KeyY: 69, Digit7: 70, KeyU: 71,
  KeyI: 72, Digit9: 73, KeyO: 74, Digit0: 75, KeyP: 76, BracketLeft: 77,
  Equal: 78, BracketRight: 79,
};

function typingKeyboardEnabled(): boolean {
  try {
    return localStorage.getItem(TYPING_KEYBOARD_STORAGE) === "1";
  } catch {
    return false;
  }
}

function setTypingKeyboardEnabled(enabled: boolean) {
  try {
    localStorage.setItem(TYPING_KEYBOARD_STORAGE, enabled ? "1" : "0");
  } catch {
    /* volatile hosts still toggle for the session */
  }
  window.dispatchEvent(new Event("rackforge:typing-keyboard"));
}

function TypingKeyboardListener() {
  const [enabled, setEnabled] = useState(typingKeyboardEnabled);
  useEffect(() => {
    const sync = () => setEnabled(typingKeyboardEnabled());
    window.addEventListener("rackforge:typing-keyboard", sync);
    return () => window.removeEventListener("rackforge:typing-keyboard", sync);
  }, []);
  useEffect(() => {
    if (!enabled) return;
    const held = new Set<number>();
    const isTextTarget = (target: EventTarget | null) => {
      const element = target as HTMLElement | null;
      if (!element) return false;
      const tag = element.tagName;
      if (tag === "TEXTAREA" || tag === "SELECT") return true;
      if (element.isContentEditable) return true;
      if (tag === "INPUT") {
        // Only TEXT entry wins over the notes; a focused fader, checkbox
        // or button is not typing.
        const type = (element as HTMLInputElement).type;
        return !["checkbox", "radio", "range", "button", "color"].includes(type);
      }
      return false;
    };
    const down = (event: KeyboardEvent) => {
      if (event.repeat || event.ctrlKey || event.metaKey || event.altKey) return;
      if (isTextTarget(event.target)) return;
      const note = TYPING_KEY_NOTES[event.code];
      if (note === undefined || held.has(note)) return;
      event.preventDefault();
      held.add(note);
      sendVirtualMidi(0x90, note, 100);
    };
    const up = (event: KeyboardEvent) => {
      const note = TYPING_KEY_NOTES[event.code];
      if (note === undefined || !held.has(note)) return;
      held.delete(note);
      sendVirtualMidi(0x80, note, 0);
    };
    const releaseAll = () => {
      for (const note of held) sendVirtualMidi(0x80, note, 0);
      held.clear();
    };
    // Keyboard events do not cross frame boundaries, and in Play mode the
    // player's focus usually sits inside the plugin panel's iframe -- so
    // the listener rides along into every same-origin frame, re-scanned as
    // panels mount and unmount.
    const attached = new Set<Window>();
    const attach = (target: Window) => {
      if (attached.has(target)) return;
      attached.add(target);
      target.addEventListener("keydown", down);
      target.addEventListener("keyup", up);
    };
    attach(window);
    const scanFrames = () => {
      for (const frame of Array.from(document.querySelectorAll("iframe"))) {
        try {
          const inner = (frame as HTMLIFrameElement).contentWindow;
          if (inner && inner.document) attach(inner);
        } catch {
          /* cross-origin frames keep their keys */
        }
      }
    };
    scanFrames();
    const scanner = window.setInterval(scanFrames, 2000);
    window.addEventListener("blur", releaseAll);
    return () => {
      window.clearInterval(scanner);
      for (const target of attached) {
        try {
          target.removeEventListener("keydown", down);
          target.removeEventListener("keyup", up);
        } catch {
          /* a navigated-away frame is already gone */
        }
      }
      window.removeEventListener("blur", releaseAll);
      releaseAll();
    };
  }, [enabled]);
  return null;
}

function TypingKeyboardCard() {
  const [enabled, setEnabled] = useState(typingKeyboardEnabled);
  const toggle = (next: boolean) => {
    setEnabled(next);
    setTypingKeyboardEnabled(next);
  };
  return (
    <article className="settings-card">
      <div className="settings-icon settings-icon-svg">
        <Keyboard aria-hidden="true" strokeWidth={1.7} />
      </div>
      <div className="settings-copy">
        <span className="card-kicker">Computer keys</span>
        <h2>Typing Keyboard</h2>
        <p>
          Play notes with the computer keyboard, FL Studio layout: the Z row
          is one octave, the Q row the next, sharps on the row above each.
          Text fields always take priority.
        </p>
      </div>
      <ToggleSwitch
        className="typing-keyboard-switch"
        checked={enabled}
        label="Typing input"
        description={enabled ? "Computer keys play notes" : "Computer keys are ignored"}
        checkedLabel="Enabled"
        uncheckedLabel="Disabled"
        onChange={toggle}
      />
    </article>
  );
}

/**
 * The cover over a plugin surface.
 *
 * On by default because it is what says the plugin is running *on* something.
 * Off is a real option, not a debug flag: a dense panel is easier to read
 * through nothing at all, and that trade belongs to whoever is playing.
 */
function ScreenGlassCard() {
  const [glass, setGlass] = useState<ScreenGlass>(() => readScreenGlass());
  const on = glass === "glass";
  const choose = (next: ScreenGlass) => {
    setGlass(next);
    storeScreenGlass(next);
  };

  return (
    <article className="settings-card">
      <div className="settings-icon settings-icon-svg">
        <MonitorSmartphone aria-hidden="true" />
      </div>
      <div className="settings-copy">
        <span className="card-kicker">Plugin surface</span>
        <h2>Screen glass</h2>
        <p>
          Shows plugin panels behind an acrylic cover: the corners fall off, a
          sheen crosses the sheet, and it carries the film any panel picks up.
          Panels render bare unless you turn it on.
        </p>
      </div>
      <ToggleSwitch
        className="typing-keyboard-switch"
        checked={on}
        label="Screen glass"
        description={on ? "Panels sit behind the cover" : "Panels render bare"}
        checkedLabel="On"
        uncheckedLabel="Off"
        onChange={(next) => choose(next ? "glass" : "clean")}
      />
    </article>
  );
}

const SETTINGS_TABS = [
  ["audio", "Audio"],
  ["midi", "MIDI"],
  ["input", "Input"],
  ["screen", "Screen"],
  // Network is the local HTTP server, and the PIN guards exactly that: it is
  // one subject, so it is one section. Security used to sit beside it, which
  // asked the player to know that the passcode belonged to the server.
  ["network", "Network"],
] as const;

type SettingsTab = (typeof SETTINGS_TABS)[number][0];

/** The sections this host actually has.
 *
 * The browser demo is its own host: the page is the instrument, and there is
 * no HTTP server to publish, no port to choose and no PIN to guard it with.
 * Offering the section would be offering settings that answer to nothing.
 */
function settingsTabsFor(browserHost: boolean): ReadonlyArray<readonly [SettingsTab, string]> {
  return browserHost ? SETTINGS_TABS.filter(([id]) => id !== "network") : SETTINGS_TABS;
}

function isSettingsTab(value: string | null, browserHost: boolean): value is SettingsTab {
  return settingsTabsFor(browserHost).some(([id]) => id === value);
}

function SettingsPage({
  initial,
  onConfigChange,
  onAudioChange,
}: {
  initial: HostSettingsBootstrap;
  onConfigChange: (config: WebPublicConfig) => void;
  onAudioChange: (audioSettings: HostAudioSettings) => void;
}) {
  const [config, setConfig] = useState<WebPublicConfig | null>(initial.config);
  const [webDraft, setWebDraft] = useState<{ enabled: boolean; port: number } | null>(
    initial.config ? { enabled: initial.config.enabled, port: initial.config.port } : null,
  );
  const [webBusy, setWebBusy] = useState(false);
  const [webMessage, setWebMessage] = useState<string | null>(null);
  const [audioSettings, setAudioSettings] = useState<HostAudioSettings | null>(initial.audioSettings);
  const [audioDraft, setAudioDraft] = useState<HostAudioPreferences | null>(initial.audioSettings?.preferences ?? null);
  const [audioOperation, setAudioOperation] = useState<"refresh" | "test" | "save" | null>(null);
  const audioBusy = audioOperation !== null;
  const [audioMessage, setAudioMessage] = useState<string | null>(null);
  // The tab lives in the URL so a section stays linkable. `replace` keeps
  // tab-hopping out of the back button.
  const [searchParams, setSearchParams] = useSearchParams();
  const tabs = settingsTabsFor(IS_BROWSER_HOST);
  // `security` was its own section until the PIN moved in beside the server it
  // protects. A link someone kept still lands where the passcode now lives.
  const requestedTab = searchParams.get("tab") === "security"
    ? "network"
    : searchParams.get("tab");
  const settingsTab: SettingsTab = isSettingsTab(requestedTab, IS_BROWSER_HOST)
    ? requestedTab
    : "audio";
  const setSettingsTab = (tab: SettingsTab) => {
    setSearchParams(tab === "audio" ? {} : { tab }, { replace: true });
  };
  const loadAudioSettings = useCallback(async () => {
    setAudioOperation("refresh");
    setAudioMessage(null);
    try {
      const settings = await hostJson<HostAudioSettings>("/api/v1/host/audio");
      setAudioSettings(settings);
      setAudioDraft(settings.preferences);
      onAudioChange(settings);
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "Device refresh failed.");
    } finally {
      setAudioOperation(null);
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
      input_device: undefined,
      input_channels: [],
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
  const selectAudioInput = (name: string) => {
    if (!audioSettings || !audioDraft) return;
    if (!name) {
      setAudioDraft({ ...audioDraft, input_device: undefined, input_channels: [] });
      return;
    }
    const input = (audioSettings.inventory.inputs ?? []).find(
      (candidate) => candidate.driver === audioDraft.driver && candidate.name === name,
    );
    if (!input) return;
    setAudioDraft({
      ...audioDraft,
      input_device: input.name,
      input_channels: input.channels > 0 ? [1] : [],
      input_gain_db: audioDraft.input_gain_db ?? 0,
      sample_rate_hz: input.sample_rates.includes(audioDraft.sample_rate_hz)
        ? audioDraft.sample_rate_hz
        : input.default_sample_rate,
      buffer_frames: input.buffer_frames.includes(audioDraft.buffer_frames ?? -1)
        ? audioDraft.buffer_frames
        : undefined,
    });
  };
  const saveAudioSettings = async () => {
    if (!audioDraft) return;
    setAudioOperation("save");
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
      setAudioOperation(null);
    }
  };
  const testAudio = async () => {
    setAudioOperation("test");
    setAudioMessage(null);
    try {
      await hostJson("/api/v1/host/audio/test", { method: "POST" });
      setAudioMessage("Playing test note.");
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "Audio test failed.");
    } finally {
      setAudioOperation(null);
    }
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

  return (
    <>
      <PageHeading
        eyebrow="RackForge"
        title="Settings"
        detail="Host-wide configuration. Plugin-specific controls live inside each plugin."
      />
      <nav className="settings-tabs" role="tablist" aria-label="Settings sections">
        {tabs.map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={settingsTab === id}
            className={settingsTab === id ? "active" : ""}
            onClick={() => setSettingsTab(id)}
          >
            {label}
          </button>
        ))}
      </nav>
      <section className="settings-grid">
        {settingsTab === "input" ? <TypingKeyboardCard /> : null}
        {settingsTab === "screen" ? <ScreenGlassCard /> : null}
        {/* Audio and MIDI are separate sections of one card: they are different
            things to set up, but they share a draft and one Apply commits both,
            so splitting them into two cards would put two save buttons on the
            same pending edit. */}
        {(settingsTab === "audio" || settingsTab === "midi") && audioSettings && audioDraft ? (
          <article className="settings-card host-audio-settings-card">
            <div className="settings-icon">{settingsTab === "midi" ? "⌸" : "♫"}</div>
            <div className="settings-copy">
              <span className="card-kicker">{audioSettings.host} host</span>
              <h2>{settingsTab === "midi" ? "MIDI" : "Audio"}</h2>
              <p>
                {settingsTab === "midi"
                  ? "Inputs this device offers. Enabled ports are opened by the native runtime."
                  : "Available controls are provided by this device and applied by its native audio runtime."}
              </p>
            </div>
            <div className="host-audio-form">
              {settingsTab === "audio" ? (
              <>
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
              <label>
                <span>Audio input</span>
                <select
                  value={audioDraft.input_device ?? ""}
                  onChange={(event) => selectAudioInput(event.target.value)}
                >
                  <option value="">Disabled</option>
                  {(audioSettings.inventory.inputs ?? [])
                    .filter((input) => input.driver === audioDraft.driver)
                    .map((input) => (
                      <option key={input.name} value={input.name}>
                        {input.name}{input.is_default ? " (default)" : ""}
                      </option>
                    ))}
                </select>
              </label>
              {(() => {
                const input = (audioSettings.inventory.inputs ?? []).find(
                  (candidate) => candidate.driver === audioDraft.driver
                    && candidate.name === audioDraft.input_device,
                );
                if (!input) return null;
                const selected = audioDraft.input_channels ?? [];
                return (
                  <fieldset>
                    <legend>Audio input channels</legend>
                    {Array.from({ length: input.channels }, (_, index) => index + 1).map((channel) => (
                      <label className="host-audio-check" key={channel}>
                        <input
                          type="checkbox"
                          checked={selected.includes(channel)}
                          disabled={!selected.includes(channel) && selected.length >= 2}
                          onChange={(event) => setAudioDraft({
                            ...audioDraft,
                            input_channels: event.target.checked
                              ? [...selected, channel].sort((left, right) => left - right)
                              : selected.filter((candidate) => candidate !== channel),
                          })}
                        />
                        <span>Input {channel}</span>
                      </label>
                    ))}
                    <small>Choose one channel for a mono source such as a guitar, or two for stereo.</small>
                  </fieldset>
                );
              })()}
              {audioDraft.input_device ? (
                <label>
                  <span>Input trim</span>
                  <select
                    value={audioDraft.input_gain_db ?? 0}
                    onChange={(event) => setAudioDraft({
                      ...audioDraft,
                      input_gain_db: Number(event.target.value),
                    })}
                  >
                    {[-24, -18, -12, -6, 0, 3, 6, 9, 12, 18, 24].map((gain) => (
                      <option key={gain} value={gain}>{gain > 0 ? "+" : ""}{gain} dB</option>
                    ))}
                  </select>
                </label>
              ) : null}
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
              </>
              ) : null}
              {settingsTab === "midi" ? (
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
              ) : null}
              {settingsTab === "audio" && audioSettings.runtime ? (() => {
                const runtime = audioSettings.runtime;
                const health = runtime.stream_health ?? (runtime.running ? "healthy" : "stopped");
                const metrics = [
                  runtime.sample_rate
                    ? ["Actual rate", `${runtime.sample_rate} Hz`]
                    : null,
                  runtime.buffer_size_frames
                    ? ["Active buffer", `${runtime.buffer_size_frames} frames`]
                    : null,
                  typeof runtime.callback_load_percent === "number"
                    ? ["Audio load", `${runtime.callback_load_percent.toFixed(1)}%`]
                    : null,
                  typeof runtime.xruns === "number"
                    ? ["Buffer underruns", String(runtime.xruns)]
                    : null,
                  typeof runtime.midi_dropped_events === "number"
                    ? ["Dropped MIDI", String(runtime.midi_dropped_events)]
                    : null,
                  // Whether the instrument is actually spread across cores.
                  // Without this the sequential fallback is indistinguishable
                  // from the pool, which is how a silent one went unnoticed.
                  runtime.render_pool
                    ? [
                        "Render pool",
                        runtime.render_pool.workers > 0
                          ? `${runtime.render_pool.workers} worker${
                              runtime.render_pool.workers === 1 ? "" : "s"
                            }`
                          : runtime.render_pool.isolated
                            ? "sequential"
                            : "sequential · unavailable",
                      ]
                    : null,
                  runtime.render_pool && runtime.render_pool.workers > 0
                    ? ["Late blocks", String(runtime.render_pool.missed_blocks)]
                    : null,
                ].filter((metric): metric is [string, string] => metric !== null);
                return (
                  <section className={`host-runtime-health ${health}`} aria-label="Audio runtime health">
                    <header>
                      <div>
                        <span>Runtime health</span>
                        <strong>{health}</strong>
                      </div>
                      <i aria-hidden="true" />
                    </header>
                    {metrics.length ? (
                      <dl>
                        {metrics.map(([label, value]) => (
                          <div key={label}>
                            <dt>{label}</dt>
                            <dd>{value}</dd>
                          </div>
                        ))}
                      </dl>
                    ) : null}
                    {runtime.render_pool?.reason ? (
                      <p className="host-runtime-note">{runtime.render_pool.reason}</p>
                    ) : null}
                  </section>
                );
              })() : null}
              <div className="host-audio-actions">
                <button className="secondary-button" disabled={audioBusy} onClick={() => void loadAudioSettings()}>
                  <AsyncActionLabel active={audioOperation === "refresh"} activeLabel="Refreshing…">
                    Refresh devices
                  </AsyncActionLabel>
                </button>
                <button className="secondary-button" disabled={audioBusy} onClick={() => void testAudio()}>
                  <AsyncActionLabel active={audioOperation === "test"} activeLabel="Playing…">
                    Test note
                  </AsyncActionLabel>
                </button>
                <button className="primary-button" disabled={audioBusy} onClick={() => void saveAudioSettings()}>
                  <AsyncActionLabel active={audioOperation === "save"} activeLabel="Applying…">Apply</AsyncActionLabel>
                </button>
              </div>
              {audioMessage ? <p className="settings-message">{audioMessage}</p> : null}
            </div>
          </article>
        ) : null}
        {(settingsTab === "audio" || settingsTab === "midi") && (!audioSettings || !audioDraft) ? (
          <article className="settings-card host-audio-settings-card unavailable">
            <div className="settings-icon">{settingsTab === "midi" ? "⌸" : "♫"}</div>
            <div className="settings-copy">
              <span className="card-kicker">Host capabilities</span>
              <h2>{settingsTab === "midi" ? "MIDI" : "Audio"} unavailable</h2>
              <p>The current host did not publish its audio and MIDI settings.</p>
            </div>
            <div className="host-audio-actions">
              <button
                className="secondary-button"
                disabled={audioBusy}
                onClick={() => void loadAudioSettings()}
              >
                <AsyncActionLabel active={audioOperation === "refresh"} activeLabel="Refreshing…">
                  Try again
                </AsyncActionLabel>
              </button>
            </div>
            {audioMessage ? <p className="settings-message">{audioMessage}</p> : null}
          </article>
        ) : null}
        {settingsTab === "network" ? (
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
              <label className="settings-check">
                <input type="checkbox" checked={webDraft.enabled} onChange={(event) => setWebDraft({ ...webDraft, enabled: event.target.checked })} />
                <span>Enable HTTP server</span>
              </label>
              <label>
                <span>Port</span>
                <input type="number" min="1024" max="65535" disabled={!webDraft.enabled} value={webDraft.port} onChange={(event) => setWebDraft({ ...webDraft, port: Number(event.target.value) })} />
              </label>
              <button className="secondary-button" disabled={webBusy || webDraft.port < 1024 || webDraft.port > 65535} onClick={() => void saveWebSettings()}>
                <AsyncActionLabel active={webBusy} activeLabel="Applying…">
                  Apply server settings
                </AsyncActionLabel>
              </button>
              {webMessage ? <p>{webMessage}</p> : null}
            </div>
          ) : null}
        </article>
        ) : null}
        {settingsTab === "network" ? <ChangePinCard /> : null}

      </section>
    </>
  );
}

/** Which RackForge the player is looking at, in their words rather than the
 *  wire's. Only ever what this build can actually know about itself. */
function hostShellName(health: HostHealth | null): string {
  if (IS_BROWSER_HOST) return "This browser";
  if (isVstHost()) return "VST3 plug-in";
  if (isDesktopHost()) return "Desktop app";
  if (health?.host === "desktop") return "Desktop app";
  if (health?.host === "vst3") return "VST3 plug-in";
  return "Web interface";
}

/* About says what this build is, where it is running and what it speaks.
   Everything on it is read from the host or stamped in at build time — a
   version panel that guesses is worse than none. */
function AboutPage() {
  const health = useHostHealth();
  const hostRevision = health?.revision;
  const drift = hostRevision !== undefined && hostRevision !== __UI_REVISION__;
  const shell = hostShellName(health);
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

        <article className="settings-card">
          <div className="settings-copy">
            <span className="card-kicker">This build</span>
            <h2>{shell}</h2>
            <p>
              The interface and the host binary are stamped separately, so a
              half-finished deploy shows here instead of as a behaviour you
              cannot explain.
            </p>
          </div>
          <dl className="about-facts">
            <div>
              <dt>Interface</dt>
              <dd>{__UI_REVISION__}</dd>
            </div>
            <div>
              <dt>Host</dt>
              <dd>{hostRevision ?? "—"}</dd>
            </div>
          </dl>
          {drift ? (
            <p className="about-drift">
              These disagree. The interface and the host came from different
              builds; reinstall the one that is behind.
            </p>
          ) : null}
        </article>

        <article className="settings-card">
          <div className="settings-copy">
            <span className="card-kicker">Typefaces</span>
            <h2>Set in three</h2>
            <p>
              Chakra Petch for headings, Barlow Semi Condensed for the panel
              legends, JetBrains Mono for anything that must line up in a
              column. All three under the SIL Open Font License, whose text
              ships beside the fonts.
            </p>
          </div>
        </article>
      </section>
    </>
  );
}

function EmptyState({ title }: { title: string }) {
  return (
    <div className="empty-state">
      {/* Was the letters "RF" on an accent square — a stand-in from before the
          mark existed. There is a real one now, and it follows the lighting. */}
      <BrandMark />
      <h2>{title}</h2>
      <p>RackForge will update this view as soon as Core is available.</p>
    </div>
  );
}
