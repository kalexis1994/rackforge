package org.rackforge.android;

import static org.junit.Assert.assertEquals;

import java.util.List;
import org.junit.Test;

public final class StartupPluginOrderTest {
    private static StartupPluginOrder.Candidate plugin(
            String id, String root, boolean compatible, boolean enabled) {
        return new StartupPluginOrder.Candidate(id, root, compatible, enabled);
    }

    @Test
    public void freshInstallStartsWithConcertGrand() {
        List<String> roots = StartupPluginOrder.roots(List.of(
                plugin("org.rackforge.rf-106", "/plugins/rf-106", true, true),
                plugin(StartupPluginOrder.DEFAULT_PLUGIN_ID, "/plugins/piano", true, true)
        ), null);

        assertEquals(List.of("/plugins/piano", "/plugins/rf-106"), roots);
    }

    @Test
    public void persistedSelectionWinsAndPianoRemainsFirstFallback() {
        List<String> roots = StartupPluginOrder.roots(List.of(
                plugin(StartupPluginOrder.DEFAULT_PLUGIN_ID, "/plugins/piano", true, true),
                plugin("org.rackforge.rf-106", "/plugins/rf-106", true, true),
                plugin("org.rackforge.other", "/plugins/other", true, true)
        ), "/plugins/rf-106");

        assertEquals(
                List.of("/plugins/rf-106", "/plugins/piano", "/plugins/other"),
                roots);
    }

    @Test
    public void unavailablePluginsCannotBecomeStartupCandidates() {
        List<String> roots = StartupPluginOrder.roots(List.of(
                plugin(StartupPluginOrder.DEFAULT_PLUGIN_ID, "/plugins/piano", false, true),
                plugin("org.rackforge.disabled", "/plugins/disabled", true, false),
                plugin("org.rackforge.ready", "/plugins/ready", true, true)
        ), "/plugins/disabled");

        assertEquals(List.of("/plugins/ready"), roots);
    }
}
