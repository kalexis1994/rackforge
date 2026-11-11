/**
 * RackForge in a native shell -- the Android app, the desktop window, the
 * VST3 editor -- is an application, not a page: a pinch must not zoom its
 * controls away. In a browser the page stays zoomable, as any page should.
 */

const ZOOM_TERMS = /^(maximum-scale|minimum-scale|user-scalable)\s*=/i;

/** The viewport description with scaling fixed at 1. */
export function withoutZoom(content: string): string {
  const kept = content
    .split(",")
    .map((term) => term.trim())
    .filter((term) => term.length > 0 && !ZOOM_TERMS.test(term));
  return [...kept, "maximum-scale=1", "minimum-scale=1", "user-scalable=no"].join(", ");
}

/** Fixes the document's scale; a native shell calls it before the first render. */
export function lockViewportZoom(document: Document): void {
  const meta = document.querySelector('meta[name="viewport"]');
  if (meta) meta.setAttribute("content", withoutZoom(meta.getAttribute("content") ?? ""));
}
