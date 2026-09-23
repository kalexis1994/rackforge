import { useCallback, useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";
import { AsyncActionLabel } from "../../components/AsyncSpinner";
import { PageHeading } from "../../components/PageHeading";
import { VelocityCurveReading } from "../../components/VelocityCurveReading";
import { openAudioDriverPanel } from "../../gateway";
import { HostRequestError, IS_BROWSER_HOST, hostJson, isNativeHost } from "../../host";
import { ChangePinCard } from "../../pages/settings/ChangePinCard";
import { ScreenGlassCard } from "../../pages/settings/ScreenGlassCard";
import { TypingKeyboardCard } from "../../pages/settings/TypingKeyboardCard";
import { HostSettingsBootstrap } from "../../pages/settings/hostSettings";
import { SettingsTab, isSettingsTab, settingsTabsFor } from "../../pages/settings/tabs";
import { type HostAudioPreferences, type HostAudioSettings, type WebPublicConfig } from "../../types";

export function SettingsPage({
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
  // A host that does not serve the endpoint at all is not a host whose
  // request failed, and the two need different words on screen.
  const [audioUnsupported, setAudioUnsupported] = useState(false);
  const [audioDraft, setAudioDraft] = useState<HostAudioPreferences | null>(initial.audioSettings?.preferences ?? null);
  const [audioOperation, setAudioOperation] = useState<"refresh" | "test" | "save" | "panel" | null>(null);
  const audioBusy = audioOperation !== null;
  const [audioMessage, setAudioMessage] = useState<string | null>(null);
  // The tab lives in the URL so a section stays linkable. `replace` keeps
  // tab-hopping out of the back button.
  const [searchParams, setSearchParams] = useSearchParams();
  const serverless = IS_BROWSER_HOST || isNativeHost();
  const tabs = settingsTabsFor(serverless);
  // `security` was its own section until the PIN moved in beside the server it
  // protects. A link someone kept still lands where the passcode now lives.
  const requestedTab = searchParams.get("tab") === "security"
    ? "network"
    : searchParams.get("tab");
  const settingsTab: SettingsTab = isSettingsTab(requestedTab, serverless)
    ? requestedTab
    : "audio";
  const setSettingsTab = (tab: SettingsTab) => {
    setSearchParams(tab === "audio" ? {} : { tab }, { replace: true });
  };
  // "Refresh devices" is the one read that may scan the audio hardware while
  // a stream plays: the desktop host otherwise answers from what it last
  // scanned, because opening every endpoint of every backend under a running
  // stream held notes back while the player played. Only the desktop reads
  // the flag, and the other hosts match this path exactly, so it goes to the
  // desktop alone.
  const hostScansOnRequest = audioSettings?.host === "desktop";
  const loadAudioSettings = useCallback(async () => {
    setAudioOperation("refresh");
    setAudioMessage(null);
    try {
      const settings = await hostJson<HostAudioSettings>(
        hostScansOnRequest ? "/api/v1/host/audio?refresh=true" : "/api/v1/host/audio",
      );
      setAudioSettings(settings);
      setAudioDraft(settings.preferences);
      onAudioChange(settings);
    } catch (error) {
      if (error instanceof HostRequestError && error.status === 404) {
        setAudioUnsupported(true);
        setAudioMessage(null);
      } else {
        setAudioMessage(error instanceof Error ? error.message : "Device refresh failed.");
      }
    } finally {
      setAudioOperation(null);
    }
  }, [onAudioChange, hostScansOnRequest]);

  // The readings below the form -- health, actual rate, audio load, buffer
  // underruns -- are LIVE, and the snapshot behind them was not: the whole
  // settings bootstrap is requested once when the app mounts and memoised for
  // the life of the page. The browser host boots its engine only after someone
  // has touched the page, so at that moment there is no engine and never can
  // be, and the panel then showed "LOST" over a running instrument for as long
  // as the tab stayed open. It cost an evening of hunting a dead audio engine
  // that was playing the whole time.
  //
  // Only the readings are refreshed here. `loadAudioSettings` also replaces the
  // draft, which would throw away edits someone had not applied yet.
  const refreshAudioReadings = useCallback(async () => {
    try {
      const settings = await hostJson<HostAudioSettings>("/api/v1/host/audio");
      setAudioSettings(settings);
      onAudioChange(settings);
    } catch {
      // A reading that fails to arrive leaves the last one standing: this is
      // a meter, and a meter that erases itself on a hiccup is worse than one
      // that is a couple of seconds old.
    }
  }, [onAudioChange]);

  useEffect(() => {
    if (settingsTab !== "audio") return;
    // Both reads are callbacks, not the effect body: a reading is news from
    // outside React, and setting state straight from an effect cascades a
    // render. The first one lands on the next tick rather than two seconds in.
    const first = window.setTimeout(() => void refreshAudioReadings(), 0);
    const timer = window.setInterval(() => void refreshAudioReadings(), 2_000);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [settingsTab, refreshAudioReadings]);

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
  // A reading is heard while it is being drawn: a curve you cannot play is a
  // curve you cannot judge. The host takes it without writing it down and
  // puts back what is applied when this screen stops asking -- when the tab
  // is left, when Reset is pressed, or when the screen simply goes away.
  const auditionedRef = useRef(false);
  useEffect(() => {
    const restore = () => {
      if (!auditionedRef.current) return;
      auditionedRef.current = false;
      void hostJson("/api/v1/host/midi/velocity-preview", { method: "DELETE" }).catch(
        () => undefined,
      );
    };
    if (settingsTab !== "midi" || !audioDraft || !audioSettings) {
      restore();
      return;
    }
    const applied = audioSettings.preferences;
    const same =
      JSON.stringify(audioDraft.velocity_curve ?? null) ===
        JSON.stringify(applied.velocity_curve ?? null) &&
      JSON.stringify(audioDraft.velocity_curves ?? {}) ===
        JSON.stringify(applied.velocity_curves ?? {});
    if (same) {
      restore();
      return;
    }
    // Debounced: a drag is a hundred readings, and the engine only needs the
    // one the hand came to rest on.
    const timer = window.setTimeout(() => {
      auditionedRef.current = true;
      void hostJson("/api/v1/host/midi/velocity-preview", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          velocity_curve: audioDraft.velocity_curve,
          velocity_curves: audioDraft.velocity_curves ?? {},
        }),
      }).catch(() => undefined);
    }, 120);
    return () => window.clearTimeout(timer);
  }, [settingsTab, audioDraft, audioSettings]);

  // And when this screen is closed altogether.
  useEffect(
    () => () => {
      if (!auditionedRef.current) return;
      auditionedRef.current = false;
      void hostJson("/api/v1/host/midi/velocity-preview", { method: "DELETE" }).catch(
        () => undefined,
      );
    },
    [],
  );

  const resetAudioSettings = () => {
    if (!audioSettings || !audioDraft) return;
    const applied = audioSettings.preferences;
    setAudioMessage(null);
    setAudioDraft(
      settingsTab === "midi"
        ? {
            ...audioDraft,
            midi_inputs: applied.midi_inputs,
            velocity_curve: applied.velocity_curve,
            velocity_curves: applied.velocity_curves,
          }
        : {
            ...applied,
            midi_inputs: audioDraft.midi_inputs,
            velocity_curve: audioDraft.velocity_curve,
            velocity_curves: audioDraft.velocity_curves,
          },
    );
  };

  const saveAudioSettings = async () => {
    if (!audioDraft || !audioSettings) return;
    setAudioOperation("save");
    setAudioMessage(null);
    try {
      // The button applies the tab it is on. Both tabs edit one document --
      // the host takes its audio settings whole -- but a player on the MIDI
      // tab who presses Apply means the ports and the readings, not a driver
      // they changed their mind about next door. So the payload is what the
      // host already has, with only this tab's fields laid over it, and the
      // other tab's edits stay pending for its own button.
      const applied = audioSettings.preferences;
      const payload =
        settingsTab === "midi"
          ? {
              ...applied,
              midi_inputs: audioDraft.midi_inputs,
              velocity_curve: audioDraft.velocity_curve,
              velocity_curves: audioDraft.velocity_curves,
            }
          : {
              ...audioDraft,
              midi_inputs: applied.midi_inputs,
              velocity_curve: applied.velocity_curve,
              velocity_curves: applied.velocity_curves,
            };
      const settings = await hostJson<HostAudioSettings>("/api/v1/host/audio", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      setAudioSettings(settings);
      // What this tab committed comes back from the host; what the other tab
      // has pending is left where the player put it.
      setAudioDraft((current) =>
        current === null
          ? settings.preferences
          : settingsTab === "midi"
            ? {
                ...current,
                midi_inputs: settings.preferences.midi_inputs,
                velocity_curve: settings.preferences.velocity_curve,
                velocity_curves: settings.preferences.velocity_curves,
              }
            : { ...settings.preferences, midi_inputs: current.midi_inputs, velocity_curve: current.velocity_curve, velocity_curves: current.velocity_curves },
      );
      onAudioChange(settings);
      setAudioMessage(settingsTab === "midi" ? "MIDI settings applied." : "Audio settings applied.");
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "Audio settings failed.");
    } finally {
      setAudioOperation(null);
    }
  };
  // Whether this tab has anything to apply or to reset.
  const audioTabDirty = (() => {
    if (!audioDraft || !audioSettings) return false;
    const applied = audioSettings.preferences;
    const midiPart = (settings: HostAudioPreferences) =>
      JSON.stringify([
        settings.midi_inputs,
        settings.velocity_curve ?? null,
        settings.velocity_curves ?? {},
      ]);
    if (settingsTab === "midi") return midiPart(audioDraft) !== midiPart(applied);
    const audioPart = (settings: HostAudioPreferences) =>
      JSON.stringify({ ...settings, midi_inputs: [], velocity_curve: null, velocity_curves: {} });
    return audioPart(audioDraft) !== audioPart(applied);
  })();

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

  // The window belongs to the driver that is applied, not to the one picked
  // in an unsaved draft: until a new choice is applied, the button would
  // open the settings of the driver being left.
  const driverPanel = audioSettings?.driver_panel ?? null;
  const driverPanelBlocked = !driverPanel
    ? null
    : !driverPanel.available
      ? driverPanel.detail ?? "Not available right now."
      : audioDraft && audioSettings && audioDraft.driver !== audioSettings.preferences.driver
        ? "Apply the driver change first: this opens the settings of the driver in use."
        : null;

  const openDriverPanel = async () => {
    if (!driverPanel) return;
    setAudioOperation("panel");
    setAudioMessage(null);
    try {
      await openAudioDriverPanel();
      setAudioMessage(
        driverPanel.kind === "asio"
          ? "Opening the driver's settings on this computer. A change there reopens the audio stream; refresh the devices afterwards."
          : "Opening the Windows sound settings.",
      );
    } catch (error) {
      setAudioMessage(error instanceof Error ? error.message : "The driver settings could not open.");
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
        {/* Audio and MIDI are two sections of one card because the host keeps
            one audio document, but each tab's Apply commits its own half of it
            and leaves the other half where the player left it. */}
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
              /* Two plain fieldsets: the form's own grid already fills as
                 many 280px columns as fit, so the ports land on the left and
                 the curve beside them where there is room and underneath
                 where there is not. A wrapper here would have been one item in
                 that grid, and the two would have stacked forever. */
              <>
              <fieldset className="midi-ports">
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
              <fieldset className="velocity-curve-fieldset">
                <legend>Velocity</legend>
                {/* Across is what the keyboard sent, up is what is played. A
                    reading belongs to the keybed it corrects, so the square
                    edits one port at a time and every other device falls back
                    to the shared one. It rides the same draft as the ports, so
                    one Apply commits both -- and the host applies a reading
                    without touching the audio stream. */}
                <VelocityCurveReading
                  draft={audioDraft}
                  ports={audioSettings.inventory.midi_inputs}
                  sourceKeys={audioSettings.midi_source_keys ?? {}}
                  live={settingsTab === "midi"}
                  onChange={setAudioDraft}
                />
              </fieldset>
              </>
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
                {driverPanel ? (
                  <button
                    className="secondary-button"
                    disabled={audioBusy || driverPanelBlocked !== null}
                    title={driverPanelBlocked ?? undefined}
                    onClick={() => void openDriverPanel()}
                  >
                    <AsyncActionLabel active={audioOperation === "panel"} activeLabel="Opening…">
                      {driverPanel.kind === "asio" ? "Driver settings" : "Windows sound settings"}
                    </AsyncActionLabel>
                  </button>
                ) : null}
                {/* Back to what is applied, for this tab. It is the way out of
                    an audition as much as an undo: the reading the engine is
                    hearing goes back with the draft. */}
                <button
                  className="secondary-button"
                  disabled={audioBusy || !audioTabDirty}
                  onClick={resetAudioSettings}
                >
                  Reset
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
              <h2>
                {settingsTab === "midi" ? "MIDI" : "Audio"}{" "}
                {audioUnsupported ? "configured on the device" : "unavailable"}
              </h2>
              <p>
                {audioUnsupported
                  ? "This RackForge host does not choose audio and MIDI devices over the network. Activating an instrument writes config/audio.toml from the example beside it, and the engine starts from that file."
                  : "The current host did not publish its audio and MIDI settings."}
              </p>
            </div>
            <div className="host-audio-actions" hidden={audioUnsupported}>
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
