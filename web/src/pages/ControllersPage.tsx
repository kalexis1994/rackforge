import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSelector } from "react-redux";
import { Link, useSearchParams } from "react-router";
import { AsyncNotice } from "../components/AsyncStateBoundary";
import { PageHeading } from "../components/PageHeading";
import { RfLoader } from "../components/RfLoader";
import { ControllerInputList } from "../components/controllers/ControllerInputList";
import { InputEditor, UserControllerForm } from "../components/controllers/ControlsEditor";
import { InputAssignments } from "../components/controllers/InputAssignments";
import {
  buildControllerDevices,
  type ControllerDevice,
  type ControllerInput,
  type ControllerPackageSummary,
  emptyControllerMap,
  inputForActivity,
  inputFromActivity,
  inputMessageLabel,
  isUserController,
  unknownSourceDevices,
  withLearntInput,
  withMapping,
  withoutMapping,
} from "../controllerMapping";
import {
  exportControllerMap,
  importControllerMap,
  requestControllerMaps,
  requestMidiSources,
  saveControllerMap,
  saveUserController,
  subscribeMidiActivity,
} from "../gateway";
import { useMediaQuery } from "../hooks/useMediaQuery";
import {
  hostJson,
  IS_BROWSER_HOST,
  isDesktopHost,
  isNativeHost,
  readNativeTextFile,
  savePortableTextFile,
} from "../host";
import { usePluginCatalog } from "../pluginCatalog";
import type { RootState } from "../store";
import type { ControllerMap, MidiSourceStatus, RegisteredController, RfMapFile } from "../types";

/** How long a control stays lit after its last message. */
const LIT_MS = 450;
/** How often the list of attached controllers is asked for again. */
const REFRESH_MS = 5000;
const PHONE = "(max-width: 760px), (orientation: landscape) and (max-height: 600px) and (max-width: 1200px)";
const MAX_RFMAP_BYTES = 4 * 1024 * 1024;
/** Controls learnt from unknown inputs, kept for the browser session. */
const LEARNT_KEY = "rackforge.controllers.learnt";

interface Loaded {
  packages: ControllerPackageSummary[];
  registered: RegisteredController[];
  maps: ControllerMap[];
  sources: MidiSourceStatus[];
}

type Notice = { tone: "success" | "error" | "info"; text: string };

function readLearnt(): Map<string, ControllerInput[]> {
  try {
    const stored = window.sessionStorage.getItem(LEARNT_KEY);
    const entries = stored ? (JSON.parse(stored) as Array<[string, ControllerInput[]]>) : [];
    return new Map(Array.isArray(entries) ? entries : []);
  } catch {
    return new Map();
  }
}

function writeLearnt(learnt: Map<string, ControllerInput[]>) {
  try {
    window.sessionStorage.setItem(LEARNT_KEY, JSON.stringify([...learnt.entries()]));
  } catch {
    // Private windows and full storage keep nothing; the controls stay on
    // screen for as long as the page does.
  }
}

/**
 * The Controllers section: every controller RackForge knows, its controls,
 * and what the player made each one do in each plugin. Moving a control
 * lights it here, so the player finds the one in their hand without
 * reading a manual. A keyboard RackForge has no package for is described
 * here too: its controls are learnt as they move, named, and saved as a
 * controller of the player's own.
 */
export function ControllersPage() {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [busy, setBusy] = useState<"export" | "import" | "controls" | null>(null);
  const [searchParams, setSearchParams] = useSearchParams();
  const [selectedInputId, setSelectedInputId] = useState<string | null>(null);
  const [lit, setLit] = useState<ReadonlySet<string>>(new Set());
  const [stray, setStray] = useState<string | null>(null);
  const [learnt, setLearnt] = useState<Map<string, ControllerInput[]>>(readLearnt);
  // A player's own controller whose controls are being changed.
  const [controlsDraft, setControlsDraft] = useState<{ deviceId: string; inputs: ControllerInput[] } | null>(null);
  const litUntil = useRef(new Map<string, number>());
  const fileInput = useRef<HTMLInputElement | null>(null);
  const phone = useMediaQuery(PHONE);
  const catalog = usePluginCatalog();
  const snapshot = useSelector((state: RootState) => state.rackforge.snapshot);
  const connection = useSelector((state: RootState) => state.rackforge.connection);
  const playingPluginId = snapshot?.instances.find(
    (instance) => instance.instance_id === snapshot.active_instance_id,
  )?.plugin_id;
  const plugins = useMemo(
    () => catalog.plugins.filter((plugin) => plugin.kind === "instrument" || plugin.kind === "effect"),
    [catalog.plugins],
  );
  const keepsMaps = !IS_BROWSER_HOST && !isNativeHost();

  useEffect(() => writeLearnt(learnt), [learnt]);

  // The packages rarely change and are asked for until they have been read
  // once; the attachments, maps and inputs are asked for again all the time.
  const packagesRead = useRef(false);
  const load = useCallback(async () => {
    const [maps, packages, sources] = await Promise.all([
      requestControllerMaps(),
      packagesRead.current
        ? Promise.resolve(null)
        : hostJson<{ controllers?: ControllerPackageSummary[] }>("/api/v1/controllers")
          .then((response) => response.controllers ?? [])
          .catch(() => null),
      requestMidiSources().catch(() => null),
    ]);
    if (packages) packagesRead.current = true;
    setLoaded((current) => ({
      packages: packages ?? current?.packages ?? [],
      registered: maps.controllers,
      maps: maps.maps,
      sources: sources ?? current?.sources ?? [],
    }));
    setLoadError(null);
  }, []);

  // Asked once the session is up, and again after every reconnection:
  // controllers come and go as they are plugged in, and the host attaches
  // them on its own.
  useEffect(() => {
    if (connection !== "online") return;
    let active = true;
    const run = () =>
      load().catch((reason: unknown) => {
        if (active) setLoadError(reason instanceof Error ? reason.message : "Could not read the controllers.");
      });
    void run();
    const timer = window.setInterval(() => void run(), REFRESH_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [connection, load]);

  const devices = useMemo(() => {
    if (!loaded) return [];
    const known = buildControllerDevices(loaded.packages, loaded.registered, loaded.maps);
    if (!keepsMaps) return known;
    const claimed = new Set(
      loaded.registered.flatMap((entry) => (entry.source ? [entry.source.id] : [])),
    );
    // Hide the host's own touch keyboard: it is not a controller to map.
    const sources = loaded.sources.filter((entry) => !entry.source.id.startsWith("rackforge."));
    return [...known, ...unknownSourceDevices(sources, claimed, learnt)];
  }, [keepsMaps, learnt, loaded]);
  const requestedId = searchParams.get("device");
  const chosen: ControllerDevice | undefined =
    devices.find((candidate) => candidate.id === requestedId) ?? devices[0];
  const editingControls = Boolean(chosen?.unknown) || (chosen !== undefined && controlsDraft?.deviceId === chosen.id);
  const device: ControllerDevice | undefined = useMemo(
    () => (chosen && controlsDraft?.deviceId === chosen.id ? { ...chosen, inputs: controlsDraft.inputs } : chosen),
    [chosen, controlsDraft],
  );
  const selectedInput = device?.inputs.find((input) => input.id === selectedInputId);

  const selectDevice = (id: string) => {
    setSelectedInputId(null);
    setStray(null);
    setSearchParams((params) => {
      const next = new URLSearchParams(params);
      next.set("device", id);
      return next;
    }, { replace: true });
  };

  // What the device's input sends lights its controls. While controls are
  // being described, a message no control answers becomes a new one;
  // otherwise it is shown as such, so a knob in another bank is not a
  // mystery.
  const deviceRef = useRef(device);
  const selectedRef = useRef(selectedInputId);
  const learningRef = useRef(editingControls);
  useEffect(() => {
    deviceRef.current = device;
    selectedRef.current = selectedInputId;
    learningRef.current = editingControls;
  }, [device, editingControls, selectedInputId]);
  useEffect(() => {
    const unsubscribe = subscribeMidiActivity((events) => {
      const current = deviceRef.current;
      if (!current?.source) return;
      const sourceId = current.source.id;
      const now = performance.now();
      let changed = false;
      let heard: ControllerInput[] = [];
      for (const event of events) {
        if (event.source.id !== sourceId) continue;
        const input = inputForActivity(event, [...current.inputs, ...heard]);
        if (input) {
          litUntil.current.set(input.id, now + LIT_MS);
          changed = true;
          setStray(null);
          if (selectedRef.current === null) setSelectedInputId(input.id);
          continue;
        }
        const unknown = inputFromActivity(event);
        if (!unknown) continue;
        if (learningRef.current) {
          heard = withLearntInput(heard, unknown);
          litUntil.current.set(unknown.id, now + LIT_MS);
          changed = true;
          if (selectedRef.current === null) setSelectedInputId(unknown.id);
        } else {
          setStray(inputMessageLabel(unknown));
        }
      }
      if (heard.length > 0) {
        const add = (inputs: ControllerInput[]) =>
          heard.reduce((list, input) => withLearntInput(list, input), inputs);
        if (current.unknown) {
          setLearnt((previous) => new Map(previous).set(sourceId, add(previous.get(sourceId) ?? [])));
        } else {
          setControlsDraft((previous) =>
            previous && previous.deviceId === current.id ? { ...previous, inputs: add(previous.inputs) } : previous,
          );
        }
      }
      if (changed) setLit(new Set(litUntil.current.keys()));
    });
    const sweep = window.setInterval(() => {
      const now = performance.now();
      let changed = false;
      for (const [id, until] of litUntil.current) {
        if (until <= now) {
          litUntil.current.delete(id);
          changed = true;
        }
      }
      if (changed) setLit(new Set(litUntil.current.keys()));
    }, 100);
    return () => {
      unsubscribe();
      window.clearInterval(sweep);
    };
  }, []);

  const replaceMap = (map: ControllerMap) =>
    setLoaded((current) =>
      current
        ? {
            ...current,
            maps: [
              ...current.maps.filter((candidate) => candidate.controller_id !== map.controller_id),
              ...(map.plugins.length > 0 ? [map] : []),
            ],
          }
        : current,
    );

  const commit = async (next: ControllerMap) => {
    const saved = await saveControllerMap(next);
    replaceMap(saved ?? next);
  };

  /** The controls being described, changed: learnt ones or a draft's. */
  const changeControls = (change: (inputs: ControllerInput[]) => ControllerInput[]) => {
    if (!device) return;
    if (device.unknown && device.source) {
      const sourceId = device.source.id;
      setLearnt((previous) => new Map(previous).set(sourceId, change(previous.get(sourceId) ?? [])));
    } else {
      setControlsDraft((previous) =>
        previous && previous.deviceId === device.id ? { ...previous, inputs: change(previous.inputs) } : previous,
      );
    }
  };

  const saveControls = async (name: string, vendor: string) => {
    if (!device?.source) return;
    setBusy("controls");
    setNotice(null);
    try {
      const saved = await saveUserController({
        ...(device.unknown ? {} : { controller_id: device.id }),
        name,
        ...(vendor ? { vendor } : {}),
        endpoint_name: device.source.name,
        inputs: device.inputs,
      });
      if (device.unknown) {
        const sourceId = device.source.id;
        setLearnt((previous) => {
          const next = new Map(previous);
          next.delete(sourceId);
          return next;
        });
      }
      setControlsDraft(null);
      setSelectedInputId(null);
      packagesRead.current = false;
      await load().catch(() => undefined);
      selectDevice(saved.controller_id);
      setNotice({
        tone: "success",
        text: `${name} saved. RackForge attaches it to ${device.source.name} in a moment.`,
      });
    } catch (reason) {
      setNotice({ tone: "error", text: reason instanceof Error ? reason.message : "Could not save the controller." });
    } finally {
      setBusy(null);
    }
  };

  const exportMap = async () => {
    if (!device?.map) return;
    setBusy("export");
    setNotice(null);
    try {
      const { file_name, file } = await exportControllerMap(device.id);
      await savePortableTextFile({
        file_name,
        mime_type: "application/vnd.rackforge.map+json",
        text: JSON.stringify(file, null, 2),
      });
      setNotice({ tone: "success", text: `${file_name} saved.` });
    } catch (reason) {
      setNotice({ tone: "error", text: reason instanceof Error ? reason.message : "Could not export the map." });
    } finally {
      setBusy(null);
    }
  };

  const importText = async (fileName: string, text: string) => {
    setBusy("import");
    setNotice(null);
    try {
      if (!fileName.toLowerCase().endsWith(".rfmap")) throw new Error("Choose an .rfmap file.");
      if (!text || new TextEncoder().encode(text).byteLength > MAX_RFMAP_BYTES) {
        throw new Error("The map file is empty or larger than 4 MiB.");
      }
      const file = JSON.parse(text) as RfMapFile;
      if (file?.format !== "org.rackforge.map" || !file.map?.controller_id) {
        throw new Error("This is not a RackForge controller map.");
      }
      const map = await importControllerMap(file);
      replaceMap(map);
      selectDevice(map.controller_id);
      setNotice({ tone: "success", text: `Map for ${map.controller_name} imported.` });
    } catch (reason) {
      setNotice({
        tone: "error",
        text: reason instanceof SyntaxError
          ? "The map file is not valid JSON."
          : reason instanceof Error ? reason.message : "Could not import the map.",
      });
    } finally {
      setBusy(null);
    }
  };

  const chooseImport = () => {
    if (isNativeHost() || isDesktopHost()) {
      setBusy("import");
      readNativeTextFile({ extensions: ["rfmap"], maximum_bytes: MAX_RFMAP_BYTES })
        .then(({ file_name, text }) => importText(file_name, text))
        .catch((reason: Error) => {
          setNotice({ tone: "error", text: reason.message });
          setBusy(null);
        });
      return;
    }
    fileInput.current?.click();
  };

  const heading = (
    <PageHeading
      title="Controllers"
      detail="Move a control to find it, then choose what it does in each plugin."
    />
  );

  if (!loaded) {
    return (
      <>
        {heading}
        {loadError ? (
          <AsyncNotice tone="error" title="Could not read the controllers">
            {loadError} RackForge tries again on its own.
          </AsyncNotice>
        ) : (
          <RfLoader
            label="Controllers"
            detail={connection === "online" ? "Reading the controllers and their maps…" : "Waiting for RackForge…"}
            size="medium"
          />
        )}
      </>
    );
  }

  const showDetail = Boolean(selectedInput) && phone;
  const groups = [...new Set((device?.inputs ?? []).flatMap((input) => (input.group ? [input.group] : [])))];
  const canEditControls = Boolean(
    keepsMaps && device && !device.unknown && isUserController(device.id) && device.package && device.source,
  );

  return (
    <>
      {heading}
      {!keepsMaps || notice ? (
        <div className="controllers-notices">
          {keepsMaps ? null : (
            <AsyncNotice tone="info" title="No maps kept here">
              This device shows a controller's controls. RackForge on a computer or a Raspberry Pi keeps
              what you map them to.
            </AsyncNotice>
          )}
          {notice ? (
            <AsyncNotice tone={notice.tone} title={notice.text} onDismiss={() => setNotice(null)} />
          ) : null}
        </div>
      ) : null}

      {devices.length === 0 ? (
        <section className="settings-card controller-empty">
          <h2>No controller yet</h2>
          <p>
            Plug in a MIDI controller and enable it in <Link to="/settings">Settings · Audio &amp; MIDI</Link>.
            It appears here as soon as RackForge hears it.
          </p>
          {keepsMaps ? (
            <button type="button" className="secondary-button" disabled={busy !== null} onClick={chooseImport}>
              Import a map…
            </button>
          ) : null}
        </section>
      ) : device ? (
        <section className="controllers-workspace">
          <header className="controllers-toolbar">
            <label className="controllers-device">
              <span>Controller</span>
              <select value={device.id} disabled={busy === "controls"} onChange={(event) => selectDevice(event.target.value)}>
                {devices.map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>
                    {candidate.name}
                    {candidate.unknown
                      ? " · new"
                      : candidate.connected ? "" : candidate.orphaned ? " · package removed" : " · not connected"}
                  </option>
                ))}
              </select>
            </label>
            <span className={`controllers-status${device.connected ? " connected" : ""}`}>
              <i aria-hidden="true" />
              {device.unknown
                ? device.connected ? "No package knows this controller yet" : "This input is not connected"
                : device.connected
                  ? `Listening on ${device.source?.name ?? "its input"}`
                  : device.orphaned
                    ? "Its package is no longer installed"
                    : "Not connected: mappings apply when it returns"}
            </span>
            <div className="controllers-actions">
              {canEditControls && !editingControls ? (
                <button
                  type="button"
                  className="secondary-button"
                  disabled={busy !== null}
                  onClick={() => {
                    setSelectedInputId(null);
                    setControlsDraft({ deviceId: device.id, inputs: device.inputs });
                  }}
                >
                  Edit controls
                </button>
              ) : null}
              {device.package && !editingControls ? (
                <Link className="secondary-button" to={`/controllers/${encodeURIComponent(device.id)}`}>
                  Package
                </Link>
              ) : null}
              {keepsMaps && !editingControls ? (
                <>
                  <button type="button" className="secondary-button" disabled={busy !== null} onClick={chooseImport}>
                    {busy === "import" ? "Importing…" : "Import…"}
                  </button>
                  <button
                    type="button"
                    className="secondary-button"
                    disabled={busy !== null || !device.map}
                    onClick={() => void exportMap()}
                  >
                    {busy === "export" ? "Exporting…" : "Export"}
                  </button>
                </>
              ) : null}
            </div>
          </header>

          {editingControls ? (
            <section className="controllers-describe">
              <div className="controllers-describe-copy">
                <h2>{device.unknown ? "Tell RackForge about this controller" : "Change its controls"}</h2>
                <p>
                  Move every knob, fader, button and pad you want to use: each one appears in the list as it
                  moves. Name them, then save. The controller becomes yours, and its controls can then be
                  assigned to any plugin.
                </p>
              </div>
              <UserControllerForm
                key={device.id}
                initialName={device.name}
                initialVendor={device.vendor}
                inputs={device.inputs}
                saving={busy === "controls"}
                existing={!device.unknown}
                onSave={(name, vendor) => void saveControls(name, vendor)}
                onCancel={device.unknown ? undefined : () => {
                  setControlsDraft(null);
                  setSelectedInputId(null);
                }}
              />
            </section>
          ) : null}

          {stray && device.connected && !editingControls ? (
            <p className="controllers-stray" role="status">
              {stray} is not one of the controls this package names.
            </p>
          ) : null}

          {device.inputs.length === 0 ? (
            <section className="settings-card controller-empty">
              {editingControls ? (
                <>
                  <h2>Move a control</h2>
                  <p>
                    {device.connected
                      ? `Nothing heard from ${device.source?.name ?? "this input"} yet. Turn a knob or press a button.`
                      : "This input is not connected. Plug the controller in and enable it in Settings."}
                  </p>
                </>
              ) : (
                <>
                  <h2>{device.name} names no control</h2>
                  <p>Its package lists no inputs to assign. A newer version of the package may; ask its author.</p>
                </>
              )}
            </section>
          ) : (
            <div className={`controllers-panes${showDetail ? " detail" : ""}`}>
              <ControllerInputList
                device={device}
                selectedId={selectedInputId}
                lit={lit}
                playingPluginId={playingPluginId}
                onSelect={setSelectedInputId}
              />
              {selectedInput && editingControls ? (
                <InputEditor
                  key={`${device.id}:${selectedInput.id}`}
                  input={selectedInput}
                  groups={groups}
                  onClose={phone ? () => setSelectedInputId(null) : undefined}
                  onChange={(input) =>
                    changeControls((inputs) => inputs.map((candidate) => (candidate.id === input.id ? input : candidate)))
                  }
                  onRemove={() => {
                    changeControls((inputs) => inputs.filter((candidate) => candidate.id !== selectedInput.id));
                    setSelectedInputId(null);
                  }}
                />
              ) : selectedInput ? (
                <InputAssignments
                  key={`${device.id}:${selectedInput.id}`}
                  device={device}
                  input={selectedInput}
                  plugins={plugins}
                  playingPluginId={playingPluginId}
                  readOnly={!keepsMaps}
                  onClose={phone ? () => setSelectedInputId(null) : undefined}
                  onSaveMapping={async (plugin, mapping) => {
                    if (!keepsMaps) throw new Error("This device does not keep controller maps.");
                    await commit(
                      withMapping(
                        device.map ?? emptyControllerMap(device.id, device.name),
                        { plugin_id: plugin.plugin_id, plugin_name: plugin.plugin_name },
                        mapping,
                      ),
                    );
                  }}
                  onRemoveMapping={async (pluginId, mappingId) => {
                    if (!device.map) return;
                    await commit(withoutMapping(device.map, pluginId, mappingId));
                  }}
                />
              ) : (
                <section className="controller-input-detail controller-input-hint">
                  <h2>{editingControls ? "Choose a control to name it" : "Move a control"}</h2>
                  <p>
                    {editingControls
                      ? "Or move another one to add it to the list."
                      : device.connected
                        ? "Turn a knob, push a fader or press a button on the controller: it lights in the list and opens here."
                        : "Choose a control in the list. Moving one lights it once the controller is connected."}
                  </p>
                </section>
              )}
            </div>
          )}
        </section>
      ) : null}

      <input
        ref={fileInput}
        className="visually-hidden"
        type="file"
        accept=".rfmap,application/json"
        tabIndex={-1}
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (!file) return;
          if (file.size > MAX_RFMAP_BYTES) {
            setNotice({ tone: "error", text: "The map file is larger than 4 MiB." });
            return;
          }
          void file.text().then((text) => importText(file.name, text));
        }}
      />
    </>
  );
}
