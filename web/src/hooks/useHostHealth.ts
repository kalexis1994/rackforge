import { useEffect, useState } from "react";
import { hostJson } from "../host";

export interface HostHealth {
  revision?: string;
  ui_revision?: string;
  host?: string;
}

/** What the host binary says about itself. Absent until it answers. */
export function useHostHealth() {
  const [host, setHost] = useState<HostHealth | null>(null);
  useEffect(() => {
    let cancelled = false;
    hostJson<HostHealth>("/api/v1/health")
      .then((health) => {
        if (!cancelled) setHost(health);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);
  return host;
}
