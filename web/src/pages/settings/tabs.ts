export const SETTINGS_TABS = [
  ["audio", "Audio"],
  ["midi", "MIDI"],
  ["input", "Input"],
  ["screen", "Screen"],
  // Network is the local HTTP server, and the PIN guards exactly that: it is
  // one subject, so it is one section. Security used to sit beside it, which
  // asked the player to know that the passcode belonged to the server.
  ["network", "Network"],
] as const;

export type SettingsTab = (typeof SETTINGS_TABS)[number][0];

/** The sections this host actually has.
 *
 * Network is the local HTTP server and the PIN that guards it, and two hosts
 * have no such server. The browser demo is its own host: the page is the
 * instrument, with no port to choose and no PIN to guard it with. The Android
 * app runs its engine in-process behind a JNI bridge, with nothing listening
 * on a port and no endpoint that would answer the section's questions.
 *
 * Windows, the Linux desktop and the Raspberry Pi all publish a server, and
 * all keep it.
 */
export function settingsTabsFor(serverless: boolean): ReadonlyArray<readonly [SettingsTab, string]> {
  return serverless ? SETTINGS_TABS.filter(([id]) => id !== "network") : SETTINGS_TABS;
}

export function isSettingsTab(value: string | null, serverless: boolean): value is SettingsTab {
  return settingsTabsFor(serverless).some(([id]) => id === value);
}
