import { useEffect, useState } from "react";
import { materializePluginState, requestPluginStateParameters } from "../gateway";
import type { PluginParameterSnapshot } from "../types";

export type PluginParameterSchema = PluginParameterSnapshot["schema"];

/*
 * A plugin's parameter schema without an instance on stage: the host builds
 * a state of the plugin's default program and answers what that state's
 * parameters are, as the Rack editor does for a slot. One ask per plugin
 * version for the life of the page; a failure is not remembered, so the
 * next look tries again.
 */
const schemas = new Map<string, Promise<PluginParameterSchema>>();

export function loadPluginParameterSchema(pluginId: string, version = ""): Promise<PluginParameterSchema> {
  const key = `${pluginId}@${version}`;
  let pending = schemas.get(key);
  if (!pending) {
    pending = materializePluginState(pluginId)
      .then(requestPluginStateParameters)
      .then((snapshot) => snapshot.schema);
    schemas.set(key, pending);
    pending.catch(() => schemas.delete(key));
  }
  return pending;
}

export type SchemaState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; schema: PluginParameterSchema }
  | { status: "error"; message: string };

export function usePluginParameterSchema(pluginId: string | null, version = ""): SchemaState {
  const [state, setState] = useState<{ key: string; value: SchemaState }>({ key: "", value: { status: "idle" } });
  const key = pluginId ? `${pluginId}@${version}` : "";
  useEffect(() => {
    if (!pluginId) return;
    let active = true;
    loadPluginParameterSchema(pluginId, version)
      .then((schema) => {
        if (active) setState({ key, value: { status: "ready", schema } });
      })
      .catch((reason: unknown) => {
        if (active) {
          setState({
            key,
            value: {
              status: "error",
              message: reason instanceof Error ? reason.message : "Could not read this plugin's parameters.",
            },
          });
        }
      });
    return () => {
      active = false;
    };
  }, [key, pluginId, version]);
  if (!pluginId) return { status: "idle" };
  return state.key === key ? state.value : { status: "loading" };
}
