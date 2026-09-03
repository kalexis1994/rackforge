/**
 * What the keyboard last sent, for the velocity square to aim with.
 *
 * The host keeps one word: a strike count, the velocity that arrived, and
 * what the reading made of it. This asks for it only while the square is on
 * screen, and stops the moment it is not — a settings page has no business
 * polling a host for a marker nobody is looking at.
 */

import { useEffect, useRef, useState } from "react";

import { hostJson } from "./host";

export interface LastStrike {
  /** Strikes since the engine started. Zero means nothing has been played. */
  count: number;
  /** What the keyboard sent, 1..127. */
  velocity: number;
  /** What the host's own reading made of it. */
  played: number;
}

/** How often to ask. Fast enough to feel immediate, slow enough to be free. */
const INTERVAL_MS = 70;

export function useLastStrike(enabled: boolean): LastStrike | null {
  const [strike, setStrike] = useState<LastStrike | null>(null);
  const alive = useRef(false);

  useEffect(() => {
    if (!enabled) return;
    alive.current = true;
    let timer = 0;
    const tick = async () => {
      try {
        const next = await hostJson<LastStrike>("/api/v1/host/midi/last-strike");
        if (!alive.current) return;
        // Only a new strike is news: setting the same object every tick would
        // re-render this page seventeen times a second for nothing.
        setStrike((current) =>
          current && current.count === next.count ? current : next,
        );
      } catch {
        // A host that cannot answer simply leaves the square unmarked.
      }
      if (alive.current) {
        timer = window.setTimeout(() => void tick(), INTERVAL_MS);
      }
    };
    void tick();
    return () => {
      alive.current = false;
      window.clearTimeout(timer);
    };
  }, [enabled]);

  // Read, not cleared: switching the square off is a state of the caller,
  // and writing it back into this one would be a render to say nothing.
  return enabled ? strike : null;
}
