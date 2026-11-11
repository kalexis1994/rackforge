import { useEffect, useRef, useState } from "react";
import { useParams } from "react-router";
import { PageHeading } from "../components/PageHeading";
import { PluginSurfaceState } from "../components/PluginSurfaceState";
import { hostJson } from "../host";

export type ControllerSettingSummary = {
  id: string;
  name: string;
  kind: string;
  default: string;
  page: string | null;
  value: string;
};

export type ControllerSummary = {
  id: string;
  name: string;
  version: string;
  enabled: boolean;
  trust: string;
  runtime: string;
  devices: number;
  settings: ControllerSettingSummary[];
};

export function ControllerPage() {
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
