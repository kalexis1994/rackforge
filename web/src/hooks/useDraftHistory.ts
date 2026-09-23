import { useCallback, useEffect, useRef, useState } from "react";
import {
  emptyHistory,
  jumpSteps,
  recordStep,
  redoStep,
  undoStep,
  type EditHistory,
} from "../editHistory";

export interface DraftHistory {
  canUndo: boolean;
  canRedo: boolean;
  undo: () => void;
  redo: () => void;
  /** The list, oldest first: what each step did, and which one is current. */
  entries: { label: string; current: boolean }[];
  /** Moves to the list entry at `index` (as in `entries`). */
  jumpTo: (index: number) => void;
  /** The next change is not a step -- a save replacing the draft with what
   *  was stored, say. */
  skipNext: () => void;
}

/**
 * Keeps an editor's history beside its draft, by watching the draft: every
 * change is recorded, named by `describe`, whoever made it. A different
 * `identity` -- another Rack opened -- starts a new history.
 */
export function useDraftHistory<T>(
  draft: T | null,
  setDraft: (value: T) => void,
  identity: string | null,
  describe: (before: T, after: T) => string,
): DraftHistory {
  const [history, setHistory] = useState<EditHistory<T>>(emptyHistory);
  const previous = useRef<T | null>(draft);
  const applying = useRef(false);
  const skipping = useRef(false);
  const describeRef = useRef(describe);
  describeRef.current = describe;

  useEffect(() => {
    previous.current = draft;
    applying.current = false;
    skipping.current = false;
    setHistory(emptyHistory());
    // Only a new identity restarts the history; the draft is read at that
    // moment, not followed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [identity]);

  useEffect(() => {
    const before = previous.current;
    previous.current = draft;
    if (applying.current || skipping.current) {
      applying.current = false;
      skipping.current = false;
      return;
    }
    if (before === null || draft === null || before === draft) return;
    if (JSON.stringify(before) === JSON.stringify(draft)) return;
    const label = describeRef.current(before, draft);
    setHistory((current) => recordStep(current, before, label, Date.now()));
  }, [draft]);

  const apply = useCallback((next: { history: EditHistory<T>; value: T } | null) => {
    if (!next) return;
    applying.current = true;
    setHistory(next.history);
    setDraft(next.value);
  }, [setDraft]);

  const undo = useCallback(() => {
    if (draft === null) return;
    apply(undoStep(history, draft));
  }, [apply, draft, history]);
  const redo = useCallback(() => {
    if (draft === null) return;
    apply(redoStep(history, draft));
  }, [apply, draft, history]);
  const jumpTo = useCallback((index: number) => {
    if (draft === null) return;
    const stepsBack = history.past.length - index;
    if (stepsBack === 0) return;
    apply(jumpSteps(history, draft, stepsBack));
  }, [apply, draft, history]);
  const skipNext = useCallback(() => {
    skipping.current = true;
  }, []);

  return {
    canUndo: history.past.length > 0,
    canRedo: history.future.length > 0,
    undo,
    redo,
    // The states the draft has been in: as opened, then after each step.
    // The one it is in now is `past.length` steps from the start.
    entries: [
      "Opened",
      ...history.past.map((entry) => entry.label),
      ...history.future.map((entry) => entry.label),
    ].map((label, index) => ({ label, current: index === history.past.length })),
    jumpTo,
    skipNext,
  };
}
