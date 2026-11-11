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
  /** The list is revealed by now whatever the artwork is doing. Kept short
   *  -- a list held back reads as a hang, not as care. */
  timeoutMs: number;
}

export const ARTWORK_REVEAL_TIMING: RevealTiming = {
  loaderDelayMs: 120,
  loaderMinimumMs: 450,
  // --rf-motion-emphasized, the loader's fade in CSS.
  loaderFadeMs: 320,
  timeoutMs: 1_500,
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
 *
 * The deadline is part of the arithmetic, not a timer beside it: an
 * unfinished wait counts as finished at `startedAt + timeoutMs`. Nothing an
 * image or an effect does can hold the list back past it.
 */
export function revealState(
  startedAt: number,
  readyAt: number | null,
  now: number,
  timing: RevealTiming = ARTWORK_REVEAL_TIMING,
): RevealState {
  const deadline = startedAt + timing.timeoutMs;
  const finishedAt = readyAt === null
    ? now >= deadline ? deadline : null
    : Math.min(readyAt, deadline);
  const loaderAt = startedAt + timing.loaderDelayMs;
  if (finishedAt !== null && finishedAt <= loaderAt) {
    // Fast enough that a loader would only have flashed.
    return now >= finishedAt
      ? { revealed: true, loader: "hidden", nextChangeAt: null }
      : { revealed: false, loader: "hidden", nextChangeAt: finishedAt };
  }
  if (now < loaderAt) return { revealed: false, loader: "hidden", nextChangeAt: loaderAt };
  if (finishedAt === null) return { revealed: false, loader: "shown", nextChangeAt: deadline };
  const revealAt = Math.max(finishedAt, loaderAt + timing.loaderMinimumMs);
  if (now < revealAt) return { revealed: false, loader: "shown", nextChangeAt: revealAt };
  const goneAt = revealAt + timing.loaderFadeMs;
  if (now < goneAt) return { revealed: true, loader: "leaving", nextChangeAt: goneAt };
  return { revealed: true, loader: "gone", nextChangeAt: null };
}

/**
 * Artwork already waited for in this visit -- decoded, broken, or given up
 * on. Reopening never waits for the same image twice: a slow or broken one
 * would otherwise hold the list back on every open.
 */
const waitedFor = new Set<string>();

function decodeImage(url: string): Promise<void> {
  if (waitedFor.has(url)) return Promise.resolve();
  const image = new Image();
  image.src = url;
  return image
    .decode()
    .catch(() => {
      // A broken banner must not hold the list back; it shows as broken.
    })
    .finally(() => {
      waitedFor.add(url);
    });
}

/**
 * The reveal state for a list whose artwork is `urls`.
 *
 * One wait per mount, on one clock. The list can change while it is open --
 * the catalogue refreshes, the active instrument moves to the top -- and an
 * earlier version started a new wait each time: the clock restarted, and a
 * list already on screen hid again behind the loader. Now a change only adds
 * artwork to decode; the deadline still counts from the open, and once the
 * list is revealed it stays revealed, new artwork loading in place.
 */
export function useArtworkReveal(urls: readonly string[], timing = ARTWORK_REVEAL_TIMING) {
  // The set, not the order: the list reorders when the active instrument
  // changes, and that is not new artwork to wait for.
  const key = [...new Set(urls)].sort().join("\n");
  const [startedAt] = useState(() => performance.now());
  const [readyAt, setReadyAt] = useState<number | null>(() =>
    urls.every((url) => waitedFor.has(url)) ? startedAt : null,
  );
  const [now, setNow] = useState(startedAt);

  useEffect(() => {
    if (readyAt !== null) return;
    let alive = true;
    const list = key ? key.split("\n") : [];
    void Promise.all(list.map(decodeImage)).then(() => {
      if (alive) setReadyAt((current) => current ?? performance.now());
    });
    return () => {
      alive = false;
    };
  }, [key, readyAt]);

  const state = revealState(startedAt, readyAt, now, timing);

  // Wake at the next change: the loader's arrival, the deadline, the reveal,
  // the end of the loader's fade. Nothing polls.
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
    if (readyAt === null) return;
    const timer = window.setTimeout(() => setNow(performance.now()), 0);
    return () => window.clearTimeout(timer);
  }, [readyAt]);

  return state;
}
