import { type PluginWebDescriptor } from "./types";

/**
 * How a kind is named: on the tag a card wears, and on the heading above the
 * group of them. An unstated kind reads as an instrument, which is what a
 * package could only be before the field existed.
 */
export function pluginKindPresentation(kind: PluginWebDescriptor["kind"] | undefined) {
  switch (kind) {
    case "effect":
      return { label: "Effect", plural: "Effects", className: "effect" };
    case "midi_processor":
      return {
        label: "MIDI Processor",
        plural: "MIDI Processors",
        className: "midi-processor",
      };
    default:
      return { label: "Instrument", plural: "Instruments", className: "instrument" };
  }
}

export function formatPluginVersion(version: string | undefined) {
  if (!version) return "";
  return ` v${version.replace(/^[vV]/, "")}`;
}

export function formatFileSize(bytes: number) {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} bytes`;
}
