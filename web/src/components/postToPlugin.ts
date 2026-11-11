/**
 * Posts a message into a plugin's frame, from this window.
 *
 * WebKitGTK -- the view the Linux desktop shell embeds -- delivered a message
 * posted straight into the frame from here as if the frame had posted it to
 * itself: its `MessageEvent.source` was the frame's own window, not this one.
 * Every plugin answers its parent alone, so each such message was dropped,
 * and RF-Musette's panel waited four seconds and said RackForge did not
 * answer. Called through `Reflect.apply`, as here, the message carries this
 * window as its source in WebKitGTK too; Chromium and WebView2 report it
 * either way.
 */
export function postToPlugin(
  frame: HTMLIFrameElement | null | undefined,
  message: unknown,
): void {
  const target = frame?.contentWindow;
  if (!target) return;
  Reflect.apply(target.postMessage, target, [message, window.location.origin]);
}
