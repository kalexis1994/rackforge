package org.rackforge.android;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/** Defines deterministic startup precedence without coupling it to Android or JSON. */
final class StartupPluginOrder {
    static final String DEFAULT_PLUGIN_ID = "org.rackforge.concert-grand";

    static final class Candidate {
        final String pluginId;
        final String packageRoot;
        final boolean compatible;
        final boolean enabled;

        Candidate(String pluginId, String packageRoot, boolean compatible, boolean enabled) {
            this.pluginId = pluginId;
            this.packageRoot = packageRoot;
            this.compatible = compatible;
            this.enabled = enabled;
        }
    }

    private StartupPluginOrder() {
    }

    static List<String> roots(List<Candidate> candidates, String preferredRoot) {
        String restored = null;
        String bundledDefault = null;
        List<String> remaining = new ArrayList<>();
        for (Candidate candidate : candidates) {
            if (!candidate.compatible || !candidate.enabled || candidate.packageRoot.isBlank()) {
                continue;
            }
            if (candidate.packageRoot.equals(preferredRoot)) {
                restored = candidate.packageRoot;
            } else if (DEFAULT_PLUGIN_ID.equals(candidate.pluginId)) {
                bundledDefault = candidate.packageRoot;
            } else {
                remaining.add(candidate.packageRoot);
            }
        }

        Set<String> ordered = new LinkedHashSet<>();
        if (restored != null) ordered.add(restored);
        if (bundledDefault != null) ordered.add(bundledDefault);
        ordered.addAll(remaining);
        return new ArrayList<>(ordered);
    }
}
