import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { requestAudioInput } from "../gateway";
import type { AudioInputStatus } from "../types";

/** Asked again this long after a host failed to answer: one that predates
 *  the question, or one that is reconnecting. */
const RETRY_AFTER_FAILURE_MS = 4_000;

/** What the host captures, without its peaks. */
export type AudioInputState = Omit<AudioInputStatus, "peaks">;

/**
 * The peaks of the captured inputs, reading by reading. Kept out of React
 * state on purpose: they change several times a second, and only the meters
 * that show them should draw again -- not the graph around them.
 */
export interface AudioInputPeakFeed {
  get: () => number[];
  subscribe: (listener: () => void) => () => void;
}

const NO_PEAKS: number[] = [];

function createPeakFeed(): AudioInputPeakFeed & { publish: (peaks: number[]) => void } {
  let peaks = NO_PEAKS;
  const listeners = new Set<() => void>();
  return {
    get: () => peaks,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    publish: (next) => {
      peaks = next;
      for (const listener of listeners) listener();
    },
  };
}

function sameState(a: AudioInputState | null, b: AudioInputState) {
  return a !== null
    && a.availability === b.availability
    && a.device_name === b.device_name
    && a.device_channels === b.device_channels
    && a.gain_db === b.gain_db
    && a.cable_routing === b.cable_routing
    && a.reason === b.reason
    && a.captured.length === b.captured.length
    && a.captured.every((channel, index) => channel === b.captured[index]);
}

/**
 * Reads what the host captures, again and again while `intervalMs` is set:
 * the next request goes out only after the previous one is answered, so a
 * slow host is never asked twice at once. `status` is the same object until
 * something in it changes, so what depends on it -- the graph's problems,
 * the node's subtitle -- is not recomputed on every reading. A host that
 * cannot answer leaves it null -- unknown, which is not "no input" -- and is
 * asked again a while later.
 */
export function useAudioInputStatus(intervalMs: number | null): {
  status: AudioInputState | null;
  peaks: AudioInputPeakFeed;
} {
  const [status, setStatus] = useState<AudioInputState | null>(null);
  const statusRef = useRef<AudioInputState | null>(null);
  const feed = useMemo(createPeakFeed, []);
  useEffect(() => {
    if (intervalMs === null) return;
    let active = true;
    let timer: number | undefined;
    const ask = () => {
      requestAudioInput()
        .then((input) => {
          if (!active) return;
          const { peaks, ...next } = input;
          if (!sameState(statusRef.current, next)) {
            statusRef.current = next;
            setStatus(next);
          }
          feed.publish(Array.isArray(peaks) ? peaks : NO_PEAKS);
          timer = window.setTimeout(ask, intervalMs);
        })
        .catch(() => {
          if (!active) return;
          if (statusRef.current !== null) {
            statusRef.current = null;
            setStatus(null);
          }
          feed.publish(NO_PEAKS);
          timer = window.setTimeout(ask, RETRY_AFTER_FAILURE_MS);
        });
    };
    ask();
    return () => {
      active = false;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [feed, intervalMs]);
  return { status, peaks: feed };
}

/** The latest peaks from a feed, for a meter. */
export function useAudioInputPeaks(feed: AudioInputPeakFeed | null): number[] {
  return useSyncExternalStore(
    feed?.subscribe ?? noSubscription,
    feed?.get ?? noPeaks,
  );
}

const noSubscription = () => () => {};
const noPeaks = () => NO_PEAKS;
