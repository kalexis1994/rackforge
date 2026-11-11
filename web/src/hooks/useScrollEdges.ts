import { useEffect, useState, type RefObject } from "react";

export interface ScrollEdges {
  /** There is content scrolled out past the start (left) edge. */
  start: boolean;
  /** There is content still to scroll to past the end (right) edge. */
  end: boolean;
}

/** A pixel of slack: fractional layout never lands exactly on the end. */
const SLACK = 1;

/** Which edges of a horizontal scroller have content beyond them. */
export function scrollEdges(scrollLeft: number, scrollWidth: number, clientWidth: number): ScrollEdges {
  const overflow = scrollWidth - clientWidth;
  if (overflow <= SLACK) return { start: false, end: false };
  return {
    start: scrollLeft > SLACK,
    end: scrollLeft < overflow - SLACK,
  };
}

/**
 * The edges of a horizontal scroller that have content beyond them, kept
 * current as it scrolls, as it is resized, and as what it holds changes --
 * so an edge can fade out where there is more to see instead of cutting the
 * content off square.
 */
export function useScrollEdges(ref: RefObject<HTMLElement | null>): ScrollEdges {
  const [edges, setEdges] = useState<ScrollEdges>({ start: false, end: false });
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = () => {
      const next = scrollEdges(element.scrollLeft, element.scrollWidth, element.clientWidth);
      setEdges((current) =>
        current.start === next.start && current.end === next.end ? current : next,
      );
    };
    update();
    element.addEventListener("scroll", update, { passive: true });
    // Its own size, and the size of whatever it holds: an item added or
    // removed changes how far it scrolls without changing its box.
    const resized = new ResizeObserver(update);
    resized.observe(element);
    const observeChildren = () => {
      for (const child of element.children) resized.observe(child);
    };
    observeChildren();
    const mutated = new MutationObserver(() => {
      observeChildren();
      update();
    });
    mutated.observe(element, { childList: true });
    return () => {
      element.removeEventListener("scroll", update);
      resized.disconnect();
      mutated.disconnect();
    };
  }, [ref]);
  return edges;
}
