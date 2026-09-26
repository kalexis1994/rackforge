import {
  
  
  useCallback,
  useEffect,
  
  useMemo,
  useRef,
  useState,
  
  
} from "react";
import { useSelector } from "react-redux";
import {
  
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
  
  
} from "react-router";
import {
  connectGateway,
  
  
  
  
  
  dispatchCommandAwait,
  
  
  
  
  
  
  
  
  
  
  stopGateway,
  
  
} from "./gateway";
import { RfLoader } from "./components/RfLoader";
import { useSurfaceTransition } from "./ui/useSurfaceTransition";
import {
  
  
  hostJson,
  
  
  
  
  
  IS_BROWSER_HOST,
  isRemoteWebClient,
  isVstHost,
  
  
  
  
  syncNativeRoute,
} from "./host";
import {
  
  
  
  
  refreshPluginCatalog,
  synchronizePluginRuntime,
  usePluginCatalog,
  
} from "./pluginCatalog";
import {
  commitPlayPluginSelection,
  
} from "./playPluginSelection";
import {
  defaultInstrument,
  firstRunView,
  hostIsStarting,
  markFirstRunCompleted,
  readFirstRunCompleted,
  shouldRunFirstRun,
  type FirstRunFailure,
} from "./firstRun";
import { FirstRunScreen } from "./FirstRunScreen";
import { BootCurtain } from "./components/BootCurtain";
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
  
  
  
  
  
  
  
  
  PluginWebDescriptor,
  
  
  PluginStateReference,
  
  
  
  
  WebAuthStatus,
  
  
  
  
} from "./types";
import { BrandMark } from "./components/BrandMark";
import { AuthLoading, PinGatePage } from "./pages/PinGatePage";
import { useMediaQuery } from "./hooks/useMediaQuery";
import { ConnectionBadge } from "./components/ConnectionBadge";
import { ControllerDockToggle } from "./components/ControllerDockToggle";
import { LightingSwitch } from "./components/LightingSwitch";
import { navItems,  vstNavItems,   } from "./components/navigation/navItems";
import { NavigationLinks } from "./components/navigation/NavigationLinks";
import { MobileNavigation } from "./components/navigation/MobileNavigation";
import { TopBar } from "./components/navigation/TopBar";
import {    synchronizePluginEnvironment } from "./pluginLifecycle";
import { PlayModeTransitionDialog } from "./dialogs/PlayModeTransitionDialog";
import { InstallPluginDialog } from "./dialogs/InstallPluginDialog";
import { TypingKeyboardListener } from "./components/TypingKeyboardListener";
import {  PluginFrame } from "./components/PluginFrame";
import { ControllerPage, } from "./pages/ControllerPage";
import { ControllersPage } from "./pages/ControllersPage";
import { AboutPage } from "./pages/AboutPage";
import { PlayPage } from "./pages/PlayPage";
import { PluginsPage } from "./pages/PluginsPage";
import { PluginPage } from "./pages/PluginPage";
import { HostSettingsBootstrap, requestHostSettingsBootstrap } from "./pages/settings/hostSettings";
import { SettingsPage } from "./pages/settings/SettingsPage";










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
  const [activationFailure, setActivationFailure] = useState<FirstRunFailure | null>(null);
  const [timedOut, setTimedOut] = useState(false);
  const started = useRef(false);
  const hostStarting = hostIsStarting(plugins);
  const active = shouldRunFirstRun({
    completed,
    sessionKnown,
    hasActiveInstance: Boolean(activeInstanceId),
  });

  // Read, not held: a catalogue that cannot be read, or one with nothing in
  // it, is a state of the host rather than of this screen.
  const failure: FirstRunFailure | null = useMemo(
    () =>
      activationFailure ??
      (catalogStatus === "error"
        ? {
            kind: "catalogue" as const,
            message: "RackForge could not read its plugin catalogue.",
          }
        : catalogStatus === "ready" && !hostStarting && defaultInstrument(plugins) === null
          ? {
              kind: "no_instruments" as const,
              message:
                "This RackForge has no instruments installed yet. Install one and it will open playing.",
            }
          : timedOut
            ? {
                kind: "timeout" as const,
                message: "RackForge is taking longer than usual to start.",
              }
            : null),
    [activationFailure, catalogStatus, hostStarting, plugins, timedOut],
  );

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
    if (catalogStatus !== "ready" || hostStarting) return;
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
        setActivationFailure({
          kind: "activation",
          message:
            error instanceof Error
              ? error.message
              : "RackForge could not open your instrument.",
        });
      }
    })();
  }, [active, catalogStatus, hostStarting, plugins, navigate, finish]);

  // The host says it is starting only in the catalogue, so read it again
  // until it has finished. Usually the session arrives with the instrument
  // the host opened first, and this screen is gone before a second read.
  useEffect(() => {
    if (!active || !hostStarting) return;
    const timer = window.setInterval(() => {
      void refreshPluginCatalog(true).catch(() => undefined);
    }, 1_000);
    return () => window.clearInterval(timer);
  }, [active, hostStarting]);

  // And a host that never answers at all still hands the interface over.
  useEffect(() => {
    if (!active) return;
    const timer = window.setTimeout(() => setTimedOut(true), 25_000);
    return () => window.clearTimeout(timer);
  }, [active]);

  // Another attempt, from the catalogue up: whatever went wrong, the host is
  // asked again rather than the player being sent past it.
  const retry = useCallback(() => {
    started.current = false;
    setActivationFailure(null);
    setTimedOut(false);
    void refreshPluginCatalog(true);
  }, []);

  // The one failure another attempt cannot fix. Plugin Manager is where an
  // instrument comes from, so that is where this goes -- not "continue" into
  // an interface with nothing in it, which is the dead end this screen was
  // built to end.
  const openPluginManager = useCallback(() => {
    finish();
    navigate("/plugins");
  }, [finish, navigate]);

  const view = useMemo(
    () => firstRunView({ catalogStatus, plugins, failure }),
    [catalogStatus, plugins, failure],
  );

  return { active, view, failure, retry, openPluginManager };
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
  // The web builds open onto whatever the engine and the catalogue say, as
  // they arrive; the desktop shows its own startup before its window has an
  // interface at all, and a plugin host has no start of its own.
  const [bootCurtainEnabled] = useState(() => IS_BROWSER_HOST || isRemoteWebClient());
  const bootActiveInstrument = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  )?.plugin_name;
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  // A surface that draws its own menu key -- the node editor's header --
  // asks for the navigation by event rather than by a prop threaded down.
  useEffect(() => {
    const open = () => setMobileMenuOpen(true);
    window.addEventListener("rackforge:open-navigation", open);
    return () => window.removeEventListener("rackforge:open-navigation", open);
  }, []);
  const [installPluginOpen, setInstallPluginOpen] = useState(false);
  const [playOverlay, setPlayOverlay] = useState<"plugins" | "presets" | null>(null);
  const [liveSurface, setLiveSurface] = useState<"perform" | "configure">("perform");
  const [liveWorkspace, setLiveWorkspace] = useState<PerformanceGraphWorkspace | null>(null);
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
  // A section -- About, Controllers, Settings, the plugin list -- scrolls
  // inside itself under a bar that stays put, as the surfaces already do.
  const isSectionPage = !isPluginSurface && !isControllerSurface && !isPerformanceSurface
    && !liveWorkspace && !showControllerDock;
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

  /* Leaving the phone-on-its-side controller closes it wherever it was
     opened: a dock opened upright is what brought it here, and while that
     stayed open the presentation rule sent the player straight back -- Exit
     did nothing. It returns to the page it came from, not to PLAY: from LIVE
     that asked to leave the stage. */
  const exitControllerSurface = useCallback(() => {
    setMobileMenuOpen(false);
    setControllerDockOpen(false);
    navigate(lastContentRoute.current, { replace: true });
  }, [navigate]);

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
    }${isSectionPage ? " section-page-active" : ""
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
        {/* The node editor's Save and Exit are in its own header, at every
            size; the rail keeps only navigation. */}
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
            performance={performance}
            menuOpen={mobileMenuOpen}
            onMenu={() => setMobileMenuOpen((open) => !open)}
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
                  onExit={exitControllerSurface}
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
            <Route
              path="/controllers"
              element={vstHost ? <Navigate to="/plugins" replace /> : <ControllersPage />}
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
            // The chain drawer is PLAY's own; where the bar that opens it is
            // hidden, the menu reaches it through PLAY.
            if (action === "effects") {
              window.dispatchEvent(new Event("rackforge:toggle-play-effects"));
            }
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
      {bootCurtainEnabled ? (
        <BootCurtain
          connection={connection}
          sessionKnown={Boolean(snapshot)}
          catalogStatus={pluginCatalog.status}
          firstRunActive={firstRun.active}
          activeInstrument={bootActiveInstrument}
        />
      ) : null}
      {firstRun.active ? (
        <FirstRunScreen
          view={firstRun.view}
          failure={firstRun.failure}
          onRetry={firstRun.retry}
          onOpenPluginManager={firstRun.openPluginManager}
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
