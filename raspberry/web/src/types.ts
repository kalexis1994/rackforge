export type ConnectionStatus = "connecting" | "online" | "offline";

export interface SoundSummary {
  id: string;
  name: string;
  bank?: string;
  detail?: string;
}

export interface PluginInstance {
  instance_id: string;
  plugin_id: string;
  plugin_name: string;
  ui_layouts: string[];
  sounds: SoundSummary[];
  selected_sound_id?: string;
}

export interface SessionSnapshot {
  schema_version: number;
  session_id: string;
  revision: number;
  active_mode: "live" | "play";
  master_level: number;
  master_pan: number;
  active_instance_id?: string;
  instances: PluginInstance[];
  audition?: AuditionState;
  program_draft?: ProgramDraftState;
}

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

export interface CoreErrorMessage {
  status: "error" | "gateway_error";
  message: string;
}

export interface WebPublicConfig {
  enabled: boolean;
  access: "local" | "lan";
  port: number;
}

export interface WebAuthStatus {
  status: "ok";
  requires_pairing: boolean;
  paired: boolean;
  pairing_active: boolean;
}

export type PluginWebSurfaceKind = "play" | "config";

export interface PluginWebDescriptor {
  plugin_id: string;
  plugin_name: string;
  version: string;
  api_version: number;
  surfaces: Array<{
    kind: PluginWebSurfaceKind;
    entry_url: string;
  }>;
}

export interface SessionCommand {
  type: string;
  [key: string]: unknown;
}
