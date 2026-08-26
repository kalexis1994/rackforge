export type StartupPhase = "audio_ready" | "control_ready" | "background_ready";

const rank: Record<StartupPhase, number> = {
  audio_ready: 1,
  control_ready: 2,
  background_ready: 3,
};

/** Monotonic availability telemetry shared by browser-host generations. */
export class StartupTimeline {
  private highest = 0;
  private readonly started = performance.now();

  constructor(private readonly host: string) {
    this.publish("starting", 0);
  }

  advance(phase: StartupPhase): number {
    const requested = rank[phase];
    if (requested < this.highest) {
      throw new Error(
        `Startup phase cannot regress from ${this.current()} to ${phase}`,
      );
    }
    const elapsed = performance.now() - this.started;
    if (requested === this.highest) return elapsed;
    this.highest = requested;
    this.publish(phase, elapsed);
    return elapsed;
  }

  current(): StartupPhase | null {
    return (
      Object.entries(rank).find(([, value]) => value === this.highest)?.[0] as
        | StartupPhase
        | undefined
    ) ?? null;
  }

  private publish(phase: StartupPhase | "starting", elapsed: number) {
    console.info(
      `STARTUP_PHASE host=${this.host} phase=${phase} elapsed_ms=${Math.round(elapsed)}`,
    );
  }
}
