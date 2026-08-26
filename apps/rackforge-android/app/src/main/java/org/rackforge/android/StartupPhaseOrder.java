package org.rackforge.android;

import java.util.concurrent.atomic.AtomicInteger;

/** Platform-neutral ordering guard for one Android engine generation. */
final class StartupPhaseOrder {
    static final int AUDIO_READY = 1;
    static final int CONTROL_READY = 2;
    static final int BACKGROUND_READY = 3;

    private final AtomicInteger highest = new AtomicInteger();

    /** Returns true only when a new phase was published. */
    boolean advance(int phase) {
        if (phase < AUDIO_READY || phase > BACKGROUND_READY) {
            throw new IllegalArgumentException("Unknown startup phase " + phase);
        }
        for (;;) {
            int current = highest.get();
            if (phase < current) {
                throw new IllegalStateException(
                        "Startup phase cannot regress from " + label(current)
                                + " to " + label(phase));
            }
            if (phase == current) return false;
            if (highest.compareAndSet(current, phase)) return true;
        }
    }

    int highest() {
        return highest.get();
    }

    static String label(int phase) {
        return switch (phase) {
            case AUDIO_READY -> "audio_ready";
            case CONTROL_READY -> "control_ready";
            case BACKGROUND_READY -> "background_ready";
            default -> "starting";
        };
    }
}
