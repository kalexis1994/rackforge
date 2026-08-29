export type ConnectionStatus = "connecting" | "online" | "idle" | "offline";

export interface HostAudioDriver {
  name: string;
  available: boolean;
  detail: string;
}

export interface HostAudioOutput {
  driver: string;
  name: string;
  is_default: boolean;
  channels: number;
  default_sample_rate: number;
  sample_rates: number[];
  buffer_frames: number[];
}

export type HostAudioInput = HostAudioOutput;

export interface HostAudioRuntimeStatus {
  running?: boolean;
  stream_health?: "healthy" | "recovering" | "lost" | string;
  sample_rate?: number;
  buffer_size_frames?: number;
  frames_per_burst?: number;
  xruns?: number;
  callback_load_percent?: number;
  midi_dropped_events?: number;
}

export interface HostAudioPreferences {
  schema_version: number;
  driver: string;
  output_device: string;
  sample_rate_hz: number;
  buffer_frames?: number;
  output_gain_db: number;
  input_device?: string;
  input_channels?: number[];
  input_gain_db?: number;
  midi_inputs: string[];
}

export interface HostAudioSettings {
  status: "ok";
  host: string;
  inventory: {
    drivers: HostAudioDriver[];
    outputs: HostAudioOutput[];
    inputs?: HostAudioInput[];
    midi_inputs: string[];
  };
  preferences: HostAudioPreferences;
  runtime?: HostAudioRuntimeStatus;
  runtime_status: string;
}

export interface SoundSummary {
  id: string;
  name: string;
  bank?: string;
  detail?: string;
  editable: boolean;
}

export interface PluginInstance {
  instance_id: string;
  plugin_id: string;
  plugin_name: string;
  ui_layouts: string[];
  config_available: boolean;
  sounds: SoundSummary[];
  selected_sound_id?: string;
}

export interface SessionSnapshot {
  schema_version: number;
  session_id: string;
  revision: number;
  active_mode: "idle" | "live" | "play";
  master_level: number;
  master_pan: number;
  live: LivePerformanceState;
  active_instance_id?: string;
  instances: PluginInstance[];
  audition?: AuditionState;
  program_draft?: ProgramDraftState;
  parameter_links?: ParameterLink[];
}

export interface OutputMeterSnapshot {
  left_peak: number;
  right_peak: number;
}

export interface OutputMeterMessage {
  status: "output_meter";
  meter: OutputMeterSnapshot;
}

export interface MidiSourceDescriptor {
  id: string;
  name: string;
  primary: boolean;
}

export interface MidiSourceStatus {
  source: MidiSourceDescriptor;
  connected: boolean;
}

export type ParameterLinkMessage =
  | { type: "control_change"; controller: number }
  | { type: "pitch_bend" }
  | { type: "note"; note: number }
  | { type: "channel_pressure" }
  | { type: "poly_pressure"; note: number };

export interface ParameterLink {
  schema_version: 1;
  id: string;
  instance_id: string;
  parameter_index: number;
  source: { source_id: string; display_name: string };
  channel: { mode: "omni" } | { mode: "channel"; channel: number };
  message: ParameterLinkMessage;
  transform: { invert: boolean };
  pass_through: "pass_through" | "consume";
}

export interface MidiLearnCandidate {
  source: MidiSourceDescriptor;
  channel: number;
  message: ParameterLinkMessage;
}

export type LiveBrowseMode = "rack" | "song" | "setlist";

export type LiveLocation =
  | { kind: "rack"; rack_id: string }
  | { kind: "song"; song_id: string; part_id: string }
  | {
      kind: "setlist";
      setlist_id: string;
      entry_id: string;
      part_id: string;
    };

export interface LivePerformanceState {
  mode: LiveBrowseMode;
  rack?: LiveLocation;
  song?: LiveLocation;
  setlist?: LiveLocation;
  active?: LiveLocation;
  active_rack_id?: string;
}

export type MidiOutputRoute =
  | { kind: "none" }
  | { kind: "bus"; bus_id: string };

export interface RackSlot {
  id: string;
  name: string;
  plugin_id: string;
  state?: PluginStateReference;
  legacy_program_id?: string;
  enabled: boolean;
  midi_input_channel?: number;
  midi_note_low: number;
  midi_note_high: number;
  midi_transpose: number;
  midi_output: MidiOutputRoute;
  audio_output_bus: string;
  level_per_mille: number;
  pan_per_mille: number;
}

export interface RackKeyboardPart {
  midi_channel: number;
  transpose: number;
}

export interface RackKeyboardParts {
  split_key?: number;
  part_1: RackKeyboardPart;
  part_2: RackKeyboardPart;
}

export interface PluginStateReference {
  schema_version: number;
  plugin_id: string;
  plugin_version: string;
  state_version: number;
  blob_sha256: string;
  byte_length: number;
  selected_sound_id?: string;
}

export interface HostPresetSummary {
  id: string;
  name: string;
  plugin_id: string;
  plugin_version: string;
  state_version: number;
  updated_unix_ms: number;
}

export interface HostPreset {
  schema_version: number;
  id: string;
  name: string;
  plugin_id: string;
  created_unix_ms: number;
  updated_unix_ms: number;
  state: PluginStateReference;
}

export interface RfPresetFile {
  format: "org.rackforge.preset";
  schema_version: number;
  exported_by: string;
  exported_unix_ms: number;
  preset: HostPreset;
  state_encoding: "base64";
  state_base64: string;
}

export type PresetImportConflictPolicy = "reject" | "replace" | "keep_both";

export type PresetImportConflictKind =
  | "id"
  | "name"
  | "id_and_name"
  | "ambiguous";

export interface RfPresetImportPreview {
  preset: HostPresetSummary;
  byte_length: number;
  conflict?: PresetImportConflictKind | null;
  compatible: boolean;
  warnings: string[];
}

export interface RfLiveRequirement {
  plugin_id: string;
  version: string;
}

/** A portable `.rflive` show: the whole performance library plus every
 * plugin state its Racks reference. The surface treats the payload as
 * opaque — it validates, transports and displays, never edits. */
export interface RfLiveFile {
  format: "org.rackforge.live";
  schema_version: number;
  exported_by: string;
  exported_unix_ms: number;
  name: string;
  library: unknown;
  states: unknown[];
  requirements: RfLiveRequirement[];
}

export interface RfLiveImportPreview {
  name: string;
  racks: number;
  songs: number;
  setlists: number;
  patterns: number;
  states: number;
  missing_plugins: RfLiveRequirement[];
  warnings: string[];
}

export interface RackGraphPosition {
  x: number;
  y: number;
}

export type RackGraphNodeKind =
  | { kind: "midi_input"; bus_id: string }
  | { kind: "audio_input"; bus_id: string }
  | { kind: "plugin"; slot_id: string }
  | { kind: "rack"; rack_id: string }
  | { kind: "midi_output"; bus_id: string }
  | { kind: "audio_output"; bus_id: string };

export interface RackGraphNode {
  id: string;
  kind: RackGraphNodeKind;
  position: RackGraphPosition;
}

export type RackGraphSignal = "midi" | "audio";

export interface RackGraphEndpoint {
  node_id: string;
  port_id: string;
}

export interface RackGraphEdge {
  id: string;
  signal: RackGraphSignal;
  source: RackGraphEndpoint;
  target: RackGraphEndpoint;
  midi_transform?: RackMidiTransform;
}

export interface RackMidiTransform {
  /** Empty means Omni. */
  source_channels: number[];
  target_channel?: number;
  note_low: number;
  note_high: number;
  transpose: number;
  notes_only: boolean;
  velocity_input_low: number;
  velocity_input_high: number;
  velocity_output_low: number;
  velocity_output_high: number;
}

export type RackGraphLabelTone =
  | "neutral"
  | "cyan"
  | "green"
  | "amber"
  | "violet"
  | "red";

export interface RackGraphLabel {
  id: string;
  text: string;
  kind: "note" | "section";
  tone: RackGraphLabelTone;
  position: RackGraphPosition;
  width: number;
  height: number;
}

export interface RackGraph {
  schema_version: number;
  nodes: RackGraphNode[];
  edges: RackGraphEdge[];
  labels?: RackGraphLabel[];
}

export interface RackDefinition {
  schema_version: number;
  id: string;
  name: string;
  enabled: boolean;
  keyboard_parts?: RackKeyboardParts;
  slots: RackSlot[];
  graph?: RackGraph;
}

export interface SongPart {
  id: string;
  name: string;
  rack_id: string;
  content?: SongPartGraph;
  /** Sequencer patterns this Part carries on stage: lane N speaks MIDI
   * channel N+1, so the Rack's channel filters route them to Slots. */
  patterns?: SongPartPatternBinding[];
}

export interface SongPartPatternBinding {
  lane: number;
  pattern_id: string;
}

export interface SongPartGraph {
  keyboard_parts?: RackKeyboardParts;
  slots: RackSlot[];
  graph: RackGraph;
}

export interface SongDefinition {
  schema_version: number;
  id: string;
  name: string;
  enabled: boolean;
  parts: SongPart[];
}

export interface SetlistEntry {
  id: string;
  song_id: string;
}

export interface SetlistDefinition {
  schema_version: number;
  id: string;
  name: string;
  enabled: boolean;
  entries: SetlistEntry[];
}

export type TrigCondition =
  | "always"
  | { cycle: { hit: number; of: number } }
  | "fill"
  | "not_fill"
  | "pre"
  | "not_pre";

export interface ParameterLockSpec {
  parameter: number;
  value: number;
}

export interface PatternNoteSpec {
  tick: number;
  duration_ticks: number;
  key: number;
  velocity: number;
  channel: number;
  /** Chance this step fires, 1..=100; rolled deterministically per pass. */
  probability?: number;
  condition?: TrigCondition;
  /** Knobs frozen into this step, fired with its note-on. */
  locks?: ParameterLockSpec[];
}

/** A sequencer pattern: a performance-library entity like a Rack or a Song,
 * edited in LIVE and launched quantised against the host transport. */
export interface PatternDefinition {
  id: string;
  name: string;
  length_ticks: number;
  notes: PatternNoteSpec[];
  /** Editor lens hint; the engine never reads it. */
  view?: "drum" | "melodic";
  /** The pattern's groove, 50 (straight) to 75 (dotted). */
  swing_percent?: number;
  /** The key the phrase was written in — what key-follow transposes from. */
  root_key?: number;
  /** Full cycles to play before the follow action fires; 0 disables it. */
  follow_after?: number;
  follow_action?: "none" | "next_slot" | "previous_slot" | "first_slot" | "any_slot" | "stop";
}

export interface PerformanceLibrary {
  schema_version: number;
  racks: RackDefinition[];
  songs: SongDefinition[];
  setlists: SetlistDefinition[];
  patterns?: PatternDefinition[];
}

export interface PerformanceSnapshot {
  schema_version: number;
  revision: string;
  library: PerformanceLibrary;
  live: LivePerformanceState;
}

export type PerformanceEdit =
  | { kind: "put_rack"; rack: RackDefinition }
  | { kind: "delete_rack"; rack_id: string }
  | { kind: "put_song"; song: SongDefinition }
  | { kind: "delete_song"; song_id: string }
  | { kind: "put_setlist"; setlist: SetlistDefinition }
  | { kind: "delete_setlist"; setlist_id: string }
  | { kind: "put_pattern"; pattern: PatternDefinition }
  | { kind: "delete_pattern"; pattern_id: string };

export interface AuditionState {
  lease_id: number;
  instance_id: string;
  previous_sound_id?: string;
}

export type ProgramEditorValue =
  | { type: "inherited" }
  | { type: "boolean"; value: boolean }
  | { type: "integer"; value: number }
  | { type: "choice"; value: string }
  | { type: "sound_id"; value: string };

export type ProgramEditorFieldKind =
  | { type: "toggle" }
  | {
      type: "number";
      minimum: number;
      maximum: number;
      step: number;
      decimals?: number;
      unit?: string;
      allow_inherited?: boolean;
    }
  | {
      type: "choice";
      options: Array<{ value: string; label: string; detail?: string }>;
    }
  | { type: "sound"; bank?: string };

export interface ProgramEditorField {
  id: string;
  label: string;
  detail: string;
  value: ProgramEditorValue;
  kind: ProgramEditorFieldKind;
  live_preview?: boolean;
}

export interface ProgramEditorPage {
  id: string;
  label: string;
  detail: string;
  enabled: boolean;
  pages?: ProgramEditorPage[];
  fields?: ProgramEditorField[];
}

export interface ProgramDraftState {
  draft_id: number;
  instance_id: string;
  original_program_id?: string;
  name: string;
  preview_sound_id: string;
  storage_path: string;
  artifacts?: Array<{
    storage_path: string;
    media_type: string;
    bytes: number[];
  }>;
  document_json: string;
  editor: {
    schema_version: number;
    title: string;
    pages: ProgramEditorPage[];
  };
  dirty: boolean;
}

export interface CoreSnapshotMessage {
  status: "snapshot";
  snapshot: SessionSnapshot;
}

export interface PerformanceSnapshotMessage {
  status: "performance_snapshot" | "performance_edited";
  snapshot: PerformanceSnapshot;
}

export interface CoreErrorMessage {
  status: "error" | "gateway_error";
  message: string;
  code?: string;
}

export interface CoreCommandAppliedMessage {
  status: "command_applied";
  client_id: string;
  command_id: number;
  revision: number;
  events: unknown[];
}

export interface WebPublicConfig {
  enabled: boolean;
  access: "local" | "lan";
  port: number;
  configurable?: boolean;
}

export interface WebAuthStatus {
  status: "ok";
  /// Whether this host decides access by PIN at all. Desktop serves the
  /// person already at the machine and answers false, so the interface can
  /// leave out a control that would do nothing there.
  pin_managed: boolean;
  requires_pin: boolean;
  unlocked: boolean;
  /// `enrolling` while an unclaimed device will still accept a chosen PIN,
  /// `unclaimed` once that window has closed, `set` once one exists.
  pin_state: "enrolling" | "unclaimed" | "set";
  pin_digits: number;
  /// Seconds before another PIN may be tried, or zero.
  locked_for: number;
}

export type PluginWebSurfaceKind = "play" | "config";

export interface PluginBranding {
  icon_url: string;
  banner_url: string;
  splash_url: string;
  background_color?: string;
  accent_color?: string;
}

export interface PluginWebDescriptor {
  plugin_id: string;
  plugin_name: string;
  version: string;
  kind: "instrument" | "effect" | "midi_processor";
  active: boolean;
  /** Host package state is stable, but its runtime is still being replaced. */
  transitioning?: boolean;
  managed: boolean;
  api_version: number;
  branding?: PluginBranding | null;
  surfaces: Array<{
    kind: PluginWebSurfaceKind;
    entry_url: string;
  }>;
  resources: PluginResourceRequirement[];
}

export interface PluginResourceRequirement {
  id: string;
  name: string;
  kind: "file" | "directory";
  required: boolean;
  data_path?: string;
  package_path?: string;
}

export interface ResourceMount {
  id: string;
  name: string;
  read_only: boolean;
}

export interface ResourceEntry {
  id: string;
  mount_id: string;
  parent_id: string | null;
  name: string;
  kind: "file" | "directory";
  size: number | null;
  modified_unix_ms: number | null;
  lazy: boolean;
  can_read: boolean;
}

export type ResourceSelectionSource = "client_upload" | "host_entry";

/**
 * Short-lived handle to a file owned by the RackForge host. The native path or
 * Android content URI intentionally never crosses the host boundary.
 */
export interface ResourceSelection {
  selection_id: string;
  display_name: string;
  kind: "file" | "directory";
  size: number | null;
  source: ResourceSelectionSource;
  expires_in_seconds: number;
}

export interface ResourceGrant {
  grant_id: string;
  resource_id: string;
  display_name: string;
  kind: "file" | "directory";
}

export interface GrantedResourceEntry {
  id: string;
  parent_id: string | null;
  name: string;
  kind: "file" | "directory";
  size: number | null;
  modified_unix_ms: number | null;
  lazy: boolean;
  can_read: boolean;
}

export type PluginParameterKind =
  | {
      type: "float";
      minimum: number;
      maximum: number;
      default: number;
      step: number;
      unit?: string;
    }
  | {
      type: "integer";
      minimum: number;
      maximum: number;
      default: number;
      step: number;
      unit?: string;
    }
  | { type: "boolean"; default: boolean }
  | {
      type: "enum";
      default: number;
      choices: Array<{ value: number; name: string }>;
    }
  | { type: "trigger" }
  | { type: "meter"; minimum: number; maximum: number; unit?: string };

export interface PluginParameterDescriptor {
  index: number;
  id: string;
  name: string;
  page: string;
  group?: string;
  order: number;
  kind: PluginParameterKind;
  flags: {
    automatable: boolean;
    modulatable: boolean;
    read_only: boolean;
    advanced: boolean;
  };
  suggested_control: string;
}

export interface PluginParameterSnapshot {
  instance_id: string;
  schema: {
    schema_version: number;
    pages: Array<{
      id: string;
      name: string;
      order: number;
      header?: string;
    }>;
    parameters: PluginParameterDescriptor[];
  };
  values: Array<{ index: number; value: number }>;
}

export interface PluginStateParameterSnapshot {
  state: PluginStateReference;
  schema: PluginParameterSnapshot["schema"];
  values: Array<{ index: number; value: number }>;
}

export interface PluginStateParameterResult {
  state: PluginStateReference;
  parameter_index: number;
  value: number;
}

export interface SessionCommand {
  type: string;
  [key: string]: unknown;
}
