import { useEffect, useMemo, useRef, useState } from "react";
import {
  Filemanager,
  WillowDark,
  type IApi,
  type IEntity,
} from "@svar-ui/react-filemanager";
import "@svar-ui/react-filemanager/all.css";

import type {
  PluginResourceRequirement,
  ResourceEntry,
  ResourceGrant,
  ResourceMount,
} from "./types";

interface ResourceExplorerDialogProps {
  pluginId: string;
  resource: PluginResourceRequirement;
  onCancel: () => void;
  onBound: (grant: ResourceGrant) => void;
}

function toFileManagerEntry(entry: ResourceEntry): IEntity {
  return {
    id: entry.id,
    name: entry.name,
    type: entry.kind === "directory" ? "folder" : "file",
    size: entry.size ?? undefined,
    date:
      entry.modified_unix_ms === null
        ? undefined
        : new Date(entry.modified_unix_ms),
    lazy: entry.kind === "directory" && entry.lazy,
  };
}

async function readJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      message?: string;
    } | null;
    throw new Error(body?.message ?? `HTTP ${response.status}`);
  }
  return (await response.json()) as T;
}

export function ResourceExplorerDialog({
  pluginId,
  resource,
  onCancel,
  onBound,
}: ResourceExplorerDialogProps) {
  const apiRef = useRef<IApi | null>(null);
  const entriesRef = useRef(new Map<string, ResourceEntry>());
  const [data, setData] = useState<IEntity[] | null>(null);
  const [selected, setSelected] = useState<ResourceEntry | undefined>();
  const [error, setError] = useState<string | null>(null);
  const [binding, setBinding] = useState(false);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      readJson<ResourceMount[]>("/api/v1/resources/mounts"),
    ])
      .then(async ([mounts]) => {
        const roots = await Promise.all(
          mounts.map((mount) =>
            readJson<ResourceEntry>(
              `/api/v1/resources/mounts/${encodeURIComponent(mount.id)}/root`,
            ),
          ),
        );
        if (cancelled) return;
        entriesRef.current = new Map(roots.map((entry) => [entry.id, entry]));
        setData(roots.map(toFileManagerEntry));
      })
      .catch((reason: unknown) => {
        if (!cancelled) {
          setError(
            reason instanceof Error
              ? reason.message
              : "Could not open RackForge storage.",
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selectionValid =
    selected?.can_read === true && selected.kind === resource.kind;
  const hint = useMemo(() => {
    if (!selected) return `Choose a ${resource.kind}.`;
    if (selectionValid) return selected.name;
    return `This plugin requires a ${resource.kind}.`;
  }, [resource.kind, selected, selectionValid]);

  const requestData = ({ id }: { id: string }) => {
    readJson<ResourceEntry[]>(
      `/api/v1/resources/entries/${encodeURIComponent(id)}`,
    )
      .then((entries) => {
        for (const entry of entries) entriesRef.current.set(entry.id, entry);
        return apiRef.current?.exec("provide-data", {
          id,
          data: entries.map(toFileManagerEntry),
          skipProvider: true,
        });
      })
      .catch((reason: unknown) =>
        setError(
          reason instanceof Error ? reason.message : "Could not read this folder.",
        ),
      );
  };

  const bind = () => {
    if (!selected || !selectionValid || binding) return;
    setBinding(true);
    setError(null);
    readJson<ResourceGrant>("/api/v1/resources/bind", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        plugin_id: pluginId,
        resource_id: resource.id,
        entry_id: selected.id,
      }),
    })
      .then(onBound)
      .catch((reason: unknown) => {
        setBinding(false);
        setError(
          reason instanceof Error ? reason.message : "Could not bind this resource.",
        );
      });
  };

  return (
    <div className="resource-explorer-backdrop" role="presentation">
      <section
        className="resource-explorer-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="resource-explorer-title"
      >
        <header className="resource-explorer-header">
          <div>
            <span className="eyebrow">RACKFORGE STORAGE</span>
            <h2 id="resource-explorer-title">Select {resource.name}</h2>
          </div>
          <button type="button" className="icon-button" onClick={onCancel}>
            Close
          </button>
        </header>
        <div className="resource-explorer-body">
          {data ? (
            <WillowDark>
              <Filemanager
                data={data}
                readonly
                mode="panels"
                preview={false}
                init={(api) => {
                  apiRef.current = api;
                }}
                onRequestData={requestData}
                onSelectFile={({ id }: { id?: string }) =>
                  setSelected(id ? entriesRef.current.get(id) : undefined)
                }
              />
            </WillowDark>
          ) : error ? (
            <div className="resource-explorer-empty">{error}</div>
          ) : (
            <div className="resource-explorer-empty">Reading storage…</div>
          )}
        </div>
        <footer className="resource-explorer-footer">
          <div>
            <span>{hint}</span>
            {error && data ? <strong>{error}</strong> : null}
          </div>
          <div className="resource-explorer-actions">
            <button type="button" className="secondary" onClick={onCancel}>
              Cancel
            </button>
            <button type="button" disabled={!selectionValid || binding} onClick={bind}>
              {binding ? "Selecting…" : `Select ${resource.kind}`}
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
