import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { useSelector } from "react-redux";
import { Blocks, House, Play, RadioTower, Settings2 } from "lucide-react";
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
import { LivePage } from "./LivePage";
import type { RootState } from "./store";
import type {
  PluginInstance,
  HostPresetSummary,
  ProgramEditorField,
  ProgramEditorPage,
  ProgramEditorValue,
  PluginWebDescriptor,
  PluginWebSurfaceKind,
  PluginResourceRequirement,
  ResourceGrant,
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

async function postResourceApi<T>(url: string, body: unknown): Promise<T> {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as {
      message?: string;
    } | null;
    throw new Error(payload?.message ?? `HTTP ${response.status}`);
  }
  return (await response.json()) as T;
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
  { path: "/", label: "Home", icon: House },
  { path: "/live", label: "Live", icon: RadioTower },
  { path: "/play", label: "Play", icon: Play },
  { path: "/plugins", label: "Plugins", icon: Blocks },
  { path: "/settings", label: "Settings", icon: Settings2 },
];

export function App() {
  const [auth, setAuth] = useState<WebAuthStatus | null>(null);
  const [authError, setAuthError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const refresh = () =>
      fetch("/api/v1/auth/status")
        .then(async (response) => {
          if (!response.ok) throw new Error(`HTTP ${response.status}`);
          return (await response.json()) as WebAuthStatus;
        })
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
  const isPluginSurface =
    location.pathname === "/play" ||
    location.pathname.startsWith("/plugins/");

  useEffect(() => {
    connectGateway();
    return stopGateway;
  }, []);

  return (
    <div className={`app-shell${isPluginSurface ? " plugin-surface-active" : ""}`}>
      <aside className="rail">
        <div className="brand-lockup" aria-label="RackForge">
          <BrandMark />
          <span className="brand-name">RACKFORGE</span>
        </div>
        <nav className="primary-nav">
          {navItems.map((item) => {
            const Icon = item.icon;
            return (
              <NavLink
                key={item.path}
                to={item.path}
                end={item.path === "/"}
                className={({ isActive }) =>
                  `nav-item${isActive ? " active" : ""}`
                }
              >
                <span className="nav-mark">
                  <Icon aria-hidden="true" strokeWidth={1.9} />
                </span>
                <span>{item.label}</span>
              </NavLink>
            );
          })}
        </nav>
        <ConnectionBadge status={connection} />
      </aside>

      <main className="workspace">
        <TopBar snapshot={snapshot} />
        {error && <div className="error-banner">{error}</div>}
        <div
          className={`page${isPluginSurface ? " plugin-host-page" : ""}${
            location.pathname === "/" ? " home-page" : ""
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
                />
              }
            />
            <Route path="/play" element={<PlayPage snapshot={snapshot} />} />
            <Route
              path="/plugins"
              element={<PluginsPage snapshot={snapshot} />}
            />
            <Route
              path="/plugins/:instanceId"
              element={<PluginPage snapshot={snapshot} />}
            />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </div>
      </main>
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
    fetch(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ pin }),
    })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (!response.ok) {
          if (body?.locked_for) setLockedFor(body.locked_for);
          throw new Error(body?.message ?? "That PIN was not accepted.");
        }
        return body;
      })
      .then(() => onUnlocked())
      .catch((reason: Error) => {
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

function TopBar({ snapshot }: { snapshot: SessionSnapshot | null }) {
  const active = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  );
  const selected = active?.sounds.find(
    (sound) => sound.id === active.selected_sound_id,
  );
  return (
    <header className="topbar">
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

function PlayPage({ snapshot }: { snapshot: SessionSnapshot | null }) {
  const instances = snapshot?.instances ?? [];
  const active =
    instances.find(
      (instance) => instance.instance_id === snapshot?.active_instance_id,
    ) ?? instances[0];
  const [pluginPickerOpen, setPluginPickerOpen] = useState(false);
  const [presetsOpen, setPresetsOpen] = useState(false);
  const [pluginVersions, setPluginVersions] = useState<Record<string, string>>({});
  useEffect(() => {
    let cancelled = false;
    fetch("/api/v1/plugins")
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return (await response.json()) as PluginWebDescriptor[];
      })
      .then((plugins) => {
        if (cancelled) return;
        setPluginVersions(Object.fromEntries(
          plugins.map((plugin) => [plugin.plugin_id, plugin.version]),
        ));
      })
      .catch(() => {
        if (!cancelled) setPluginVersions({});
      });
    return () => {
      cancelled = true;
    };
  }, []);
  if (!active) {
    return (
      <section className="plugin-surface-shell direct-surface">
        <PluginSurfaceState
          title="No instrument available"
          detail="RackForge Core has not registered an active instrument."
        />
      </section>
    );
  }
  return (
    <section className="plugin-surface-shell direct-surface">
      <div className="play-plugin-toolbar">
        <button
          className={`play-header-button back${pluginPickerOpen ? " active" : ""}`}
          onClick={() => {
            setPresetsOpen(false);
            setPluginPickerOpen((open) => !open);
          }}
          aria-expanded={pluginPickerOpen}
        >
          <span aria-hidden="true">▦</span>
          <strong>Select plugin</strong>
        </button>
        <div className="play-plugin-identity">
          <span>PLAY</span>
          <strong>{active.plugin_name}{formatPluginVersion(pluginVersions[active.plugin_id])}</strong>
        </div>
        <button
          className={`play-header-button presets${presetsOpen ? " active" : ""}`}
          onClick={() => {
            setPluginPickerOpen(false);
            setPresetsOpen((open) => !open);
          }}
          aria-expanded={presetsOpen}
        >
          <span className="preset-button-mark" aria-hidden="true">P</span>
          <strong>Presets</strong>
        </button>
      </div>
      <PluginFrame key={active.instance_id} instance={active} surface="play" />
      {pluginPickerOpen && (
        <PluginPickerModal
          active={active}
          instances={instances}
          versions={pluginVersions}
          onClose={() => setPluginPickerOpen(false)}
        />
      )}
      {presetsOpen && (
        <PresetModal instance={active} onClose={() => setPresetsOpen(false)} />
      )}
    </section>
  );
}

function PluginPickerModal({
  active,
  instances,
  versions,
  onClose,
}: {
  active: PluginInstance;
  instances: PluginInstance[];
  versions: Record<string, string>;
  onClose: () => void;
}) {
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);
  const orderedInstances = [
    active,
    ...instances.filter((instance) => instance.instance_id !== active.instance_id),
  ];
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
        <div className="play-plugin-selector modal-list" role="list" aria-label="Instrument plugins">
          {orderedInstances.map((instance, index) => {
            const selected = instance.instance_id === active.instance_id;
            return (
              <button
                className={selected ? "active" : ""}
                key={instance.instance_id}
                onClick={() => {
                  dispatchCommand({ type: "set_active_mode", mode: "play" });
                  if (!selected) {
                    dispatchCommand({
                      type: "select_plugin",
                      instance_id: instance.instance_id,
                    });
                  }
                  onClose();
                }}
              >
                <span className="play-plugin-number">{String(index + 1).padStart(2, "0")}</span>
                <span className="play-plugin-copy">
                  <strong>{instance.plugin_name}{formatPluginVersion(versions[instance.plugin_id])}</strong>
                  <small>{instance.sounds.length} programs · Web instrument</small>
                </span>
                <span className="play-plugin-status">
                  {selected ? "PLAYING" : "SELECT"}<i aria-hidden="true">→</i>
                </span>
              </button>
            );
          })}
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
    fetch("/api/v1/plugins")
      .then(async (response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return (await response.json()) as PluginWebDescriptor[];
      })
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
      <PluginGrid instances={running} expanded />
      {available.length > 0 && (
        <>
          <div className="plugin-section-heading">
            <span className="card-kicker">Installed</span>
            <small>Ready for a safe host activation</small>
          </div>
          <div className="plugin-grid expanded">
            {available.map((plugin, index) => (
              <article className="plugin-card installed-plugin-card" key={plugin.plugin_id}>
                <div className={`plugin-tile tile-${(index + running.length) % 4}`}>
                  <span>{plugin.plugin_name.slice(0, 2).toUpperCase()}</span>
                  <i />
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
  expanded = false,
}: {
  instances: PluginInstance[];
  expanded?: boolean;
}) {
  if (instances.length === 0)
    return <EmptyState title="Waiting for installed plugins" />;
  return (
    <div className={`plugin-grid${expanded ? " expanded" : ""}`}>
      {instances.map((instance, index) => {
        const selected = instance.sounds.find(
          (sound) => sound.id === instance.selected_sound_id,
        );
        return (
          <NavLink
            className="plugin-card"
            to={`/plugins/${encodeURIComponent(instance.instance_id)}`}
            key={instance.instance_id}
          >
            <div className={`plugin-tile tile-${index % 4}`}>
              <span>{instance.plugin_name.slice(0, 2).toUpperCase()}</span>
              <i />
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

function PluginFrame({
  instance,
  surface,
}: {
  instance: PluginInstance;
  surface: PluginWebSurfaceKind;
}) {
  const [descriptor, setDescriptor] = useState<PluginWebDescriptor | null>(null);
  const [descriptorStatus, setDescriptorStatus] = useState<
    "loading" | "ready" | "unavailable" | "error"
  >("loading");
  const snapshot = useSelector((state: RootState) => state.rackforge.snapshot);
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const pendingResourceRequestRef = useRef<string | null>(null);
  const [resourceRequest, setResourceRequest] = useState<{
    requestId: string;
    resource: PluginResourceRequirement;
  } | null>(null);

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
    fetch(`/api/v1/plugins/${encodeURIComponent(instance.plugin_id)}`)
      .then(async (response) => {
        if (response.status === 404) return null;
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return (await response.json()) as PluginWebDescriptor;
      })
      .then((value) => {
        if (!cancelled) {
          setDescriptor(value);
          setDescriptorStatus(value ? "ready" : "unavailable");
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDescriptor(null);
          setDescriptorStatus("error");
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
        requestPluginParameters(instance.instance_id)
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
        setPluginParameter(
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
        dispatchCommand({
          type: "select_sound",
          instance_id: instance.instance_id,
          sound_id: params.sound_id,
        });
        respond(true);
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
        event.data.method === "plugin.begin_program_edit" &&
        (surface === "play" || surface === "config") &&
        (params.program_id === null ||
          (typeof params.program_id === "string" &&
            instance.sounds.some(
              (sound) => sound.id === params.program_id && sound.editable,
            )))
      ) {
        dispatchCommand({
          type: "begin_program_edit",
          instance_id: instance.instance_id,
          ...(typeof params.program_id === "string"
            ? { program_id: params.program_id }
            : {}),
        });
        respond(true);
      } else if (
        event.data.method === "plugin.edit_program_field" &&
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
  }, [descriptor, instance, selectedSurface, snapshot, surface]);

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
      <iframe
        ref={frameRef}
        className="plugin-frame"
        title={`${instance.plugin_name} ${surface}`}
        src={selectedSurface.entry_url}
        sandbox="allow-scripts allow-same-origin"
        referrerPolicy="same-origin"
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
    fetch("/api/v1/auth/status")
      .then((response) => response.json())
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
    fetch("/api/v1/auth/pin", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ pin: next, current_pin: current }),
    })
      .then(async (response) => {
        const body = await response.json().catch(() => null);
        if (!response.ok) throw new Error(body?.message ?? "The PIN was not changed.");
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

function SettingsPage() {
  const [config, setConfig] = useState<WebPublicConfig | null>(null);
  const [repositoryFile, setRepositoryFile] =
    useState<PluginRepositoryFile | null>(null);
  const [catalog, setCatalog] = useState<StoreCatalogResponse | null>(null);
  const [storeBusy, setStoreBusy] = useState(false);
  const [storeMessage, setStoreMessage] = useState<string | null>(null);
  useEffect(() => {
    fetch("/api/v1/config")
      .then((response) => response.json())
      .then((value: WebPublicConfig) => setConfig(value))
      .catch(() => setConfig(null));
    fetch("/api/v1/repositories")
      .then((response) => response.json())
      .then((value: PluginRepositoryFile) => setRepositoryFile(value))
      .catch(() => setRepositoryFile(null));
  }, []);

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

  const saveRepositories = async () => {
    if (!repositoryFile) return;
    setStoreBusy(true);
    setStoreMessage(null);
    try {
      const response = await fetch("/api/v1/repositories", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(repositoryFile),
      });
      const body = await response.json();
      if (!response.ok) throw new Error(body.message ?? "Could not save repositories.");
      setRepositoryFile(body.config);
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
      const response = await fetch("/api/v1/store/catalog");
      const body = await response.json();
      if (!response.ok) throw new Error(body.message ?? "Could not refresh stores.");
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
      const response = await fetch("/api/v1/store/install", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ repository_id: repositoryId, plugin_id: pluginId }),
      });
      const body = await response.json();
      if (!response.ok) throw new Error(body.message ?? "Installation failed.");
      setStoreMessage(
        `${body.plugin_id} ${body.version} installed. Activation is available after a safe plugin reload.`,
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

function EmptyState({ title }: { title: string }) {
  return (
    <div className="empty-state">
      <span>RF</span>
      <h2>{title}</h2>
      <p>RackForge will update this view as soon as Core is available.</p>
    </div>
  );
}
