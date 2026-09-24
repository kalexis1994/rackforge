import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSelector } from "react-redux";
import { Link, useSearchParams } from "react-router";
import { AsyncNotice } from "../components/AsyncStateBoundary";
import { PageHeading } from "../components/PageHeading";
import { RfLoader } from "../components/RfLoader";
import { ControllerInputList } from "../components/controllers/ControllerInputList";
import { InputAssignments } from "../components/controllers/InputAssignments";
import {
  buildControllerDevices,
  type ControllerDevice,
  type ControllerPackageSummary,
  emptyControllerMap,
  inputForActivity,
  inputFromActivity,
  inputMessageLabel,
  withMapping,
  withoutMapping,
} from "../controllerMapping";
import {
  exportControllerMap,
  importControllerMap,
  requestControllerMaps,
  saveControllerMap,
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
import type { ControllerMap, RegisteredController, RfMapFile } from "../types";

/** How long a control stays lit after its last message. */
const LIT_MS = 450;
/** How often the list of attached controllers is asked for again. */
const REFRESH_MS = 5000;
const PHONE = "(max-width: 760px), (orientation: landscape) and (max-height: 600px) and (max-width: 1200px)";
const MAX_RFMAP_BYTES = 4 * 1024 * 1024;

interface Loaded {
  packages: ControllerPackageSummary[];
  registered: RegisteredController[];
  maps: ControllerMap[];
}

/**
 * The Controllers section: every controller RackForge knows, its controls,
 * and what the player made each one do in each plugin. Moving a control
 * lights it here, so the player finds the one in their hand without
 * reading a manual.
 */
export function ControllersPage() {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ tone: "success" | "error" | "info"; text: string } | null>(null);
  const [busy, setBusy] = useState<"export" | "import" | null>(null);
  const [searchParams, setSearchParams] = useSearchParams();
  const [selectedInputId, setSelectedInputId] = useState<string | null>(null);
  const [lit, setLit] = useState<ReadonlySet<string>>(new Set());
  const [stray, setStray] = useState<string | null>(null);
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

  // The packages rarely change and are asked for until they have been read
  // once; the attachments and maps are asked for again all the time.
  const packagesRead = useRef(false);
  const load = useCallback(async () => {
    const [maps, packages] = await Promise.all([
      requestControllerMaps(),
      packagesRead.current
        ? Promise.resolve(null)
        : hostJson<{ controllers?: ControllerPackageSummary[] }>("/api/v1/controllers")
          .then((response) => response.controllers ?? [])
          .catch(() => null),
    ]);
    if (packages) packagesRead.current = true;
    setLoaded((current) => ({
      packages: packages ?? current?.packages ?? [],
      registered: maps.controllers,
      maps: maps.maps,
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

  const devices = useMemo(
    () => (loaded ? buildControllerDevices(loaded.packages, loaded.registered, loaded.maps) : []),
    [loaded],
  );
  const requestedId = searchParams.get("device");
  const device: ControllerDevice | undefined =
    devices.find((candidate) => candidate.id === requestedId) ?? devices[0];
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

  // What the device's input sends lights its controls. Messages from other
  // inputs are someone else's; a message this package names no control for
  // is shown as such, so a knob in another bank is not a mystery.
  const deviceRef = useRef(device);
  const selectedRef = useRef(selectedInputId);
  useEffect(() => {
    deviceRef.current = device;
    selectedRef.current = selectedInputId;
  }, [device, selectedInputId]);
  useEffect(() => {
    const unsubscribe = subscribeMidiActivity((events) => {
      const current = deviceRef.current;
      if (!current?.source) return;
      const now = performance.now();
      let changed = false;
      for (const event of events) {
        if (event.source.id !== current.source.id) continue;
        const input = inputForActivity(event, current.inputs);
        if (input) {
          litUntil.current.set(input.id, now + LIT_MS);
          changed = true;
          setStray(null);
          // The first control moved is the one shown, until one is chosen.
          if (selectedRef.current === null) setSelectedInputId(input.id);
        } else {
          const unknown = inputFromActivity(event);
          if (unknown) setStray(inputMessageLabel(unknown));
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

  const keepsMaps = !IS_BROWSER_HOST && !isNativeHost();
  const showDetail = Boolean(selectedInput) && phone;

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
            RackForge lists it here as soon as a controller package recognises it.
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
              <select value={device.id} onChange={(event) => selectDevice(event.target.value)}>
                {devices.map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>
                    {candidate.name}
                    {candidate.connected ? "" : candidate.orphaned ? " · package removed" : " · not connected"}
                  </option>
                ))}
              </select>
            </label>
            <span className={`controllers-status${device.connected ? " connected" : ""}`}>
              <i aria-hidden="true" />
              {device.connected
                ? `Listening on ${device.source?.name ?? "its input"}`
                : device.orphaned
                  ? "Its package is no longer installed"
                  : "Not connected: mappings apply when it returns"}
            </span>
            <div className="controllers-actions">
              {device.package ? (
                <Link className="secondary-button" to={`/controllers/${encodeURIComponent(device.id)}`}>
                  Package
                </Link>
              ) : null}
              {keepsMaps ? (
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

          {stray && device.connected ? (
            <p className="controllers-stray" role="status">
              {stray} is not one of the controls this package names.
            </p>
          ) : null}

          {device.inputs.length === 0 ? (
            <section className="settings-card controller-empty">
              <h2>{device.name} names no control</h2>
              <p>Its package lists no inputs to assign. A newer version of the package may; ask its author.</p>
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
              {selectedInput ? (
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
                  <h2>Move a control</h2>
                  <p>
                    {device.connected
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
