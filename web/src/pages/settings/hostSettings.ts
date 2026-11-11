import { hostJson } from "../../host";
import { type HostAudioSettings, type WebPublicConfig } from "../../types";

export interface HostSettingsBootstrap {
  config: WebPublicConfig | null;
  audioSettings: HostAudioSettings | null;
}

export let hostSettingsBootstrapPromise: Promise<HostSettingsBootstrap> | null = null;

export function requestHostSettingsBootstrap() {
  hostSettingsBootstrapPromise ??= Promise.all([
    hostJson<WebPublicConfig>("/api/v1/config").catch(() => null),
    hostJson<HostAudioSettings>("/api/v1/host/audio").catch(() => null),
  ]).then(([config, audioSettings]) => ({
    config,
    audioSettings,
  }));
  return hostSettingsBootstrapPromise;
}
