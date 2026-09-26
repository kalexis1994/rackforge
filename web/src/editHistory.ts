/**
 * An editor's history: every state the draft has been in, named, so a step
 * can be undone, redone, or returned to from a list.
 *
 * Pure: the editor keeps one of these beside its draft and records each
 * change as it happens -- whoever made it, the player's hand or one of the
 * graph's rules (an effect inserted into a chain, a chain closed behind a
 * removed node). Undoing an automatic edit is as ordinary as undoing any.
 */

export interface HistoryEntry<T> {
  /** The draft as it was before the step named by `label`. */
  value: T;
  /** What the step did, as the list shows it. */
  label: string;
  /** When the step was taken, for grouping a burst of the same step. */
  at: number;
}

export interface EditHistory<T> {
  /** Oldest first: undoing takes the last. */
  past: HistoryEntry<T>[];
  /** Nearest first: redoing takes the first. Each holds the draft as it was
   *  before the undo that put it here, and the label of the step undone. */
  future: HistoryEntry<T>[];
}

export const HISTORY_LIMIT = 100;
/** Steps of the same kind closer together than this are one step: a name
 *  typed letter by letter is one "Renamed", not one per letter. */
export const HISTORY_GROUP_MS = 800;

export function emptyHistory<T>(): EditHistory<T> {
  return { past: [], future: [] };
}

/**
 * Records that the draft went from `previous` to its next state by the step
 * `label`. A new step drops whatever could have been redone; a step of the
 * same kind right after the last one extends it instead of adding another.
 */
export function recordStep<T>(
  history: EditHistory<T>,
  previous: T,
  label: string,
  now: number,
): EditHistory<T> {
  const last = history.past[history.past.length - 1];
  if (last && last.label === label && now - last.at < HISTORY_GROUP_MS) {
    return {
      past: [...history.past.slice(0, -1), { ...last, at: now }],
      future: [],
    };
  }
  const past = [...history.past, { value: previous, label, at: now }];
  return {
    past: past.length > HISTORY_LIMIT ? past.slice(past.length - HISTORY_LIMIT) : past,
    future: [],
  };
}

/** Steps back once: the draft to show, and the history after. */
export function undoStep<T>(
  history: EditHistory<T>,
  current: T,
): { history: EditHistory<T>; value: T } | null {
  const last = history.past[history.past.length - 1];
  if (!last) return null;
  return {
    value: last.value,
    history: {
      past: history.past.slice(0, -1),
      future: [{ value: current, label: last.label, at: last.at }, ...history.future],
    },
  };
}

/** Steps forward once: the draft to show, and the history after. */
export function redoStep<T>(
  history: EditHistory<T>,
  current: T,
): { history: EditHistory<T>; value: T } | null {
  const next = history.future[0];
  if (!next) return null;
  return {
    value: next.value,
    history: {
      past: [...history.past, { value: current, label: next.label, at: next.at }],
      future: history.future.slice(1),
    },
  };
}

/**
 * Moves to a step in the list: `stepsBack` undos when positive, redos when
 * negative. The list the editor shows is `past` then the present then
 * `future`, so a click on an entry is a distance from the present.
 */
export function jumpSteps<T>(
  history: EditHistory<T>,
  current: T,
  stepsBack: number,
): { history: EditHistory<T>; value: T } {
  let state = { history, value: current };
  const move = stepsBack > 0 ? undoStep : redoStep;
  for (let index = 0; index < Math.abs(stepsBack); index += 1) {
    const next = move(state.history, state.value);
    if (!next) break;
    state = next;
  }
  return state;
}
