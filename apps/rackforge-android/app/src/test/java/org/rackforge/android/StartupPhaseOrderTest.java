package org.rackforge.android;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertThrows;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public final class StartupPhaseOrderTest {
    @Test
    public void phasesAdvanceMonotonicallyAndRepeatsAreIdempotent() {
        StartupPhaseOrder order = new StartupPhaseOrder();

        assertTrue(order.advance(StartupPhaseOrder.AUDIO_READY));
        assertFalse(order.advance(StartupPhaseOrder.AUDIO_READY));
        assertTrue(order.advance(StartupPhaseOrder.CONTROL_READY));
        assertTrue(order.advance(StartupPhaseOrder.BACKGROUND_READY));
        assertEquals(StartupPhaseOrder.BACKGROUND_READY, order.highest());
    }

    @Test
    public void optionalControllerPhaseMayBeSkippedButNeverReintroduced() {
        StartupPhaseOrder order = new StartupPhaseOrder();
        order.advance(StartupPhaseOrder.AUDIO_READY);
        order.advance(StartupPhaseOrder.BACKGROUND_READY);

        assertThrows(IllegalStateException.class,
                () -> order.advance(StartupPhaseOrder.CONTROL_READY));
    }

    @Test
    public void unknownPhasesAreRejected() {
        StartupPhaseOrder order = new StartupPhaseOrder();
        assertThrows(IllegalArgumentException.class, () -> order.advance(0));
        assertThrows(IllegalArgumentException.class, () -> order.advance(4));
    }
}
