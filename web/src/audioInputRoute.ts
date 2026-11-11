import type { AudioInputState } from "./hooks/useAudioInputStatus";
import type { RackAudioInputRoute } from "./types";

/** The trim a cable may apply, in dB -- the host's own input trim's range. */
export const AUDIO_INPUT_ROUTE_GAIN_MIN_DB = -60;
export const AUDIO_INPUT_ROUTE_GAIN_MAX_DB = 24;
/** The highest input number a cable may name (the host's limit). */
const MAX_INPUT = 64;

/** The floor of an input meter, in dBFS: below it a bar is empty. */
const INPUT_METER_FLOOR_DB = -60;

/** Where a linear peak sits on the meter, 0 (floor) to 1 (0 dBFS). */
export function meterLevel(peak: number): number {
  if (!Number.isFinite(peak) || peak <= 0) return 0;
  const db = 20 * Math.log10(peak);
  return Math.max(0, Math.min(1, (db - INPUT_METER_FLOOR_DB) / -INPUT_METER_FLOOR_DB));
}

/** A route as it is kept: nothing for the default, channels only when some
 *  are chosen, a trim only when it is not unity. */
export function normalizeAudioInputRoute(
  route: RackAudioInputRoute,
): RackAudioInputRoute | undefined {
  const channels = (route.channels ?? []).slice(0, 2);
  const gain = Math.round(Math.max(
    AUDIO_INPUT_ROUTE_GAIN_MIN_DB,
    Math.min(AUDIO_INPUT_ROUTE_GAIN_MAX_DB, route.gain_db ?? 0),
  ));
  if (channels.length === 0 && gain === 0) return undefined;
  return {
    ...(channels.length > 0 ? { channels } : {}),
    ...(gain !== 0 ? { gain_db: gain } : {}),
  };
}

/** How many inputs to offer: the interface's, and never fewer than what is
 *  captured or already chosen, so a route saved on another machine still
 *  shows what it names. */
export function offeredInputCount(
  status: AudioInputState | null,
  route: RackAudioInputRoute,
): number {
  const highest = Math.max(
    2,
    status?.device_channels ?? 0,
    ...(status?.captured ?? []),
    ...(route.channels ?? []),
  );
  return Math.min(MAX_INPUT, highest);
}
