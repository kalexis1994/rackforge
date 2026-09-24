import type { VelocityCurve } from "./velocityCurve";

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
  /** Browser host only: whether the multicore render pool is actually up. */
  render_pool?: {
    isolated: boolean;
    workers: number;
    missed_blocks: number;
    reason?: string;
  };
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
  /** The reading for a keybed with none of its own. Absent means untouched. */
  velocity_curve?: VelocityCurve;
  /** And the reading each keybed that has one gets, by port name. */
  velocity_curves?: Record<string, VelocityCurve>;
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
  /** Each port's identity, so a strike can be matched to the port it came
      from without the interface knowing how the identity is derived. */
  midi_source_keys?: Record<string, number>;
  runtime?: HostAudioRuntimeStatus;
  runtime_status: string;
  /** The settings window of the driver in use, when the host can show one.
      Absent on hosts that have none. */
  driver_panel?: HostAudioDriverPanel | null;
}

export interface HostAudioDriverPanel {
  /** `asio`: the driver's own window. `system_sound`: the OS sound settings. */
  kind: "asio" | "system_sound";
  available: boolean;
  /** Why it cannot open, when it cannot. */
  detail?: string | null;
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
  /** One chain per instrument that has one; the host routes PLAY through it. */
  play_chains?: PlayChainState[];
}

/** One effect in an instrument's PLAY chain; `id` is unique within the chain. */
export interface PlayChainEffect {
  id: string;
  plugin_id: string;
  enabled: boolean;
  /** The program this effect is on, chosen from its panel in the chain. */
  program_id?: string | null;
}

/** The effects after one instrument in PLAY, in order. */
export interface PlayChainState {
  instrument_id: string;
  effects: PlayChainEffect[];
}

export interface OutputMeterSnapshot {
  left_peak: number;
  right_peak: number;
}

export interface OutputMeterMessage {
  status: "output_meter";
  meter: OutputMeterSnapshot;
}

export interface AudioHealthSnapshot {
  /** Since the previous poll, not since the stream opened. */
  load_percent: number;
  peak_percent: number;
  overruns: number;
  stream_errors: number;
  midi_dropped: number;
  recent_overruns: number;
  overrun_average_percent: number;
  overrun_average_frames: number;
  block_frames: number;
  late_callbacks: number;
  recent_late_callbacks: number;
  /** Widest gap between two callbacks in the window; 100 is on time. */
  worst_gap_percent: number;
  silenced_blocks: number;
  recent_silenced_blocks: number;
  /** What the ASIO driver itself reports losing; zero on WASAPI. */
  driver_overloads: number;
  driver_resyncs: number;
  driver_skipped_buffers: number;
  recent_driver_dropouts: number;
  capture_glitches: number;
  recent_capture_glitches: number;
  /** MIDI held by the operating system before it reached the host. */
  midi_late_driver: number;
  /** MIDI the host had and no audio block took in time. */
  midi_late_queue: number;
  recent_midi_late: number;
  worst_midi_driver_delay_ms: number;
  worst_midi_queue_delay_ms: number;
}

export interface AudioHealthMessage {
  status: "audio_health";
  health: AudioHealthSnapshot;
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
  /** Left out, the control drives the parameter across its whole range. */
  mode?: ParameterLinkMode;
}

/**
 * What a control does to its parameter. Values are in the parameter's own
 * units: a choice's value, a boolean's 0 or 1, a float within its range.
 * Knobs and faders use direct, range or zones; buttons and pads the rest.
 */
export type ParameterLinkMode =
  | { kind: "direct" }
  | { kind: "range"; min: number; max: number }
  | { kind: "zones"; values: number[] }
  | { kind: "set"; value: number }
  | { kind: "toggle"; first: number; second: number }
  | { kind: "cycle"; values: number[] }
  | { kind: "hold"; pressed: number; released: number }
  | { kind: "step"; direction: "up" | "down"; wrap?: boolean }
  | { kind: "trigger" };

/** One input of a controller driving one plugin parameter, by its id. */
export interface ControlMapping {
  id: string;
  input: {
    id: string;
    name: string;
    channel: ParameterLink["channel"];
    message: ParameterLinkMessage;
  };
  parameter_id: string;
  mode?: ParameterLinkMode;
  invert?: boolean;
  /** Left out, a button consumes its message and a knob passes it on. */
  pass_through?: "pass_through" | "consume";
}

/** Every mapping a player made for one controller, plugin by plugin. */
export interface ControllerMap {
  schema_version: 1;
  controller_id: string;
  controller_name: string;
  plugins: { plugin_id: string; plugin_name: string; mappings: ControlMapping[] }[];
}

/** One channel message the host received, numbered in arrival order. */
export interface MidiActivityEvent {
  sequence: number;
  source: MidiSourceDescriptor;
  status: number;
  data1: number;
  data2: number;
}

/** A controller package the host attached to a MIDI input. */
export interface RegisteredController {
  controller_id: string;
  source?: MidiSourceDescriptor;
  connected: boolean;
  /** The device's Identity Reply chose the package, not its port name alone. */
  identified?: boolean;
}

/** The portable `.rfmap` document. */
export interface RfMapFile {
  format: "org.rackforge.map";
  schema_version: 1;
  exported_by: string;
  exported_unix_ms: number;
  map: ControllerMap;
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
  library: PerformanceLibrary;
  states: unknown[];
  requirements: RfLiveRequirement[];
  /** The transport as it stood at export: the show's tempo and meter. */
  tempo_bpm?: number;
  beats_per_bar?: number;
  beat_unit?: number;
  /** Where the artist stood; the importing surface reactivates it. */
  live?: LivePerformanceState;
}

export interface RfLiveImportPreview {
  name: string;
  racks: number;
  songs: number;
  setlists: number;
  patterns: number;
  states: number;
  tabs?: number;
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
  /** On a cable from the audio input only: which inputs it carries and at
   *  what trim. Absent, it carries everything the host captures. */
  audio_input_route?: RackAudioInputRoute;
}

/** A cable's share of the hardware audio input. */
export interface RackAudioInputRoute {
  /** Physical inputs, one-based. Empty or absent: every captured input. One:
   *  a mono source, on both sides of a stereo plugin. Two: a stereo pair,
   *  left then right. */
  channels?: number[];
  /** Trim on this cable, in dB, -60 to +24, after the host's input trim. */
  gain_db?: number;
}

/** Whether the host is listening to an audio input. */
export type AudioInputAvailability = "open" | "disabled" | "absent" | "unsupported";

/** What the host captures, for the Rack editor. `peaks` is drained by each
 *  request. */
export interface AudioInputStatus {
  availability: AudioInputAvailability;
  device_name?: string;
  /** Inputs the interface has, numbered from 1; 0 when unknown. */
  device_channels: number;
  /** Inputs captured, one-based, in capture order. */
  captured: number[];
  gain_db: number;
  /** Whether the host honours each cable's own inputs and trim. */
  cable_routing: boolean;
  /** Linear peaks since the previous request, one per captured input. */
  peaks: number[];
  reason?: string;
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

/** One sequencer of the tabbed deck, keyed by engine lane. Library-owned
 * so every host shows the same deck and a show file carries it. */
export interface SequencerTabDefinition {
  lane: number;
  view?: "drum" | "melodic";
  slot_ids?: (string | null)[];
  active_slot?: number;
}

export interface PerformanceLibrary {
  schema_version: number;
  racks: RackDefinition[];
  songs: SongDefinition[];
  setlists: SetlistDefinition[];
  patterns?: PatternDefinition[];
  sequencer_tabs?: SequencerTabDefinition[];
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
  | { kind: "delete_pattern"; pattern_id: string }
  | { kind: "put_sequencer_tab"; tab: SequencerTabDefinition }
  | { kind: "delete_sequencer_tab"; lane: number };

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
  /** The package declares a UI, but the browser cannot currently serve it. */
  web_ui_unavailable_reason?: string;
  surfaces: Array<{
    kind: PluginWebSurfaceKind;
    entry_url: string;
  }>;
  resources: PluginResourceRequirement[];
  /** The effects the instrument suggests after itself in the PLAY chain. */
  suggested_chain?: Array<{ plugin: string; preset?: string | null }>;
  /** An effect played on its own from the audio input (a pedalboard): PLAY
   *  offers it beside the instruments. */
  play_source?: boolean;
  /**
   * The host builds this effect's voice on demand, out of the store, so the
   * chain can take it without the session having loaded it first. Hosts that
   * load every plugin up front leave it unset and are read by their instances.
   */
  chainable?: boolean;
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
      /** How the travel is spread: each step adds, or each step multiplies. */
      taper?: "linear" | "logarithmic";
    }
  | {
      type: "integer";
      minimum: number;
      maximum: number;
      default: number;
      step: number;
      unit?: string;
      taper?: "linear" | "logarithmic";
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
