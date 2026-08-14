import { useEffect, useMemo, useRef, useState } from "react";
import {
  Filemanager,
  WillowDark,
  type IApi,
  type IEntity,
} from "@svar-ui/react-filemanager";
import { FileUp } from "lucide-react";
import "@svar-ui/react-filemanager/all.css";

import { hostJson, isNativeHost } from "./host";
import { AsyncActionLabel, AsyncSpinner } from "./components/AsyncSpinner";
import type {
  PluginResourceRequirement,
  ResourceEntry,
  ResourceGrant,
  ResourceMount,
  ResourceSelection,
} from "./types";

const MAX_CLIENT_RESOURCE_BYTES = 2 * 1024 * 1024 * 1024;

type ResourceExplorerDialogProps =
  | {
      mode?: "bind";
      pluginId: string;
      resource: PluginResourceRequirement;
      onCancel: () => void;
      onBound: (grant: ResourceGrant) => void;
      onSelected?: never;
    }
  | {
      mode: "select";
      selection: { name: string; kind: "file" | "directory" };
      onCancel: () => void;
      onSelected: (entry: ResourceEntry) => void;
      resource?: never;
      pluginId?: never;
      onBound?: never;
    };

function toFileManagerEntry(entry: ResourceEntry, parentId: string): IEntity {
  const id = `${parentId === "/" ? "" : parentId}/${entry.name}`;
  return {
    id,
    parent: parentId,
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

export function ResourceExplorerDialog(props: ResourceExplorerDialogProps) {
  const target = props.mode === "select" ? props.selection : props.resource;
  const { onCancel } = props;
  const apiRef = useRef<IApi | null>(null);
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
  const entriesRef = useRef(new Map<string, ResourceEntry>());
  const [data, setData] = useState<IEntity[] | null>(null);
  const [selected, setSelected] = useState<ResourceEntry | undefined>();
  const [error, setError] = useState<string | null>(null);
  const [operation, setOperation] = useState<"binding" | "uploading" | null>(null);
  const binding = operation !== null;

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      hostJson<ResourceMount[]>("/api/v1/resources/mounts"),
    ])
      .then(async ([mounts]) => {
        const roots = await Promise.all(
          mounts.map((mount) =>
            hostJson<ResourceEntry>(
              `/api/v1/resources/mounts/${encodeURIComponent(mount.id)}/root`,
            ),
          ),
        );
        if (cancelled) return;
        const entities = roots.map((entry) => toFileManagerEntry(entry, "/"));
        entriesRef.current = new Map(
          entities.map((entity, index) => [entity.id, roots[index]]),
        );
        setData(entities);
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
    selected?.can_read === true && selected.kind === target.kind;
  const hint = useMemo(() => {
    if (!selected) return `Choose a ${target.kind}.`;
    if (selectionValid) return selected.name;
    return `This selection requires a ${target.kind}.`;
  }, [target.kind, selected, selectionValid]);

  const requestData = ({ id }: { id: string }) => {
    const parent = entriesRef.current.get(id);
    if (!parent) {
      setError("RackForge could not resolve this folder.");
      return;
    }
    hostJson<ResourceEntry[]>(
      `/api/v1/resources/entries/${encodeURIComponent(parent.id)}`,
    )
      .then((entries) => {
        const entities = entries.map((entry) => toFileManagerEntry(entry, id));
        entities.forEach((entity, index) =>
          entriesRef.current.set(entity.id, entries[index]),
        );
        return apiRef.current?.exec("provide-data", {
          id,
          data: entities,
          skipProvider: true,
        });
      })
      .catch((reason: unknown) =>
        setError(
          reason instanceof Error ? reason.message : "Could not read this folder.",
        ),
      );
  };

  const select = () => {
    if (!selected || !selectionValid || binding) return;
    if (props.mode === "select") {
      props.onSelected(selected);
      return;
    }
    setOperation("binding");
    setError(null);
    hostJson<ResourceGrant>("/api/v1/resources/bind", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        plugin_id: props.pluginId,
        resource_id: props.resource.id,
        entry_id: selected.id,
      }),
    })
      .then(props.onBound)
      .catch((reason: unknown) => {
        setOperation(null);
        setError(
          reason instanceof Error ? reason.message : "Could not bind this resource.",
        );
      });
  };

  const uploadClientFile = async (file: File) => {
    if (props.mode === "select") return;
    if (file.size === 0 || file.size > MAX_CLIENT_RESOURCE_BYTES) {
      setError("The file is empty or exceeds RackForge's 2 GB limit.");
      return;
    }
    setOperation("uploading");
    setError(null);
    try {
      const selection = await hostJson<ResourceSelection>(
        `/api/v1/resources/uploads?name=${encodeURIComponent(file.name)}`,
        {
          method: "POST",
          headers: { "content-type": "application/octet-stream" },
          body: file,
        },
      );
      const grant = await hostJson<ResourceGrant>(
        "/api/v1/resources/bind-selection",
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            plugin_id: props.pluginId,
            resource_id: props.resource.id,
            selection_id: selection.selection_id,
          }),
        },
      );
      props.onBound(grant);
    } catch (reason) {
      setOperation(null);
      setError(
        reason instanceof Error
          ? reason.message
          : "Could not upload this resource to RackForge.",
      );
    } finally {
      if (uploadInputRef.current) uploadInputRef.current.value = "";
    }
  };

  const clientUploadAvailable =
    props.mode !== "select" && target.kind === "file" && !isNativeHost();

  return (
    <div className="resource-explorer-backdrop" role="presentation">
      <section
        className={`resource-explorer-dialog${clientUploadAvailable ? " has-client-upload" : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="resource-explorer-title"
      >
        <header className="resource-explorer-header">
          <div>
            <span className="eyebrow">RACKFORGE STORAGE</span>
            <h2 id="resource-explorer-title">Select {target.name}</h2>
          </div>
          <button type="button" className="icon-button" onClick={onCancel} disabled={binding}>
            Close
          </button>
        </header>
        {clientUploadAvailable ? (
          <div className="resource-explorer-source-bar">
            <div>
              <FileUp aria-hidden="true" />
              <span>
                <strong>File on this device</strong>
                <small>Upload it securely to the RackForge host</small>
              </span>
            </div>
            <button
              type="button"
              disabled={binding}
              onClick={() => uploadInputRef.current?.click()}
            >
              <AsyncActionLabel active={operation === "uploading"} activeLabel="Uploading…">
                Upload from this device
              </AsyncActionLabel>
            </button>
            <input
              ref={uploadInputRef}
              className="visually-hidden"
              type="file"
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) void uploadClientFile(file);
              }}
            />
          </div>
        ) : null}
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
            <div className="resource-explorer-empty async-status-line">
              <AsyncSpinner label="Reading storage…" size="medium" />
              <span>Reading storage…</span>
            </div>
          )}
        </div>
        <footer className="resource-explorer-footer">
          <div>
            <span>{hint}</span>
            {error && data ? <strong>{error}</strong> : null}
          </div>
          <div className="resource-explorer-actions">
            <button type="button" className="secondary" onClick={onCancel} disabled={binding}>
              Cancel
            </button>
            <button type="button" disabled={!selectionValid || binding} onClick={select}>
              <AsyncActionLabel active={operation === "binding"} activeLabel="Selecting…">
                {`Select ${target.kind}`}
              </AsyncActionLabel>
            </button>
          </div>
        </footer>
      </section>
    </div>
  );
}
