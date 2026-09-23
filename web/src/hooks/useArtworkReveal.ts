import { useEffect, useState } from "react";

/**
 * Shows a list once its artwork has arrived, and a loader only when that
 * takes long enough to be seen.
 *
 * A list of branded cards appearing at once, and then its banners landing
 * one by one as each image finishes, reads as something breaking in rather
 * than opening. So the artwork is decoded first; the list stays hidden (its
 * space kept, so the dialog does not change size) and is revealed whole.
 *
 * The loader follows two rules that stop it from being noise: it appears
 * only if the wait outlasts `loaderDelayMs` -- a cached list simply appears
 * -- and once it has appeared it stays `loaderMinimumMs`, so it never blinks.
 * Revealing starts the loader's fade and the list's entrance together.
 */
export interface RevealTiming {
  loaderDelayMs: number;
  loaderMinimumMs: number;
  loaderFadeMs: number;
  /** Given up waiting: whatever has not decoded by now shows as it loads. */
  timeoutMs: number;
}

export const ARTWORK_REVEAL_TIMING: RevealTiming = {
  loaderDelayMs: 120,
  loaderMinimumMs: 450,
  // --rf-motion-emphasized, the loader's fade in CSS.
  loaderFadeMs: 320,
  timeoutMs: 2_500,
};

export type RevealLoader = "hidden" | "shown" | "leaving" | "gone";

export interface RevealState {
  revealed: boolean;
  loader: RevealLoader;
  /** When the state next changes, or null once it never will. */
  nextChangeAt: number | null;
}

/**
 * What is on screen at `now`, for a wait that started at `startedAt` and
 * finished at `readyAt` (null while still waiting).
 */
export function revealState(
  startedAt: number,
  readyAt: number | null,
  now: number,
  timing: RevealTiming = ARTWORK_REVEAL_TIMING,
): RevealState {
  const loaderAt = startedAt + timing.loaderDelayMs;
  if (readyAt !== null && readyAt <= loaderAt) {
    // Fast enough that a loader would only have flashed.
    return now >= readyAt
      ? { revealed: true, loader: "hidden", nextChangeAt: null }
      : { revealed: false, loader: "hidden", nextChangeAt: readyAt };
  }
  if (now < loaderAt) return { revealed: false, loader: "hidden", nextChangeAt: loaderAt };
  if (readyAt === null) return { revealed: false, loader: "shown", nextChangeAt: null };
  const revealAt = Math.max(readyAt, loaderAt + timing.loaderMinimumMs);
  if (now < revealAt) return { revealed: false, loader: "shown", nextChangeAt: revealAt };
  const goneAt = revealAt + timing.loaderFadeMs;
  if (now < goneAt) return { revealed: true, loader: "leaving", nextChangeAt: goneAt };
  return { revealed: true, loader: "gone", nextChangeAt: null };
}

/** Artwork already decoded in this visit: reopening shows it at once. */
const decoded = new Set<string>();

function decodeImage(url: string): Promise<void> {
  if (decoded.has(url)) return Promise.resolve();
  const image = new Image();
  image.src = url;
  return image
    .decode()
    .then(() => {
      decoded.add(url);
    })
    .catch(() => {
      // A broken banner must not hold the list back; it shows as broken.
    });
}

/**
 * The reveal state for a list whose artwork is `urls`. Everything is keyed
 * on the joined list, so a new array with the same images is the same wait.
 */
export function useArtworkReveal(urls: readonly string[], timing = ARTWORK_REVEAL_TIMING) {
  const key = urls.join("\n");
  const [wait, setWait] = useState(() => ({
    key,
    startedAt: performance.now(),
    readyAt: urls.every((url) => decoded.has(url)) ? performance.now() : null as number | null,
  }));
  const [now, setNow] = useState(() => performance.now());

  // A different set of images is a new wait.
  const current = wait.key === key ? wait : null;

  useEffect(() => {
    let alive = true;
    const startedAt = performance.now();
    const list = key ? key.split("\n") : [];
    if (wait.key !== key) {
      const readyAt = list.every((url) => decoded.has(url)) ? startedAt : null;
      // Deferred to a task: the wait is recorded as a new one, not set in
      // the middle of this render's commit.
      queueMicrotask(() => {
        if (alive) setWait({ key, startedAt, readyAt });
      });
    }
    if (list.every((url) => decoded.has(url))) {
      return () => {
        alive = false;
      };
    }
    const settle = () => {
      if (!alive) return;
      const readyAt = performance.now();
      setWait((previous) =>
        previous.key === key && previous.readyAt === null ? { ...previous, readyAt } : previous,
      );
    };
    const timer = window.setTimeout(settle, timing.timeoutMs);
    void Promise.all(list.map(decodeImage)).then(settle);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [key, timing.timeoutMs, wait.key]);

  const state = current
    ? revealState(current.startedAt, current.readyAt, now, timing)
    : { revealed: false, loader: "hidden" as const, nextChangeAt: null };

  // Wake at the next change: the loader's arrival, the reveal, the end of
  // the loader's fade. Nothing polls.
  useEffect(() => {
    if (state.nextChangeAt === null) return;
    const timer = window.setTimeout(
      () => setNow(performance.now()),
      Math.max(0, state.nextChangeAt - performance.now()),
    );
    return () => window.clearTimeout(timer);
  }, [state.nextChangeAt]);

  // A wait that just finished changes what `now` means: read the clock again.
  useEffect(() => {
    if (current?.readyAt == null) return;
    const timer = window.setTimeout(() => setNow(performance.now()), 0);
    return () => window.clearTimeout(timer);
  }, [current?.readyAt]);

  return state;
}
