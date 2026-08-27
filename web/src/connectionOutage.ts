export const CONNECTION_INTERRUPTED_MESSAGE =
  "The RackForge Core connection was interrupted.";

/**
 * Keeps brief transport handovers out of the user-facing error channel.
 *
 * WebSocket and native WebView transports can report an error immediately
 * before their automatic reconnect succeeds. That is useful diagnostic data,
 * but presenting it as a durable Core failure makes a healthy recovery look
 * broken. One timer spans all reconnect attempts and is only cancelled after
 * the session is online again.
 */
export class DeferredConnectionOutage {
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly delayMs: number,
    private readonly publish: () => void,
  ) {}

  begin() {
    if (this.timer !== null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.publish();
    }, this.delayMs);
  }

  recover() {
    if (this.timer === null) return;
    clearTimeout(this.timer);
    this.timer = null;
  }
}
