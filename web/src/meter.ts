export const METER_FLOOR_DB = -60;

export function amplitudeToMeterDb(value: number) {
  if (!Number.isFinite(value) || value <= 0) return METER_FLOOR_DB;
  return Math.max(METER_FLOOR_DB, Math.min(3, 20 * Math.log10(value)));
}

export function meterPercent(db: number) {
  return Math.max(0, Math.min(100, ((db - METER_FLOOR_DB) / -METER_FLOOR_DB) * 100));
}
