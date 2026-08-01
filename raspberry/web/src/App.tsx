import { useEffect, useRef, useState, type FormEvent } from "react";
import { useSelector } from "react-redux";
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
  dispatchCommand,
  loadPluginPreset,
  requestPluginPresets,
  savePluginPreset,
  stopGateway,
} from "./gateway";
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
  SessionSnapshot,
  WebAuthStatus,
  WebPublicConfig,
} from "./types";

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
  { path: "/", label: "Home", mark: "⌂" },
  { path: "/live", label: "Live", mark: "◆" },
  { path: "/play", label: "Play", mark: "▶" },
  { path: "/plugins", label: "Plugins", mark: "▦" },
  { path: "/settings", label: "Settings", mark: "⚙" },
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
  if (auth.requires_pairing) {
    return (
      <PairDevicePage
        pairingActive={auth.pairing_active}
        onPaired={() =>
          setAuth({ ...auth, paired: true, requires_pairing: false })
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
    <div className="app-shell">
      <aside className="rail">
        <div className="brand-lockup" aria-label="RackForge">
          <span className="brand-mark">RF</span>
          <span className="brand-name">RACKFORGE</span>
        </div>
        <nav className="primary-nav">
          {navItems.map((item) => (
            <NavLink
              key={item.path}
              to={item.path}
              end={item.path === "/"}
              className={({ isActive }) =>
                `nav-item${isActive ? " active" : ""}`
              }
            >
              <span className="nav-mark">{item.mark}</span>
              <span>{item.label}</span>
            </NavLink>
          ))}
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
      <div className="pairing-panel compact">
        <span className="brand-mark">RF</span>
        <p>{message}</p>
      </div>
    </main>
  );
}

function PairDevicePage({
  pairingActive,
  onPaired,
}: {
  pairingActive: boolean;
  onPaired: () => void;
}) {
  const [code, setCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (code.length !== 6) return;
    setSubmitting(true);
    setError(null);
    fetch("/api/v1/auth/pair", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code }),
    })
      .then(async (response) => {
        if (!response.ok) throw new Error("Invalid or expired pairing code.");
        return response.json();
      })
      .then(() => onPaired())
      .catch((reason: Error) => setError(reason.message))
      .finally(() => setSubmitting(false));
  };

  return (
    <main className="pairing-shell">
      <section className="pairing-panel">
        <div className="pairing-brand">
          <span className="brand-mark">RF</span>
          <span>RACKFORGE</span>
        </div>
        <span className="eyebrow accent">Secure device access</span>
        <h1>Pair this browser</h1>
        <p>
          {pairingActive
            ? "Enter the six-digit code shown on the Arturia display."
            : "On the Arturia, open CONFIG → SYSTEM → WEB INTERFACE → PAIR DEVICE and press OK."}
        </p>
        <form onSubmit={submit}>
          <input
            value={code}
            onChange={(event) =>
              setCode(event.target.value.replace(/\D/g, "").slice(0, 6))
            }
            inputMode="numeric"
            autoComplete="one-time-code"
            placeholder="000000"
            aria-label="Six-digit pairing code"
            disabled={!pairingActive || submitting}
          />
          <button
            className="primary-button"
            disabled={!pairingActive || code.length !== 6 || submitting}
          >
            {submitting ? "Pairing…" : "Pair device"}
          </button>
        </form>
        {error && <div className="pairing-error">{error}</div>}
        <small>Codes expire after two minutes and allow five attempts.</small>
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
  const [view, setView] = useState<"menu" | "start" | "presets">("menu");
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
  if (view === "start") {
    return (
      <section className="plugin-surface-shell direct-surface">
        <div className="play-action-bar">
          <button onClick={() => setView("menu")}>← {active.plugin_name}</button>
          <button onClick={() => setView("presets")}>Presets</button>
        </div>
        <PluginFrame key={active.instance_id} instance={active} surface="play" />
      </section>
    );
  }
  if (view === "presets") {
    return (
      <PlayPresets
        instance={active}
        onBack={() => setView("menu")}
        onLoaded={() => setView("start")}
      />
    );
  }
  return (
    <section className="play-launcher">
      <div className="play-launcher-title">
        <span className="eyebrow">PLAY · Plugin</span>
        <h1>{active.plugin_name}</h1>
        <p>Start from the current state or restore a reusable RackForge preset.</p>
      </div>
      <div className="play-launcher-actions">
        <button
          className="play-launch-card primary"
          onClick={() => {
            dispatchCommand({ type: "set_active_mode", mode: "play" });
            setView("start");
          }}
        >
          <span>01</span><strong>START</strong><small>Open the plugin</small>
        </button>
        <button className="play-launch-card" onClick={() => setView("presets")}>
          <span>02</span><strong>PRESETS</strong><small>Load or save complete state</small>
        </button>
      </div>
    </section>
  );
}

function PlayPresets({
  instance,
  onBack,
  onLoaded,
}: {
  instance: PluginInstance;
  onBack: () => void;
  onLoaded: () => void;
}) {
  const [mode, setMode] = useState<"load" | "save">("load");
  const [presets, setPresets] = useState<HostPresetSummary[]>([]);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const refresh = () =>
    requestPluginPresets(instance.plugin_id)
      .then(setPresets)
      .catch((error: Error) => setMessage(error.message));
  useEffect(() => {
    void refresh();
  }, [instance.plugin_id]);
  const load = (preset: HostPresetSummary) => {
    setBusy(true);
    setMessage(null);
    loadPluginPreset(instance.instance_id, preset.id)
      .then(onLoaded)
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
        setMessage(`Saved ${preset.name}.`);
        return refresh();
      })
      .catch((error: Error) => setMessage(error.message))
      .finally(() => setBusy(false));
  };
  return (
    <section className="preset-workspace">
      <header className="preset-workspace-header">
        <button onClick={onBack}>← Back</button>
        <div><span className="eyebrow">{instance.plugin_name}</span><h1>Presets</h1></div>
        <div className="preset-mode-tabs">
          <button className={mode === "load" ? "active" : ""} onClick={() => setMode("load")}>Load</button>
          <button className={mode === "save" ? "active" : ""} onClick={() => setMode("save")}>Save</button>
        </div>
      </header>
      {message && <p className="preset-message">{message}</p>}
      {mode === "load" ? (
        <div className="preset-list">
          {presets.length === 0 ? (
            <EmptyState title="No RackForge presets saved for this plugin" />
          ) : presets.map((preset) => (
            <button disabled={busy} onClick={() => load(preset)} key={preset.id}>
              <span><strong>{preset.name}</strong><small>State v{preset.state_version} · {preset.plugin_version}</small></span>
              <i>LOAD →</i>
            </button>
          ))}
        </div>
      ) : (
        <form className="preset-save-form" onSubmit={save}>
          <label><span>Preset name</span><input autoFocus maxLength={96} value={name} onChange={(event) => setName(event.target.value)} placeholder="Warm Piano" /></label>
          <p>This captures the plugin exactly as it is now. Existing Slots remain independent.</p>
          <button disabled={busy || !name.trim()} type="submit">{busy ? "Saving…" : "Save current state"}</button>
        </form>
      )}
    </section>
  );
}

function PluginsPage({ snapshot }: { snapshot: SessionSnapshot | null }) {
  return (
    <>
      <PageHeading
        eyebrow="Plugin rack"
        title="Sound engines"
        detail="Every plugin owns its programs and editor while RackForge provides the host session."
      />
      <PluginGrid instances={snapshot?.instances ?? []} expanded />
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
  return <PluginSurfaceTabs instance={instance} />;
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

function PluginSurfaceTabs({ instance }: { instance: PluginInstance }) {
  const [surface, setSurface] = useState<PluginWebSurfaceKind>("play");
  return (
    <section className="plugin-surface-shell">
      <div className="plugin-surface-toolbar">
        <div className="plugin-surface-identity">
          <NavLink to="/plugins" aria-label="Back to plugins">
            ←
          </NavLink>
          <strong>{instance.plugin_name}</strong>
        </div>
        <div className="plugin-surface-tabs" role="tablist">
          <button
            className={surface === "play" ? "active" : ""}
            onClick={() => setSurface("play")}
          >
            Play
          </button>
          <button
            className={surface === "config" ? "active" : ""}
            onClick={() => setSurface("config")}
          >
            Config
          </button>
        </div>
      </div>
      <PluginFrame
        key={instance.instance_id}
        instance={instance}
        surface={surface}
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
      const respond = (ok: boolean, error?: string) =>
        send({
          protocol: "rackforge.plugin.web@1",
          kind: "response",
          request_id: event.data.request_id,
          ok,
          ...(error ? { error } : {}),
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
        event.data.method === "plugin.select_sound" &&
        surface === "play" &&
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
        event.data.method === "plugin.begin_program_edit" &&
        surface === "config" &&
        (params.program_id === null ||
          (typeof params.program_id === "string" &&
            instance.sounds.some(
              (sound) =>
                sound.id === params.program_id && sound.bank === "custom",
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
        surface === "config" &&
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
        surface === "config" &&
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
        surface === "config" &&
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
        surface === "config" &&
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
        surface === "config" &&
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
  }, [instance, selectedSurface, snapshot, surface]);

  const editLease =
    surface === "config" &&
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

  if (descriptorStatus === "loading") {
    return (
      <PluginSurfaceState
        title={`Connecting to ${instance.plugin_name}`}
        detail="Loading the plugin web interface."
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
  return (
    <iframe
      ref={frameRef}
      className="plugin-frame"
      title={`${instance.plugin_name} ${surface}`}
      src={selectedSurface.entry_url}
      sandbox="allow-scripts allow-same-origin"
      referrerPolicy="same-origin"
    />
  );
}

function SettingsPage() {
  const [config, setConfig] = useState<WebPublicConfig | null>(null);
  useEffect(() => {
    fetch("/api/v1/config")
      .then((response) => response.json())
      .then((value: WebPublicConfig) => setConfig(value))
      .catch(() => setConfig(null));
  }, []);

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
              The interface is bound to this Raspberry Pi only while secure
              pairing is being configured.
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
        <article className="settings-card pairing-card">
          <div className="settings-icon">••</div>
          <div className="settings-copy">
            <span className="card-kicker">Security</span>
            <h2>Pair a device</h2>
            <p>
              A one-time code shown on the controller will authorize a phone,
              tablet or computer on your network.
            </p>
          </div>
          <button className="secondary-button" disabled>
            Available after controller handshake
          </button>
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
