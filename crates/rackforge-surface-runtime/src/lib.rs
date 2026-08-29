//! RackForge-owned LITTLE surface state and plugin navigation.
//!
//! This crate has no MIDI, SysEx, USB or controller-model knowledge.

use rackforge_audio_api::{
    AudioDeviceDescriptor, AudioDeviceSelector, AudioFallbackPolicy, AudioOutputProfile,
    AudioOutputState, AudioSampleFormat,
};
use rackforge_controller_api::LITTLE_TEXT_COLUMNS;
use rackforge_performance_api::{
    LibraryRevision, LiveBrowseMode, LiveLocation, LivePerformanceState, MidiOutputRoute,
    PERFORMANCE_SCHEMA_VERSION, PerformanceEdit, PerformanceLibrary, PerformanceSnapshot,
    RackDefinition, RackId, RackKeyboardPart, RackKeyboardParts, RackSlot, RackSlotId,
    SetlistDefinition, SetlistEntry, SetlistEntryId, SetlistId, SongDefinition, SongId, SongPart,
    SongPartId,
};
use rackforge_plugin_api::{ParameterDescriptor, ParameterKind, ParameterSchema};
use rackforge_program_api::{
    ProgramEditorField, ProgramEditorFieldKind, ProgramEditorPage, ProgramEditorValue,
};
use rackforge_session_api::ProgramDraftState;
pub use rackforge_ui::Input;
use rackforge_ui::{
    Component, ComponentEvent, Frame, NavigationAction as Action, Rect, TextFallback, VisualState,
    components::{
        Button, CarouselItem, ConfirmationDialog, EditableValue, SecretEditor, SecretValue,
        SimpleCarousel, Spinner, TextEditor, ValueCarousel, ValueItem,
    },
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const DISPLAY_COLUMNS: usize = LITTLE_TEXT_COLUMNS;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Header {
    Visible(String),
    /// Host-owned plugin identity plus page-local context. Keeping these as
    /// separate values lets every controller project the same LITTLE model to
    /// its own viewport instead of forcing an 18-column layout on all screens.
    Plugin {
        name: String,
        short_name: String,
        context: String,
    },
    Hidden,
}

impl Header {
    /// Projects a semantic header into a character-cell viewport.
    pub fn text(&self, columns: usize) -> Option<String> {
        match self {
            Self::Visible(text) => Some(fit_surface_text(text, columns)),
            Self::Plugin {
                name,
                short_name,
                context,
            } => Some(responsive_plugin_header(name, short_name, context, columns)),
            Self::Hidden => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FooterButton {
    pub label: String,
    pub state: VisualState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Screen {
    pub header: Header,
    pub line_1: String,
    pub line_2: String,
    pub footer: [FooterButton; 4],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenRegions {
    pub header: bool,
    pub body: bool,
    pub footer: bool,
}

impl ScreenRegions {
    pub const fn full() -> Self {
        Self {
            header: true,
            body: true,
            footer: true,
        }
    }

    pub const fn header() -> Self {
        Self {
            header: true,
            body: false,
            footer: false,
        }
    }

    pub const fn is_empty(self) -> bool {
        !self.header && !self.body && !self.footer
    }

    fn union(self, other: Self) -> Self {
        Self {
            header: self.header || other.header,
            body: self.body || other.body,
            footer: self.footer || other.footer,
        }
    }

    fn between(previous: Option<&Screen>, next: &Screen) -> Self {
        let Some(previous) = previous else {
            return Self::full();
        };
        Self {
            header: previous.header != next.header,
            body: previous.line_1 != next.line_1 || previous.line_2 != next.line_2,
            footer: previous.footer != next.footer,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum SurfaceUpdatePriority {
    #[default]
    Background,
    Interactive,
    Immediate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenUpdate {
    pub revision: u64,
    pub screen: Screen,
    pub changed: ScreenRegions,
    pub priority: SurfaceUpdatePriority,
}

/// Latest-wins compositor shared by every LITTLE transport.
///
/// Producers only publish semantic screens. A transport takes the most recent
/// revision when it is ready, so a slow OLED never builds a queue of obsolete
/// animations. Delivery is tracked separately from desired state: reconnects
/// invalidate delivery and obtain one complete authoritative snapshot.
#[derive(Clone, Debug, Default)]
pub struct ScreenCompositor {
    desired: Option<Screen>,
    delivered: Option<Screen>,
    invalidated: ScreenRegions,
    revision: u64,
    pending_revision: Option<u64>,
    pending_priority: SurfaceUpdatePriority,
}

impl ScreenCompositor {
    pub fn publish(&mut self, screen: Screen, priority: SurfaceUpdatePriority) -> bool {
        if self.desired.as_ref() == Some(&screen) {
            // Re-publishing the same desired pixels must not create another
            // revision, but an interaction may legitimately promote an
            // already-pending background refresh.
            if self.pending_revision.is_some() {
                self.pending_priority = self.pending_priority.max(priority);
            }
            return false;
        }
        self.desired = Some(screen);
        if self.desired == self.delivered && self.invalidated.is_empty() {
            self.pending_revision = None;
            self.pending_priority = SurfaceUpdatePriority::Background;
            return false;
        }
        self.queue(priority);
        true
    }

    pub fn invalidate(&mut self, regions: ScreenRegions, priority: SurfaceUpdatePriority) {
        if regions.is_empty() || self.desired.is_none() {
            return;
        }
        self.invalidated = self.invalidated.union(regions);
        self.queue(priority);
    }

    pub fn invalidate_delivery(&mut self) {
        self.delivered = None;
        self.invalidated = ScreenRegions::full();
        if self.desired.is_some() {
            self.queue(SurfaceUpdatePriority::Immediate);
        }
    }

    pub fn take(&mut self) -> Option<ScreenUpdate> {
        let revision = self.pending_revision.take()?;
        let screen = self.desired.clone()?;
        let changed =
            ScreenRegions::between(self.delivered.as_ref(), &screen).union(self.invalidated);
        self.invalidated = ScreenRegions::default();
        let priority = std::mem::take(&mut self.pending_priority);
        if changed.is_empty() {
            return None;
        }
        self.delivered = Some(screen.clone());
        Some(ScreenUpdate {
            revision,
            screen,
            changed,
            priority,
        })
    }

    pub fn desired(&self) -> Option<&Screen> {
        self.desired.as_ref()
    }

    fn queue(&mut self, priority: SurfaceUpdatePriority) {
        self.revision = self.revision.saturating_add(1);
        self.pending_revision = Some(self.revision);
        self.pending_priority = self.pending_priority.max(priority);
    }
}

/// Lock-bounded latest-screen handoff for UI and controller transport threads.
/// It never waits for hardware and never allocates an unbounded message queue.
#[derive(Clone, Debug, Default)]
pub struct ScreenMailbox {
    compositor: Arc<Mutex<ScreenCompositor>>,
}

impl ScreenMailbox {
    pub fn publish(&self, screen: Screen, priority: SurfaceUpdatePriority) -> bool {
        self.with_compositor(|compositor| compositor.publish(screen, priority))
    }

    pub fn take(&self) -> Option<ScreenUpdate> {
        self.with_compositor(ScreenCompositor::take)
    }

    pub fn invalidate_delivery(&self) {
        self.with_compositor(ScreenCompositor::invalidate_delivery);
    }

    fn with_compositor<T>(&self, operation: impl FnOnce(&mut ScreenCompositor) -> T) -> T {
        // A UI-side panic must not permanently disable the controller surface.
        // The compositor contains only replaceable presentation state, so
        // recovering the poisoned guard is safer than taking down audio/UI.
        let mut compositor = self
            .compositor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        operation(&mut compositor)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaySound {
    pub id: String,
    pub name: String,
    pub bank: String,
    pub detail: String,
    pub editable: bool,
}

/// One RackForge-owned preset available for the active plugin.
///
/// This is intentionally separate from [`PlaySound`]: sounds are native
/// plugin PROGRAMS, while these presets are portable snapshots owned by the
/// host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayPreset {
    pub id: String,
    pub name: String,
    pub detail: String,
}

impl PlayPreset {
    pub fn new(id: impl Into<String>, name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: normalized_display_text(&name.into(), "PRESET"),
            detail: normalized_display_text(&detail.into(), "RACKFORGE PRESET"),
        }
    }
}

impl PlaySound {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        bank: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: normalized_display_text(&name.into(), "UNNAMED").to_ascii_uppercase(),
            bank: bank.into(),
            detail: normalized_display_text(&detail.into(), " "),
            editable: false,
        }
    }

    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayPlugin {
    pub instance_id: String,
    pub plugin_id: String,
    pub full_name: String,
    pub name: String,
    /// Compact identity used by constrained controller displays. The surface
    /// owns the actual layout; this is only the plugin-provided fallback.
    pub short_name: String,
    pub config_available: bool,
}

impl PlayPlugin {
    pub fn new(
        instance_id: impl Into<String>,
        plugin_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Self {
        let full_name = clean_surface_text(&name.into(), "PLUGIN");
        let name = normalized_display_text(&full_name, "PLUGIN");
        Self {
            instance_id: instance_id.into(),
            plugin_id: plugin_id.into(),
            short_name: compact_plugin_name(&name),
            full_name,
            name,
            // Preserve the behavior of callers compiled against the original
            // constructor. Runtime catalogs always override this from the
            // plugin manifest.
            config_available: true,
        }
    }

    pub fn config_available(mut self, available: bool) -> Self {
        self.config_available = available;
        self
    }

    pub fn short_name(mut self, short_name: impl AsRef<str>) -> Self {
        self.short_name = compact_plugin_name(short_name.as_ref());
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveMode {
    Idle,
    Live,
    Play,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebAccess {
    Local,
    Lan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSystemSettings {
    pub enabled: bool,
    pub access: WebAccess,
    pub port: u16,
    pub lan_ip: Option<[u8; 4]>,
    pub service_online: bool,
    /// Whether an access PIN has been chosen yet.
    ///
    /// The controller reports this and does not offer to change it. A PIN is
    /// entered in a browser, and a device that showed its own PIN on a screen
    /// would be back to needing a screen — which was the whole reason the code
    /// on this display stopped being how access works.
    pub pin_set: bool,
}

impl Default for WebSystemSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            access: WebAccess::Local,
            port: 8787,
            lan_ip: None,
            service_online: false,
            pin_set: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedWifiNetwork {
    pub id: String,
    pub name: String,
    pub ssid: Option<String>,
    pub active: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WifiSystemSettings {
    pub available: bool,
    pub enabled: bool,
    pub connected: bool,
    pub ssid: Option<String>,
    pub signal_percent: Option<u8>,
    pub saved_networks: Vec<SavedWifiNetwork>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredWifiNetwork {
    pub ssid: String,
    pub signal_percent: u8,
    pub secured: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramExitDestination {
    CustomPrograms,
    ActiveMode {
        mode: ActiveMode,
        selected_sound_id: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramExitDecision {
    Save,
    Discard,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuCommand {
    SetActiveMode {
        mode: ActiveMode,
    },
    SelectPlugin {
        instance_id: String,
    },
    SetLiveBrowseMode {
        mode: LiveBrowseMode,
    },
    ActivateLiveTarget {
        location: LiveLocation,
    },
    PreviewRack {
        rack: RackDefinition,
    },
    EditPerformance {
        expected_revision: LibraryRevision,
        edit: PerformanceEdit,
    },
    SelectSound {
        id: String,
    },
    LoadPluginPreset {
        preset_id: String,
    },
    SavePluginPreset {
        name: String,
    },
    SetPluginParameter {
        instance_id: String,
        parameter_index: u32,
        value: f64,
    },
    TriggerPluginParameter {
        instance_id: String,
        parameter_index: u32,
    },
    BeginProgramEdit {
        program_id: Option<String>,
    },
    EditProgramDraftField {
        draft_id: u64,
        field_id: String,
        value: ProgramEditorValue,
        preview: bool,
    },
    RestoreProgramDraftPreview {
        draft_id: u64,
    },
    SetProgramDraftName {
        draft_id: u64,
        name: String,
    },
    SaveProgramDraft {
        draft_id: u64,
    },
    CancelProgramEdit {
        draft_id: u64,
    },
    ResolveProgramExit {
        draft_id: u64,
        decision: ProgramExitDecision,
        destination: ProgramExitDestination,
    },
    ReturnToActiveMode {
        mode: ActiveMode,
        cancel_draft_id: Option<u64>,
        selected_sound_id: Option<String>,
    },
    ForceHome,
    SetWebEnabled {
        enabled: bool,
    },
    SetWebAccess {
        access: WebAccess,
    },
    SetWebPort {
        port: u16,
    },
    ActivateSavedWifi {
        connection_id: String,
    },
    ForgetSavedWifi {
        connection_id: String,
    },
    ConnectDiscoveredWifi {
        ssid: String,
        passphrase: Option<SecretValue>,
    },
    DisconnectWifi,
    SetWifiEnabled {
        enabled: bool,
    },
    ScanWifi,
    ApplyAudioOutput {
        profile: AudioOutputProfile,
    },
}

impl Screen {
    fn with_header(
        header: impl Into<String>,
        line_1: impl Into<String>,
        line_2: impl Into<String>,
    ) -> Self {
        let screen = Self {
            header: Header::Visible(header.into()),
            line_1: line_1.into(),
            line_2: line_2.into(),
            footer: standard_footer(None),
        };
        debug_assert!(screen.is_valid());
        screen
    }

    fn with_plugin_header(
        name: impl Into<String>,
        short_name: impl Into<String>,
        context: impl Into<String>,
        line_1: impl Into<String>,
        line_2: impl Into<String>,
    ) -> Self {
        let screen = Self {
            header: Header::Plugin {
                name: clean_surface_text(&name.into(), "PLUGIN"),
                short_name: compact_plugin_name(&short_name.into()),
                context: clean_surface_text(&context.into(), " ").to_ascii_uppercase(),
            },
            line_1: line_1.into(),
            line_2: line_2.into(),
            footer: standard_footer(None),
        };
        debug_assert!(screen.is_valid());
        screen
    }

    pub fn is_valid(&self) -> bool {
        let header_is_valid = match &self.header {
            Header::Visible(header) => valid_line(header),
            Header::Plugin {
                name,
                short_name,
                context,
            } => [name, short_name, context]
                .into_iter()
                .all(|value| valid_semantic_text(value)),
            Header::Hidden => true,
        };
        header_is_valid
            && [&self.line_1, &self.line_2]
                .into_iter()
                .all(|line| valid_line(line))
            && self
                .footer
                .iter()
                .all(|button| valid_footer_label(&button.label))
    }
}

fn valid_semantic_text(value: &str) -> bool {
    !value.is_empty() && value.is_ascii() && !value.as_bytes().contains(&0)
}

fn valid_line(line: &str) -> bool {
    !line.is_empty()
        && line.len() <= DISPLAY_COLUMNS
        && line.is_ascii()
        && !line.as_bytes().contains(&0)
}

fn valid_footer_label(label: &str) -> bool {
    !label.is_empty() && label.len() <= 7 && label.is_ascii() && !label.as_bytes().contains(&0)
}

fn standard_footer(pressed: Option<usize>) -> [FooterButton; 4] {
    let labels = ["OK", "<", ">", "BACK"];
    std::array::from_fn(|index| FooterButton {
        label: labels[index].into(),
        state: if pressed == Some(index) {
            VisualState::Pressed
        } else {
            VisualState::Normal
        },
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Page {
    Home,
    Live,
    LiveRacks,
    LiveSongs,
    LiveSongParts,
    LiveSetlists,
    LiveSetlistEntries,
    Play,
    Config,
    ConfigRacks,
    ConfigRackEditor,
    ConfigRackName,
    ConfigRackSlots,
    ConfigRackSlotEditor,
    ConfigRackSlotName,
    ConfigRackSlotPlugin,
    ConfigRackSlotMidiChannel,
    ConfigRackSlotLowKey,
    ConfigRackSlotHighKey,
    ConfigRackSlotOctave,
    ConfigRackSlotMidiOutput,
    ConfigRackSlotAudioOutput,
    ConfigRackSlotLevel,
    ConfigRackSlotPan,
    ConfigSongs,
    ConfigSongEditor,
    ConfigSongName,
    ConfigSongParts,
    ConfigSongPartEditor,
    ConfigSongPartName,
    ConfigSongPartRack,
    ConfigSetlists,
    ConfigSetlistEditor,
    ConfigSetlistName,
    ConfigSetlistEntries,
    ConfigSetlistEntryEditor,
    ConfigSetlistEntrySong,
    PerformanceUnsaved,
    PerformanceDelete,
    PerformanceBusy,
    PerformanceResult,
    KeyboardParts,
    Plugins,
    PluginConfigUnavailable,
    System,
    Audio,
    AudioOutput,
    AudioRate,
    AudioLatency,
    AudioBusy,
    AudioResult,
    SystemWeb,
    SystemWifi,
    SystemWifiNetworks,
    SystemWifiKnown,
    SystemWifiKnownActions,
    SystemWifiDiscovered,
    SystemWifiDiscoveredActions,
    SystemWifiPassword,
    SystemWifiBusy,
    SystemWifiResult,
    PluginLibrary,
    PluginPresets,
    PluginPresetName,
    PluginPresetBusy,
    PluginPresetResult,
    PluginPrograms,
    PluginPlay,
    PluginParameterPages,
    PluginParameters,
    PluginParameterValue,
    PluginCustomPrograms,
    PluginProgramSections,
    PluginName,
    PluginLayerMenu,
    PluginTimbre,
    PluginEnvelope,
    PluginPitchEnvelope,
    PluginRange,
    PluginLfo,
    PluginTuning,
    PluginLayerLevel,
    PluginSharedFx,
    PluginProgramOutput,
    PluginUnsavedChanges,
    ProgramEditorRoot,
    ProgramEditorSection,
    ProgramEditorField,
    ProgramEditorSound,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum PluginPlayContext {
    #[default]
    Standalone,
    RackSlot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingProgramExit {
    return_page: Page,
    destination: ProgramExitDestination,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PerformanceDraft {
    Rack(RackDefinition),
    Song(SongDefinition),
    Setlist(SetlistDefinition),
}

impl PerformanceDraft {
    fn name(&self) -> &str {
        match self {
            Self::Rack(value) => &value.name,
            Self::Song(value) => &value.name,
            Self::Setlist(value) => &value.name,
        }
    }

    fn enabled(&self) -> bool {
        match self {
            Self::Rack(value) => value.enabled,
            Self::Song(value) => value.enabled,
            Self::Setlist(value) => value.enabled,
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::Rack(value) => value.enabled = enabled,
            Self::Song(value) => value.enabled = enabled,
            Self::Setlist(value) => value.enabled = enabled,
        }
    }

    fn into_edit(self) -> PerformanceEdit {
        match self {
            Self::Rack(rack) => PerformanceEdit::PutRack { rack },
            Self::Song(song) => PerformanceEdit::PutSong { song },
            Self::Setlist(setlist) => PerformanceEdit::PutSetlist { setlist },
        }
    }
}

/// Where the player's cursor was standing in the LIVE lists, held by
/// identity so it can be found again after the library changes underneath.
#[derive(Debug)]
struct BrowsedLive {
    mode_index: usize,
    rack: Option<rackforge_performance_api::RackId>,
    song: Option<rackforge_performance_api::SongId>,
    song_part: Option<rackforge_performance_api::SongPartId>,
    setlist: Option<rackforge_performance_api::SetlistId>,
    setlist_entry: Option<(
        rackforge_performance_api::SetlistEntryId,
        rackforge_performance_api::SongPartId,
    )>,
}

#[derive(Debug)]
pub struct Menu {
    page: Page,
    active_mode: ActiveMode,
    home_index: usize,
    live_index: usize,
    live_rack_index: usize,
    live_song_index: usize,
    live_song_part_index: usize,
    live_setlist_index: usize,
    live_setlist_entry_index: usize,
    performance_library: PerformanceLibrary,
    performance_revision: LibraryRevision,
    live_performance: LivePerformanceState,
    performance_list_index: usize,
    performance_editor_index: usize,
    performance_child_index: usize,
    performance_child_editor_index: usize,
    performance_value_index: usize,
    performance_draft: Option<PerformanceDraft>,
    performance_dirty: bool,
    performance_name: TextEditor,
    performance_child_name: TextEditor,
    performance_confirm: ConfirmationDialog,
    performance_spinner: Spinner,
    performance_result: Option<(bool, String)>,
    performance_return_page: Page,
    keyboard_parts_index: usize,
    keyboard_parts_return_page: Page,
    keyboard_parts_editing: bool,
    keyboard_split_quick_action: bool,
    play_index: usize,
    play_plugins: Vec<PlayPlugin>,
    config_index: usize,
    plugin_index: usize,
    active_plugin_instance_id: Option<String>,
    pending_plugin_instance_id: Option<String>,
    active_plugin_id: String,
    active_plugin_full_name: String,
    active_plugin_name: String,
    active_plugin_short_name: String,
    active_plugin_config_available: bool,
    plugin_spinner: Spinner,
    system_index: usize,
    system_web_index: usize,
    web_settings: WebSystemSettings,
    web_edit_candidate: WebSystemSettings,
    system_web_editing: bool,
    wifi_settings: WifiSystemSettings,
    system_wifi_index: usize,
    wifi_networks_index: usize,
    wifi_saved_index: usize,
    wifi_known_action_index: usize,
    wifi_discovered_networks: Vec<DiscoveredWifiNetwork>,
    wifi_discovered_index: usize,
    wifi_radio_editing: bool,
    wifi_radio_candidate: bool,
    wifi_password: SecretEditor,
    wifi_spinner: Spinner,
    wifi_result: Option<(bool, String)>,
    audio_state: Option<AudioOutputState>,
    audio_index: usize,
    audio_value_index: usize,
    audio_spinner: Spinner,
    audio_result: Option<(bool, String)>,
    plugin_library_index: usize,
    plugin_preset_index: usize,
    plugin_program_bank_index: usize,
    plugin_parameter_page_index: usize,
    plugin_parameter_index: usize,
    plugin_parameter_schema: Option<ParameterSchema>,
    plugin_parameter_values: BTreeMap<u32, f64>,
    plugin_parameter_editor: Option<ValueCarousel>,
    plugin_presets: Vec<PlayPreset>,
    plugin_active_preset_id: Option<String>,
    plugin_preset_name: TextEditor,
    plugin_preset_result: Option<(bool, String)>,
    plugin_play_index: usize,
    plugin_play_context: PluginPlayContext,
    plugin_custom_index: usize,
    plugin_section_index: usize,
    plugin_layer_index: usize,
    plugin_layer_option_index: usize,
    plugin_timbre_index: usize,
    plugin_sounds: Vec<PlaySound>,
    plugin_active_sound_id: Option<String>,
    play_anchor_sound_id: Option<String>,
    audition_lease_id: Option<u64>,
    program_draft: Option<ProgramDraftState>,
    program_name: TextEditor,
    envelope: ValueCarousel,
    pitch_envelope: ValueCarousel,
    lfo: ValueCarousel,
    tuning: ValueCarousel,
    range: ValueCarousel,
    layer_level: ValueCarousel,
    program_output: ValueCarousel,
    unsaved_changes: ConfirmationDialog,
    pending_program_exit: Option<PendingProgramExit>,
    editor_path: Vec<usize>,
    editor_selections: Vec<usize>,
    editor_field: Option<ValueCarousel>,
    editor_field_id: Option<String>,
    pressed_button: Option<usize>,
    pending_command: Option<MenuCommand>,
}

const HOME_ITEMS: [&str; 3] = ["LIVE", "PLAY", "CONFIG"];
const HOME_HEADER: &str = "RACKFORGE";
const LIVE_ITEMS: [&str; 3] = ["RACK", "SONG", "SETLIST"];
const LIVE_DETAILS: [&str; 3] = [
    "All configured Racks",
    "Songs and parts",
    "Performance order",
];
const CONFIG_ITEMS: [&str; 6] = ["RACKS", "SONGS", "SETLISTS", "PLUGINS", "AUDIO", "SYSTEM"];
const CONFIG_DETAILS: [&str; 6] = [
    "Playable Racks",
    "Arrange Parts",
    "Performance order",
    "Plugin settings",
    "Output settings",
    "RackForge settings",
];
const RACK_EDITOR_ITEMS: [&str; 5] = ["NAME", "SLOTS", "ENABLED", "SAVE", "DELETE"];
const RACK_SLOT_EDITOR_ITEMS: [&str; 12] = [
    "NAME",
    "PLUGIN",
    "MIDI CH",
    "LOW KEY",
    "HIGH KEY",
    "OCTAVE",
    "MIDI OUT",
    "AUDIO OUT",
    "LEVEL",
    "PAN",
    "ENABLED",
    "DELETE",
];
const KEYBOARD_PART_EDITOR_ITEMS: [&str; 3] = ["MIDI CH", "SPLIT KEY", "OCTAVE"];
const SONG_EDITOR_ITEMS: [&str; 5] = ["NAME", "PARTS", "ENABLED", "SAVE", "DELETE"];
const SETLIST_EDITOR_ITEMS: [&str; 5] = ["NAME", "SONGS", "ENABLED", "SAVE", "DELETE"];
const PART_EDITOR_ITEMS: [&str; 5] = ["NAME", "RACK", "MOVE LEFT", "MOVE RIGHT", "DELETE"];
const ENTRY_EDITOR_ITEMS: [&str; 4] = ["SONG", "MOVE LEFT", "MOVE RIGHT", "DELETE"];
const SYSTEM_WEB_ITEM: (&str, &str) = ("WEB INTERFACE", "Browser access");
const SYSTEM_WIFI_ITEM: (&str, &str) = ("WI-FI", "Wireless network");
const SYSTEM_WEB_ITEMS: [&str; 6] = [
    "ENABLED",
    "ACCESS",
    "ADDRESS",
    "PORT",
    "ACCESS PIN",
    "STATUS",
];
const SYSTEM_WIFI_ITEMS: [&str; 3] = ["STATUS", "NETWORKS", "RADIO"];
const AUDIO_ITEMS: [&str; 3] = ["OUTPUT", "SAMPLE RATE", "LATENCY"];
const AUDIO_LATENCIES: [(&str, u32, u32); 4] = [
    ("ULTRA 2 MS", 32, 96),
    ("LOW 4 MS", 64, 192),
    ("BALANCED 8 MS", 128, 384),
    ("SAFE 16 MS", 256, 768),
];
const WIFI_NETWORK_GROUPS: [&str; 2] = ["KNOWN", "DISCOVERED"];
const WIFI_KNOWN_ACTIONS: [&str; 2] = ["CONNECT", "FORGET"];
const WIFI_ACTIVE_ACTIONS: [&str; 2] = ["DISCONNECT", "FORGET"];
const PLUGIN_PROGRAM_SECTIONS: [&str; 6] = ["NAME", "LAYER A", "LAYER B", "FX", "OUTPUT", "SAVE"];
const PLUGIN_SECTION_DETAILS: [&str; 6] = [
    "Program name",
    "Required layer",
    "Optional layer",
    "Shared FX chain",
    "Final program gain",
    "Store program",
];
const PLUGIN_LAYER_SECTIONS: [&str; 7] = [
    "TIMBRE",
    "AMP ENV",
    "PITCH ENV",
    "LFO",
    "TUNING",
    "RANGE",
    "VOLUME",
];
const PLUGIN_LAYER_DETAILS: [&str; 7] = [
    "Sound source",
    "Amplitude ADSR",
    "Pitch EG override",
    "Rate delay depth",
    "Pitch and fine tune",
    "Key and velocity",
    "Layer mix gain",
];

impl Default for Menu {
    fn default() -> Self {
        Self {
            page: Page::Home,
            active_mode: ActiveMode::Live,
            home_index: 0,
            live_index: 0,
            live_rack_index: 0,
            live_song_index: 0,
            live_song_part_index: 0,
            live_setlist_index: 0,
            live_setlist_entry_index: 0,
            performance_library: PerformanceLibrary::empty(),
            performance_revision: LibraryRevision::new("0".repeat(64)).unwrap(),
            live_performance: LivePerformanceState::default(),
            performance_list_index: 0,
            performance_editor_index: 0,
            performance_child_index: 0,
            performance_child_editor_index: 0,
            performance_value_index: 0,
            performance_draft: None,
            performance_dirty: false,
            performance_name: performance_text_editor("performance-name", "NAME", "NEW ITEM"),
            performance_child_name: performance_text_editor(
                "performance-child-name",
                "NAME",
                "MAIN",
            ),
            performance_confirm: performance_confirmation(),
            performance_spinner: Spinner::ascii("performance-loader", "SAVING", "PLEASE WAIT"),
            performance_result: None,
            performance_return_page: Page::Config,
            keyboard_parts_index: 0,
            keyboard_parts_return_page: Page::Home,
            keyboard_parts_editing: false,
            keyboard_split_quick_action: false,
            play_index: 0,
            play_plugins: Vec::new(),
            config_index: 0,
            plugin_index: 0,
            active_plugin_instance_id: None,
            pending_plugin_instance_id: None,
            active_plugin_id: "plugin.unavailable".into(),
            active_plugin_full_name: "PLUGIN".into(),
            active_plugin_name: "PLUGIN".into(),
            active_plugin_short_name: "PLUGIN".into(),
            active_plugin_config_available: false,
            plugin_spinner: Spinner::ascii("plugin-loader", "LOADING PLUGIN", "PLEASE WAIT"),
            system_index: 0,
            system_web_index: 0,
            web_settings: WebSystemSettings::default(),
            web_edit_candidate: WebSystemSettings::default(),
            system_web_editing: false,
            wifi_settings: WifiSystemSettings::default(),
            system_wifi_index: 0,
            wifi_networks_index: 0,
            wifi_saved_index: 0,
            wifi_known_action_index: 0,
            wifi_discovered_networks: Vec::new(),
            wifi_discovered_index: 0,
            wifi_radio_editing: false,
            wifi_radio_candidate: false,
            wifi_password: wifi_password_editor(),
            wifi_spinner: Spinner::ascii("system-loader", "LOADING", "PLEASE WAIT"),
            wifi_result: None,
            audio_state: None,
            audio_index: 0,
            audio_value_index: 0,
            audio_spinner: Spinner::ascii("audio-loader", "APPLYING", "PLEASE WAIT"),
            audio_result: None,
            plugin_library_index: 0,
            plugin_preset_index: 0,
            plugin_program_bank_index: 0,
            plugin_parameter_page_index: 0,
            plugin_parameter_index: 0,
            plugin_parameter_schema: None,
            plugin_parameter_values: BTreeMap::new(),
            plugin_parameter_editor: None,
            plugin_presets: Vec::new(),
            plugin_active_preset_id: None,
            plugin_preset_name: plugin_preset_name_editor("NEW PRESET"),
            plugin_preset_result: None,
            plugin_play_index: 0,
            plugin_play_context: PluginPlayContext::Standalone,
            plugin_custom_index: 0,
            plugin_section_index: 0,
            plugin_layer_index: 0,
            plugin_layer_option_index: 0,
            plugin_timbre_index: 0,
            plugin_sounds: Vec::new(),
            plugin_active_sound_id: None,
            play_anchor_sound_id: None,
            audition_lease_id: None,
            program_draft: None,
            program_name: program_name_editor("NEW PROGRAM"),
            envelope: envelope_carousel(None),
            pitch_envelope: pitch_envelope_carousel(None),
            lfo: lfo_carousel(None),
            tuning: tuning_carousel(None),
            range: range_carousel(None),
            layer_level: layer_level_carousel(None),
            program_output: program_output_carousel(1.0),
            unsaved_changes: unsaved_changes_dialog(),
            pending_program_exit: None,
            editor_path: Vec::new(),
            editor_selections: vec![0],
            editor_field: None,
            editor_field_id: None,
            pressed_button: None,
            pending_command: None,
        }
    }
}

impl Menu {
    fn plugin_screen(
        &self,
        context: impl Into<String>,
        line_1: impl Into<String>,
        line_2: impl Into<String>,
    ) -> Screen {
        Screen::with_plugin_header(
            &self.active_plugin_full_name,
            &self.active_plugin_short_name,
            context,
            line_1,
            line_2,
        )
    }

    fn plugin_simple_screen(
        &self,
        context: impl Into<String>,
        items: &[&str],
        details: &[&str],
        selected: usize,
    ) -> Screen {
        let [line_1, line_2] = simple_carousel(items, details, selected);
        self.plugin_screen(context, line_1, line_2)
    }

    pub fn set_active_plugin(&mut self, id: impl Into<String>, name: impl Into<String>) {
        self.active_plugin_id = id.into();
        let name = name.into();
        self.active_plugin_full_name = clean_surface_text(&name, "PLUGIN");
        self.active_plugin_name = normalized_display_text(&name, "PLUGIN");
        let catalog_plugin = self
            .play_plugins
            .iter()
            .find(|plugin| plugin.plugin_id == self.active_plugin_id);
        self.active_plugin_short_name = catalog_plugin
            .map(|plugin| plugin.short_name.clone())
            .unwrap_or_else(|| compact_plugin_name(&self.active_plugin_name));
        if let Some(plugin) = catalog_plugin {
            self.active_plugin_full_name = plugin.full_name.clone();
        }
        self.active_plugin_config_available = catalog_plugin
            .map(|plugin| plugin.config_available)
            .unwrap_or(false);
        if self.play_plugins.is_empty() {
            let plugin = PlayPlugin::new(
                "",
                self.active_plugin_id.clone(),
                self.active_plugin_name.clone(),
            );
            self.active_plugin_config_available = plugin.config_available;
            self.play_plugins.push(plugin);
            self.play_index = 0;
        }
        if self.pending_plugin_instance_id.is_none() && self.active_plugin_instance_id.is_none() {
            self.active_plugin_instance_id = self
                .play_plugins
                .iter()
                .find(|plugin| plugin.plugin_id == self.active_plugin_id)
                .map(|plugin| plugin.instance_id.clone());
        }
    }

    pub fn set_play_plugins(&mut self, plugins: Vec<PlayPlugin>, active_instance_id: Option<&str>) {
        let focused = self
            .play_plugins
            .get(self.play_index)
            .map(|plugin| plugin.instance_id.clone());
        self.play_plugins = plugins;
        self.play_index = self
            .pending_plugin_instance_id
            .as_deref()
            .and_then(|id| {
                self.play_plugins
                    .iter()
                    .position(|plugin| plugin.instance_id == id)
            })
            .or_else(|| {
                active_instance_id.and_then(|id| {
                    self.play_plugins
                        .iter()
                        .position(|plugin| plugin.instance_id == id)
                })
            })
            .or_else(|| {
                focused.as_deref().and_then(|id| {
                    self.play_plugins
                        .iter()
                        .position(|plugin| plugin.instance_id == id)
                })
            })
            .unwrap_or(0)
            .min(self.play_plugins.len().saturating_sub(1));
        if self.pending_plugin_instance_id.is_none() {
            self.active_plugin_instance_id = active_instance_id.map(str::to_owned);
        }
        if let Some(plugin) = self
            .play_plugins
            .iter()
            .find(|plugin| plugin.plugin_id == self.active_plugin_id)
        {
            self.active_plugin_full_name = plugin.full_name.clone();
            self.active_plugin_short_name = plugin.short_name.clone();
            self.active_plugin_config_available = plugin.config_available;
        }
    }

    pub fn sync_active_plugin(
        &mut self,
        instance_id: impl Into<String>,
        plugin_id: impl Into<String>,
        name: impl Into<String>,
        sounds: Vec<PlaySound>,
        selected_sound_id: Option<&str>,
    ) -> bool {
        let instance_id = instance_id.into();
        if self
            .pending_plugin_instance_id
            .as_deref()
            .is_some_and(|pending| pending != instance_id)
        {
            return false;
        }
        let plugin_id = plugin_id.into();
        if self.active_plugin_id != plugin_id {
            self.plugin_presets.clear();
            self.plugin_active_preset_id = None;
            self.plugin_preset_index = 0;
            self.plugin_program_bank_index = 0;
            self.plugin_parameter_schema = None;
            self.plugin_parameter_values.clear();
            self.plugin_parameter_page_index = 0;
            self.plugin_parameter_index = 0;
            self.plugin_parameter_editor = None;
        }
        self.active_plugin_instance_id = Some(instance_id.clone());
        self.set_active_plugin(plugin_id, name);
        self.set_play_sounds(sounds, selected_sound_id);
        if self.pending_plugin_instance_id.as_deref() == Some(instance_id.as_str()) {
            self.pending_plugin_instance_id = None;
        }
        true
    }

    pub fn advance_plugin_spinner(&mut self) -> bool {
        let loading_plugin =
            self.pending_plugin_instance_id.is_some() && self.is_plugin_browser_page();
        if !loading_plugin && self.page != Page::PluginPresetBusy {
            return false;
        }
        self.plugin_spinner.advance();
        true
    }

    pub fn is_plugin_loading(&self) -> bool {
        self.pending_plugin_instance_id.is_some() && self.is_plugin_browser_page()
    }

    fn begin_plugin_loading(&mut self, instance_id: impl Into<String>) {
        self.pending_plugin_instance_id = Some(instance_id.into());
        self.plugin_spinner.reset("LOADING PLUGIN");
        self.plugin_spinner.set_detail("PLEASE WAIT");
        self.plugin_library_index = 0;
        self.plugin_play_index = 0;
    }

    fn is_plugin_browser_page(&self) -> bool {
        matches!(
            self.page,
            Page::PluginLibrary
                | Page::PluginPresets
                | Page::PluginPresetName
                | Page::PluginPresetBusy
                | Page::PluginPresetResult
                | Page::PluginPrograms
                | Page::PluginPlay
                | Page::PluginParameterPages
                | Page::PluginParameters
                | Page::PluginParameterValue
        )
    }

    pub fn sync_active_mode(&mut self, mode: ActiveMode) {
        self.active_mode = mode;
    }

    pub fn sync_performance_snapshot(&mut self, snapshot: PerformanceSnapshot) {
        if snapshot.validate().is_err() {
            return;
        }
        // What the player is looking at, held by identity rather than by
        // position. A library edit — a deck tab saved in another window, a
        // Rack renamed on the web surface — must not move the cursor out
        // from under a hand that is browsing. Only a change in what is
        // actually LIVE re-anchors the screen to what is sounding.
        let live_changed = self.live_performance != snapshot.live;
        let browsed = (!live_changed).then(|| BrowsedLive {
            mode_index: self.live_index,
            rack: self
                .live_racks()
                .get(self.live_rack_index)
                .map(|rack| rack.id.clone()),
            song: self
                .live_songs()
                .get(self.live_song_index)
                .map(|song| song.id.clone()),
            song_part: self
                .selected_live_song()
                .and_then(|song| song.parts.get(self.live_song_part_index))
                .map(|part| part.id.clone()),
            setlist: self
                .live_setlists()
                .get(self.live_setlist_index)
                .map(|setlist| setlist.id.clone()),
            setlist_entry: self
                .selected_setlist_parts()
                .get(self.live_setlist_entry_index)
                .map(|(entry, _, part)| (entry.id.clone(), part.id.clone())),
        });
        self.performance_library = snapshot.library;
        self.performance_revision = snapshot.revision;
        self.live_performance = snapshot.live;
        self.live_index = match self.live_performance.mode {
            LiveBrowseMode::Rack => 0,
            LiveBrowseMode::Song => 1,
            LiveBrowseMode::Setlist => 2,
        };

        self.live_rack_index = match self.live_performance.rack.as_ref() {
            Some(LiveLocation::Rack { rack_id }) => self
                .live_racks()
                .iter()
                .position(|rack| &rack.id == rack_id)
                .unwrap_or(0),
            _ => 0,
        };
        if let Some(LiveLocation::Song { song_id, part_id }) = self.live_performance.song.as_ref() {
            self.live_song_index = self
                .live_songs()
                .iter()
                .position(|song| &song.id == song_id)
                .unwrap_or(0);
            self.live_song_part_index = self
                .selected_live_song()
                .and_then(|song| song.parts.iter().position(|part| &part.id == part_id))
                .unwrap_or(0);
        } else {
            self.live_song_index = 0;
            self.live_song_part_index = 0;
        }
        if let Some(LiveLocation::Setlist {
            setlist_id,
            entry_id,
            part_id,
        }) = self.live_performance.setlist.as_ref()
        {
            self.live_setlist_index = self
                .live_setlists()
                .iter()
                .position(|setlist| &setlist.id == setlist_id)
                .unwrap_or(0);
            self.live_setlist_entry_index = self
                .selected_setlist_parts()
                .iter()
                .position(|(entry, _, part)| &entry.id == entry_id && &part.id == part_id)
                .unwrap_or(0);
        } else {
            self.live_setlist_index = 0;
            self.live_setlist_entry_index = 0;
        }
        if let Some(browsed) = browsed {
            self.restore_browsed_live(browsed);
        }
    }

    /// Puts the cursor back on whatever the player had it on, when that
    /// thing survived the edit. Anything that did not survive keeps the
    /// position just derived from the LIVE location — the cursor falls back
    /// to what is sounding only when what it held is gone.
    fn restore_browsed_live(&mut self, browsed: BrowsedLive) {
        self.live_index = browsed.mode_index;
        if let Some(rack_id) = browsed.rack
            && let Some(index) = self.live_racks().iter().position(|rack| rack.id == rack_id)
        {
            self.live_rack_index = index;
        }
        if let Some(song_id) = browsed.song
            && let Some(index) = self.live_songs().iter().position(|song| song.id == song_id)
        {
            self.live_song_index = index;
            if let Some(part_id) = browsed.song_part
                && let Some(part_index) = self
                    .selected_live_song()
                    .and_then(|song| song.parts.iter().position(|part| part.id == part_id))
            {
                self.live_song_part_index = part_index;
            }
        }
        if let Some(setlist_id) = browsed.setlist
            && let Some(index) = self
                .live_setlists()
                .iter()
                .position(|setlist| setlist.id == setlist_id)
        {
            self.live_setlist_index = index;
            if let Some((entry_id, part_id)) = browsed.setlist_entry
                && let Some(entry_index) = self
                    .selected_setlist_parts()
                    .iter()
                    .position(|(entry, _, part)| entry.id == entry_id && part.id == part_id)
            {
                self.live_setlist_entry_index = entry_index;
            }
        }
    }

    pub fn sync_web_settings(&mut self, settings: WebSystemSettings) {
        self.web_settings = settings;
        if !self.system_web_editing {
            self.web_edit_candidate = settings;
        }
    }

    pub fn sync_wifi_settings(&mut self, settings: WifiSystemSettings) {
        let focused_id = self
            .wifi_settings
            .saved_networks
            .get(self.wifi_saved_index)
            .map(|network| network.id.as_str());
        let active_id = settings
            .saved_networks
            .iter()
            .find(|network| network.active)
            .map(|network| network.id.as_str());
        let focus_id = focused_id.or(active_id);
        self.wifi_saved_index = focus_id
            .and_then(|id| {
                settings
                    .saved_networks
                    .iter()
                    .position(|network| network.id == id)
            })
            .unwrap_or(0)
            .min(settings.saved_networks.len().saturating_sub(1));
        self.wifi_radio_candidate = settings.enabled;
        self.wifi_settings = settings;
        self.system_index = self.system_index.min(self.system_item_count() - 1);
    }

    pub fn sync_discovered_wifi(&mut self, networks: Vec<DiscoveredWifiNetwork>) {
        let focused_ssid = self
            .wifi_discovered_networks
            .get(self.wifi_discovered_index)
            .map(|network| network.ssid.clone());
        let saved_ssids = self
            .wifi_settings
            .saved_networks
            .iter()
            .filter_map(|network| network.ssid.as_deref())
            .collect::<std::collections::BTreeSet<_>>();
        self.wifi_discovered_networks = networks
            .into_iter()
            .filter(|network| !saved_ssids.contains(network.ssid.as_str()))
            .collect();
        self.wifi_discovered_index = focused_ssid
            .as_deref()
            .and_then(|ssid| {
                self.wifi_discovered_networks
                    .iter()
                    .position(|network| network.ssid == ssid)
            })
            .unwrap_or(0)
            .min(self.wifi_discovered_networks.len().saturating_sub(1));
    }

    pub fn show_wifi_result(&mut self, success: bool, message: impl Into<String>) {
        self.wifi_password.clear();
        self.wifi_result = Some((success, message.into()));
        self.page = Page::SystemWifiResult;
    }

    pub fn complete_wifi_result(&mut self, success: bool, message: impl Into<String>) {
        let was_visible = self.page == Page::SystemWifiBusy;
        self.wifi_password.clear();
        if was_visible {
            self.wifi_result = Some((success, message.into()));
            self.page = Page::SystemWifiResult;
        }
    }

    pub fn begin_wifi_busy(&mut self, label: impl Into<String>) {
        self.wifi_spinner.reset(label);
        self.wifi_spinner.set_detail("PLEASE WAIT");
        self.wifi_result = None;
        self.page = Page::SystemWifiBusy;
    }

    pub fn advance_wifi_spinner(&mut self) -> bool {
        if self.page != Page::SystemWifiBusy {
            return false;
        }
        self.wifi_spinner.advance();
        true
    }

    pub fn sync_audio_state(&mut self, state: AudioOutputState) {
        if state.validate().is_err() {
            return;
        }
        self.audio_state = Some(state);
        self.audio_value_index = 0;
    }

    pub fn begin_audio_busy(&mut self) {
        self.audio_spinner.reset("APPLYING");
        self.audio_spinner.set_detail("PLEASE WAIT");
        self.audio_result = None;
        self.page = Page::AudioBusy;
    }

    pub fn advance_audio_spinner(&mut self) -> bool {
        if self.page != Page::AudioBusy {
            return false;
        }
        self.audio_spinner.advance();
        true
    }

    pub fn complete_audio_change(&mut self, result: Result<AudioOutputState, impl Into<String>>) {
        if self.page != Page::AudioBusy {
            return;
        }
        match result {
            Ok(state) => {
                self.sync_audio_state(state);
                self.audio_result = Some((true, "OUTPUT READY".into()));
            }
            Err(error) => self.audio_result = Some((false, error.into())),
        }
        self.page = Page::AudioResult;
    }

    pub fn audio_device_name(&self) -> Option<String> {
        self.audio_state
            .as_ref()
            .map(|state| normalized_display_text(&state.active_device.name, "AUDIO OUTPUT"))
    }

    pub fn complete_wifi_scan(&mut self, networks: Vec<DiscoveredWifiNetwork>) {
        self.sync_discovered_wifi(networks);
        if self.page == Page::SystemWifiBusy {
            self.page = Page::SystemWifiDiscovered;
        }
    }

    fn system_item_count(&self) -> usize {
        1 + usize::from(self.wifi_settings.available)
    }

    fn system_item(&self) -> (&'static str, &'static str) {
        if self.system_index == 1 && self.wifi_settings.available {
            SYSTEM_WIFI_ITEM
        } else {
            SYSTEM_WEB_ITEM
        }
    }

    pub fn set_play_sounds(&mut self, sounds: Vec<PlaySound>, selected_sound_id: Option<&str>) {
        let browsed_sound_id = self
            .filtered_sounds()
            .get(self.plugin_play_index)
            .map(|sound| sound.id.clone());
        self.plugin_sounds = sounds;
        self.plugin_active_sound_id = selected_sound_id.map(str::to_owned);
        if self.program_draft.is_none() {
            self.play_anchor_sound_id = selected_sound_id.map(str::to_owned);
        }
        let focus_id = browsed_sound_id.as_deref().or(selected_sound_id);
        if let Some(sound) =
            focus_id.and_then(|id| self.plugin_sounds.iter().find(|sound| sound.id == id))
        {
            self.plugin_program_bank_index = self.plugin_bank_index(&sound.bank).unwrap_or(0);
        }
        self.plugin_play_index = focus_id
            .and_then(|id| {
                self.filtered_sounds()
                    .iter()
                    .position(|sound| sound.id == id)
            })
            .unwrap_or(0);
    }

    pub fn set_plugin_presets(&mut self, presets: Vec<PlayPreset>, active_preset_id: Option<&str>) {
        let focused = self
            .plugin_preset_index
            .checked_sub(1)
            .and_then(|index| self.plugin_presets.get(index))
            .map(|preset| preset.id.clone());
        let active_id = active_preset_id
            .map(str::to_owned)
            .or_else(|| self.plugin_active_preset_id.clone());
        let focus_id = active_id.clone().or(focused);
        self.plugin_presets = presets;
        self.plugin_active_preset_id =
            active_id.filter(|id| self.plugin_presets.iter().any(|preset| preset.id == *id));
        self.plugin_preset_index = focus_id
            .as_deref()
            .and_then(|id| {
                self.plugin_presets
                    .iter()
                    .position(|preset| preset.id == id)
            })
            .map(|index| index + 1)
            .unwrap_or(0)
            .min(self.plugin_presets.len());
    }

    pub fn complete_plugin_preset_load(&mut self, preset_id: &str) {
        self.plugin_active_preset_id = Some(preset_id.to_owned());
        if let Some(index) = self
            .plugin_presets
            .iter()
            .position(|preset| preset.id == preset_id)
        {
            self.plugin_preset_index = index + 1;
        }
    }

    /// Completes a host-owned snapshot started from LITTLE. The host returns
    /// both the canonical preset and the refreshed catalog so the surface
    /// never has to guess an id, version or normalized name.
    pub fn complete_plugin_preset_save(
        &mut self,
        result: Result<(PlayPreset, Vec<PlayPreset>), String>,
    ) {
        match result {
            Ok((saved, presets)) => {
                let saved_id = saved.id.clone();
                self.set_plugin_presets(presets, Some(&saved_id));
                self.plugin_preset_result = Some((true, "PRESET SAVED".into()));
            }
            Err(error) => {
                self.plugin_preset_result =
                    Some((false, normalized_display_text(&error, "SAVE FAILED")));
            }
        }
        self.page = Page::PluginPresetResult;
    }

    /// Supplies the active plugin's validated public parameter contract to
    /// LITTLE. The plugin owns names, grouping and domains; RackForge owns the
    /// text navigation and dispatches mutations through the normal parameter
    /// path. Read-only meters remain visible but cannot be edited.
    pub fn sync_plugin_parameters(
        &mut self,
        schema: ParameterSchema,
        values: impl IntoIterator<Item = (u32, f64)>,
    ) {
        if schema.validate().is_err() {
            self.plugin_parameter_schema = None;
            self.plugin_parameter_values.clear();
            self.plugin_parameter_editor = None;
            return;
        }
        let preserve_open_editor = self.page == Page::PluginParameterValue
            && self.plugin_parameter_editor.is_some()
            && self
                .plugin_parameter_schema
                .as_ref()
                .is_some_and(|current| current == &schema);
        self.plugin_parameter_values = values
            .into_iter()
            .filter(|(_, value)| value.is_finite())
            .collect();
        self.plugin_parameter_schema = Some(schema);
        self.plugin_parameter_page_index = self
            .plugin_parameter_page_index
            .min(self.plugin_parameter_pages().len().saturating_sub(1));
        self.plugin_parameter_index = self
            .plugin_parameter_index
            .min(self.current_plugin_parameters().len().saturating_sub(1));
        if !preserve_open_editor {
            self.plugin_parameter_editor = None;
        }
    }

    /// Applies the canonical value returned by the host after a LITTLE edit.
    pub fn complete_plugin_parameter_set(&mut self, parameter_index: u32, value: f64) {
        if value.is_finite()
            && self.plugin_parameter_schema.as_ref().is_some_and(|schema| {
                schema
                    .parameters
                    .iter()
                    .any(|parameter| parameter.index == parameter_index)
            })
        {
            self.plugin_parameter_values.insert(parameter_index, value);
        }
    }

    fn focus_plugin_play_sound(&mut self, sound_id: Option<&str>) {
        let Some((sound_id, bank)) = sound_id.and_then(|id| {
            self.plugin_sounds
                .iter()
                .find(|sound| sound.id == id)
                .map(|sound| (sound.id.clone(), sound.bank.clone()))
        }) else {
            self.plugin_program_bank_index = 0;
            self.plugin_play_index = 0;
            return;
        };
        self.plugin_program_bank_index = self.plugin_bank_index(&bank).unwrap_or(0);
        self.plugin_play_index = self
            .filtered_sounds()
            .iter()
            .position(|sound| sound.id == sound_id)
            .unwrap_or(0);
    }

    fn plugin_play_selected_sound_id(&self) -> Option<&str> {
        match self.plugin_play_context {
            PluginPlayContext::Standalone => self.plugin_active_sound_id.as_deref(),
            PluginPlayContext::RackSlot => self.focused_rack_slot().and_then(rack_slot_sound_hint),
        }
    }

    pub fn take_command(&mut self) -> Option<MenuCommand> {
        self.pending_command.take()
    }

    pub fn audition_lease_id(&self) -> Option<u64> {
        self.audition_lease_id
    }

    pub fn sync_program_edit(
        &mut self,
        draft: Option<ProgramDraftState>,
        audition_lease_id: Option<u64>,
    ) {
        let previous_draft_id = self.program_draft.as_ref().map(|draft| draft.draft_id);
        let next_draft_id = draft.as_ref().map(|draft| draft.draft_id);
        let edit_was_visible = self.program_draft.is_some() && self.is_program_edit_page();
        self.audition_lease_id = audition_lease_id;
        self.program_draft = draft;
        if self.plugin_layer_index >= self.program_layer_count()
            && self.page != Page::PluginLayerMenu
        {
            self.plugin_layer_index = self.program_layer_count().saturating_sub(1);
        }
        self.plugin_layer_option_index = self
            .plugin_layer_option_index
            .min(self.layer_menu_len().saturating_sub(1));
        self.sync_program_output();
        self.sync_layer_editors();

        if let Some(draft_id) = next_draft_id {
            if !self.program_name.is_editing()
                && let Some(name) = self.program_draft.as_ref().map(|draft| draft.name.as_str())
            {
                self.program_name.set_value(name);
            }
            if previous_draft_id != Some(draft_id) {
                self.editor_path.clear();
                self.editor_selections.clear();
                self.editor_selections.push(0);
                self.editor_field = None;
                self.editor_field_id = None;
                self.page = Page::ProgramEditorRoot;
            }
        } else if edit_was_visible {
            self.pending_program_exit = None;
            self.page = Page::PluginCustomPrograms;
        }
    }

    pub fn set_button_pressed(&mut self, input: Input, pressed: bool) -> bool {
        let index = match input {
            Input::Button1 => 0,
            Input::Button2 => 1,
            Input::Button3 => 2,
            Input::Button4 => 3,
            _ => return false,
        };
        if pressed {
            self.pressed_button = Some(index);
        } else if self.pressed_button == Some(index) {
            self.pressed_button = None;
        }
        true
    }

    pub fn clear_pressed_button(&mut self) {
        self.pressed_button = None;
    }

    fn keyboard_parts_rack(&self) -> Option<&RackDefinition> {
        self.live_performance
            .active_rack_id
            .as_ref()
            .and_then(|id| self.performance_library.rack(id))
            .or_else(|| self.performance_library.racks.first())
    }

    fn keyboard_parts_count(&self) -> usize {
        usize::from(self.keyboard_parts_rack().is_some()) * 2
    }

    fn keyboard_parts_config(&self) -> RackKeyboardParts {
        match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Rack(rack)) => rack.keyboard_parts.unwrap_or_default(),
            _ => self
                .keyboard_parts_rack()
                .and_then(|rack| rack.keyboard_parts)
                .unwrap_or_default(),
        }
    }

    fn keyboard_part_config(&self) -> RackKeyboardPart {
        self.keyboard_parts_config().part(self.keyboard_parts_index)
    }

    fn move_keyboard_part(&mut self, delta: isize) {
        let count = self.keyboard_parts_count();
        move_wrapped(&mut self.keyboard_parts_index, count, delta);
    }

    fn move_keyboard_part_draft(&mut self, delta: isize) {
        let count = match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Rack(_)) => 2,
            _ => 0,
        };
        move_wrapped(&mut self.keyboard_parts_index, count, delta);
        self.performance_child_index = self.keyboard_parts_index;
        self.performance_child_editor_index = 0;
    }

    fn open_keyboard_parts(&mut self, edit: bool) {
        if self.page != Page::KeyboardParts && !self.keyboard_parts_editing {
            self.keyboard_parts_return_page = self.page;
        }
        let count = self.keyboard_parts_count();
        if count == 0 {
            self.performance_result = Some((false, "NO ACTIVE SLOTS".into()));
            self.performance_return_page = self.keyboard_parts_return_page;
            self.page = Page::PerformanceResult;
            return;
        }
        self.keyboard_parts_index = self.keyboard_parts_index.min(count - 1);
        self.page = Page::KeyboardParts;
        if edit {
            self.begin_keyboard_part_edit();
        }
    }

    fn begin_keyboard_part_edit(&mut self) {
        if self.performance_draft.is_some() && !self.keyboard_parts_editing {
            self.performance_result = Some((false, "EDIT IN PROGRESS".into()));
            self.page = Page::PerformanceResult;
            return;
        }
        let Some(mut rack) = self.keyboard_parts_rack().cloned() else {
            return;
        };
        let migrated = initialize_keyboard_parts(&mut rack);
        self.performance_draft = Some(PerformanceDraft::Rack(rack));
        self.performance_dirty = migrated;
        self.performance_child_index = self.keyboard_parts_index;
        self.performance_child_editor_index = 0;
        self.keyboard_parts_editing = true;
        self.page = Page::ConfigRackSlotEditor;
    }

    fn apply_keyboard_parts_input(&mut self, input: Input) {
        match input.default_navigation() {
            Some(Action::Previous) => self.move_keyboard_part(-1),
            Some(Action::Next) => self.move_keyboard_part(1),
            Some(Action::Select) => self.begin_keyboard_part_edit(),
            Some(Action::Back) => {
                self.page = self.keyboard_parts_return_page;
            }
            None => {}
        }
    }

    fn apply_keyboard_split_gesture(&mut self, split: Option<u8>) {
        if self.performance_draft.is_some() && !self.keyboard_parts_editing {
            self.performance_result = Some((false, "EDIT IN PROGRESS".into()));
            self.page = Page::PerformanceResult;
            return;
        }
        let Some(mut rack) = self.keyboard_parts_rack().cloned() else {
            self.show_performance_error("NO ACTIVE RACK");
            return;
        };
        if self.page != Page::KeyboardParts {
            self.keyboard_parts_return_page = self.page;
        }
        initialize_keyboard_parts(&mut rack);
        let mut parts = rack.keyboard_parts.unwrap_or_default();
        parts.split_key = split.map(|key| key.clamp(1, 127));
        rack.keyboard_parts = Some(parts);
        self.keyboard_parts_index = usize::from(split.is_some());
        self.performance_draft = Some(PerformanceDraft::Rack(rack));
        self.performance_dirty = true;
        self.performance_return_page = Page::KeyboardParts;
        self.keyboard_parts_editing = true;
        self.keyboard_split_quick_action = true;
        self.save_performance_draft();
    }

    pub fn apply_input(&mut self, input: Input) {
        if let Input::KeyboardSplitNote(note) = input {
            self.apply_keyboard_split_gesture(Some(note));
            return;
        }
        if self.keyboard_parts_editing
            && matches!(input, Input::KeyboardParts | Input::KeyboardPartsLong)
        {
            if input == Input::KeyboardParts && self.page == Page::ConfigRackSlotEditor {
                self.move_keyboard_part_draft(1);
            }
            return;
        }
        if input == Input::KeyboardParts {
            if self.page == Page::KeyboardParts {
                self.page = self.keyboard_parts_return_page;
            } else {
                self.open_keyboard_parts(false);
            }
            return;
        }
        if input == Input::KeyboardPartsLong {
            self.apply_keyboard_split_gesture(None);
            return;
        }
        if input == Input::HomeChord {
            self.audition_lease_id = None;
            self.program_draft = None;
            self.active_mode = ActiveMode::Idle;
            self.page = Page::Home;
            self.pending_command = Some(MenuCommand::ForceHome);
            return;
        }
        if self.is_plugin_loading() {
            return;
        }
        if input == Input::Button4Long {
            if self.program_draft.as_ref().is_some_and(|draft| draft.dirty) {
                self.open_unsaved_changes(ProgramExitDestination::ActiveMode {
                    mode: self.active_mode,
                    selected_sound_id: self.play_anchor_sound_id.clone(),
                });
                return;
            }
            self.pending_command = Some(MenuCommand::ReturnToActiveMode {
                mode: self.active_mode,
                cancel_draft_id: self.program_draft.as_ref().map(|draft| draft.draft_id),
                selected_sound_id: self.play_anchor_sound_id.clone(),
            });
            return;
        }
        if self.is_performance_config_page() {
            self.apply_performance_input(input);
        } else if self.page == Page::KeyboardParts {
            self.apply_keyboard_parts_input(input);
        } else if self.page == Page::ProgramEditorField {
            self.apply_generic_editor_field_input(input);
        } else if matches!(
            self.page,
            Page::ProgramEditorSound | Page::ProgramEditorRoot | Page::ProgramEditorSection
        ) {
            if let Some(action) = input.default_navigation() {
                self.apply_generic_editor_action(action);
            }
        } else if self.page == Page::PluginUnsavedChanges {
            self.apply_unsaved_changes_input(input);
        } else if matches!(
            self.page,
            Page::PluginPresetName | Page::PluginPresetBusy | Page::PluginPresetResult
        ) {
            self.apply_plugin_preset_input(input);
        } else if self.page == Page::PluginName {
            self.apply_program_name_input(input);
        } else if self.page == Page::PluginProgramOutput {
            self.apply_program_output_input(input);
        } else if matches!(
            self.page,
            Page::Audio
                | Page::AudioOutput
                | Page::AudioRate
                | Page::AudioLatency
                | Page::AudioBusy
                | Page::AudioResult
        ) {
            self.apply_audio_input(input);
        } else if self.page == Page::SystemWeb {
            self.apply_system_web_input(input);
        } else if self.page == Page::SystemWifi {
            self.apply_system_wifi_input(input);
        } else if matches!(
            self.page,
            Page::SystemWifiNetworks
                | Page::SystemWifiKnown
                | Page::SystemWifiKnownActions
                | Page::SystemWifiDiscovered
                | Page::SystemWifiDiscoveredActions
                | Page::SystemWifiPassword
                | Page::SystemWifiBusy
                | Page::SystemWifiResult
        ) {
            self.apply_wifi_network_input(input);
        } else if self.is_layer_parameter_page() {
            self.apply_layer_parameter_input(input);
        } else if let Some(action) = input.default_navigation() {
            self.apply(action);
        }
    }

    fn begin_program_edit(&mut self) {
        let editable_sounds = self.editable_sounds();
        let program_id = if self.plugin_custom_index == 0 {
            None
        } else {
            let Some(program) = editable_sounds.get(self.plugin_custom_index - 1) else {
                return;
            };
            Some(program.id.clone())
        };
        self.pending_command = Some(MenuCommand::BeginProgramEdit { program_id });
    }

    fn open_plugin_preset_name(&mut self) {
        let suggested = self
            .plugin_active_sound_id
            .as_deref()
            .and_then(|id| self.plugin_sounds.iter().find(|sound| sound.id == id))
            .map_or("NEW PRESET", |sound| sound.name.as_str());
        self.plugin_preset_name.set_value(suggested);
        let _ = self.plugin_preset_name.handle(Input::Button1);
        self.plugin_preset_result = None;
        self.page = Page::PluginPresetName;
    }

    fn apply_plugin_preset_input(&mut self, input: Input) {
        match self.page {
            Page::PluginPresetName => match self.plugin_preset_name.handle(input) {
                ComponentEvent::EditCommitted(_) => {
                    self.plugin_spinner.reset("SAVING PRESET");
                    self.plugin_spinner.set_detail("PLEASE WAIT");
                    self.pending_command = Some(MenuCommand::SavePluginPreset {
                        name: self.plugin_preset_name.value().to_owned(),
                    });
                    self.page = Page::PluginPresetBusy;
                }
                ComponentEvent::EditCancelled(_) | ComponentEvent::ExitRequested(_) => {
                    self.page = Page::PluginPresets;
                }
                _ => {}
            },
            Page::PluginPresetBusy => {}
            Page::PluginPresetResult => {
                if matches!(
                    input.default_navigation(),
                    Some(Action::Select | Action::Back)
                ) {
                    self.plugin_preset_result = None;
                    self.page = Page::PluginPresets;
                }
            }
            _ => {}
        }
    }

    pub fn complete_performance_edit(&mut self, result: Result<PerformanceSnapshot, String>) {
        let quick_action = self.keyboard_split_quick_action;
        self.keyboard_split_quick_action = false;
        match result {
            Ok(snapshot) => {
                self.sync_performance_snapshot(snapshot);
                self.performance_draft = None;
                self.performance_dirty = false;
                if quick_action {
                    self.keyboard_parts_editing = false;
                    self.page = Page::KeyboardParts;
                    return;
                }
                self.performance_result = Some((true, "SAVED".into()));
            }
            Err(error) => {
                self.performance_result =
                    Some((false, normalized_display_text(&error, "SAVE FAILED")));
            }
        }
        self.page = Page::PerformanceResult;
    }

    fn is_performance_config_page(&self) -> bool {
        matches!(
            self.page,
            Page::ConfigRacks
                | Page::ConfigRackEditor
                | Page::ConfigRackName
                | Page::ConfigRackSlots
                | Page::ConfigRackSlotEditor
                | Page::ConfigRackSlotName
                | Page::ConfigRackSlotPlugin
                | Page::ConfigRackSlotMidiChannel
                | Page::ConfigRackSlotLowKey
                | Page::ConfigRackSlotHighKey
                | Page::ConfigRackSlotOctave
                | Page::ConfigRackSlotMidiOutput
                | Page::ConfigRackSlotAudioOutput
                | Page::ConfigRackSlotLevel
                | Page::ConfigRackSlotPan
                | Page::ConfigSongs
                | Page::ConfigSongEditor
                | Page::ConfigSongName
                | Page::ConfigSongParts
                | Page::ConfigSongPartEditor
                | Page::ConfigSongPartName
                | Page::ConfigSongPartRack
                | Page::ConfigSetlists
                | Page::ConfigSetlistEditor
                | Page::ConfigSetlistName
                | Page::ConfigSetlistEntries
                | Page::ConfigSetlistEntryEditor
                | Page::ConfigSetlistEntrySong
                | Page::PerformanceUnsaved
                | Page::PerformanceDelete
                | Page::PerformanceBusy
                | Page::PerformanceResult
        )
    }

    fn apply_performance_input(&mut self, input: Input) {
        if self.page == Page::PerformanceBusy {
            return;
        }
        if self.page == Page::PerformanceResult {
            if matches!(input, Input::Button1 | Input::Button4 | Input::EncoderPress) {
                let success = self
                    .performance_result
                    .as_ref()
                    .is_some_and(|result| result.0);
                self.performance_result = None;
                let destination = if success {
                    self.performance_return_page
                } else {
                    self.performance_editor_page()
                };
                if success && destination == Page::KeyboardParts {
                    self.keyboard_parts_editing = false;
                }
                self.page = destination;
            }
            return;
        }
        if self.page == Page::PerformanceUnsaved {
            match self.performance_confirm.handle(input) {
                ComponentEvent::Activated(_) if self.performance_confirm.selected() == 0 => {
                    self.save_performance_draft();
                }
                ComponentEvent::Activated(_) => {
                    self.performance_draft = None;
                    self.performance_dirty = false;
                    if self.performance_return_page == Page::KeyboardParts {
                        self.keyboard_parts_editing = false;
                    }
                    self.page = self.performance_return_page;
                }
                ComponentEvent::ExitRequested(_) => self.page = self.performance_editor_page(),
                _ => {}
            }
            return;
        }
        if self.page == Page::PerformanceDelete {
            match self.performance_confirm.handle(input) {
                ComponentEvent::Activated(_) if self.performance_confirm.selected() == 0 => {
                    self.delete_performance_draft();
                }
                ComponentEvent::Activated(_) | ComponentEvent::ExitRequested(_) => {
                    self.page = self.performance_editor_page();
                }
                _ => {}
            }
            return;
        }
        if matches!(
            self.page,
            Page::ConfigRackName | Page::ConfigSongName | Page::ConfigSetlistName
        ) {
            match self.performance_name.handle(input) {
                ComponentEvent::EditCommitted(_) => {
                    let name = self.performance_name.value().to_owned();
                    match self.performance_draft.as_mut() {
                        Some(PerformanceDraft::Rack(value)) => value.name = name,
                        Some(PerformanceDraft::Song(value)) => value.name = name,
                        Some(PerformanceDraft::Setlist(value)) => value.name = name,
                        None => {}
                    }
                    self.performance_dirty = true;
                }
                ComponentEvent::ExitRequested(_) => self.page = self.performance_editor_page(),
                _ => {}
            }
            return;
        }
        if matches!(
            self.page,
            Page::ConfigSongPartName | Page::ConfigRackSlotName
        ) {
            match self.performance_child_name.handle(input) {
                ComponentEvent::EditCommitted(_) => {
                    let name = self.performance_child_name.value().to_owned();
                    match self.performance_draft.as_mut() {
                        Some(PerformanceDraft::Song(song)) => {
                            if let Some(part) = song.parts.get_mut(self.performance_child_index) {
                                part.name = name;
                                self.performance_dirty = true;
                            }
                        }
                        Some(PerformanceDraft::Rack(rack)) => {
                            if let Some(slot) = rack.slots.get_mut(self.performance_child_index) {
                                slot.name = name;
                                self.performance_dirty = true;
                            }
                        }
                        _ => {}
                    }
                }
                ComponentEvent::ExitRequested(_) => {
                    self.page = if matches!(self.performance_draft, Some(PerformanceDraft::Rack(_)))
                    {
                        Page::ConfigRackSlotEditor
                    } else {
                        Page::ConfigSongPartEditor
                    }
                }
                _ => {}
            }
            return;
        }
        if !matches!(
            input,
            Input::Button1
                | Input::Button2
                | Input::Button3
                | Input::Button4
                | Input::EncoderPress
                | Input::EncoderLeft
                | Input::EncoderRight
        ) {
            return;
        }
        let previous = matches!(input, Input::Button2 | Input::EncoderLeft);
        let next = matches!(input, Input::Button3 | Input::EncoderRight);
        let select = matches!(input, Input::Button1 | Input::EncoderPress);
        let back = input == Input::Button4;
        match self.page {
            Page::ConfigRacks | Page::ConfigSongs | Page::ConfigSetlists => {
                let len = match self.page {
                    Page::ConfigRacks => self.performance_library.racks.len() + 1,
                    Page::ConfigSongs => self.performance_library.songs.len() + 1,
                    _ => self.performance_library.setlists.len() + 1,
                };
                if previous || next {
                    move_wrapped(
                        &mut self.performance_list_index,
                        len,
                        if previous { -1 } else { 1 },
                    );
                } else if back {
                    self.page = Page::Config;
                } else if select {
                    self.begin_performance_draft();
                }
            }
            Page::ConfigRackEditor | Page::ConfigSongEditor | Page::ConfigSetlistEditor => {
                let len = 5;
                if previous || next {
                    move_wrapped(
                        &mut self.performance_editor_index,
                        len,
                        if previous { -1 } else { 1 },
                    );
                } else if back {
                    self.leave_performance_editor();
                } else if select {
                    self.activate_performance_editor_item();
                }
            }
            Page::ConfigRackSlots => self.apply_rack_slots_input(previous, next, select, back),
            Page::ConfigRackSlotEditor => {
                self.apply_rack_slot_editor_input(previous, next, select, back)
            }
            Page::ConfigRackSlotPlugin => {
                if previous || next {
                    self.move_selection(if previous { -1 } else { 1 });
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                } else if select {
                    let Some(plugin) = self.play_plugins.get(self.play_index).cloned() else {
                        return;
                    };
                    let plugin_changed = if let Some(PerformanceDraft::Rack(rack)) =
                        self.performance_draft.as_mut()
                        && let Some(slot) = rack.slots.get_mut(self.performance_child_index)
                    {
                        let plugin_changed = slot.plugin_id != plugin.plugin_id;
                        if plugin_changed {
                            slot.plugin_id = plugin.plugin_id.clone();
                            slot.state = None;
                            slot.legacy_program_id = None;
                            self.performance_dirty = true;
                        }
                        plugin_changed
                    } else {
                        false
                    };
                    let state_id = self
                        .focused_rack_slot()
                        .and_then(rack_slot_sound_hint)
                        .map(str::to_owned);
                    self.plugin_play_context = PluginPlayContext::RackSlot;
                    self.focus_plugin_play_sound(state_id.as_deref());
                    if self.active_plugin_instance_id.as_deref()
                        != Some(plugin.instance_id.as_str())
                    {
                        self.pending_command = Some(MenuCommand::SelectPlugin {
                            instance_id: plugin.instance_id.clone(),
                        });
                        self.begin_plugin_loading(plugin.instance_id);
                    } else if plugin_changed {
                        self.focus_plugin_play_sound(None);
                    }
                    self.page = Page::PluginLibrary;
                }
            }
            Page::ConfigRackSlotMidiChannel => {
                if previous || next {
                    if self.keyboard_parts_editing {
                        self.performance_value_index = if previous {
                            if self.performance_value_index <= 1 {
                                16
                            } else {
                                self.performance_value_index - 1
                            }
                        } else if self.performance_value_index >= 16 {
                            1
                        } else {
                            self.performance_value_index + 1
                        };
                    } else {
                        move_wrapped(
                            &mut self.performance_value_index,
                            17,
                            if previous { -1 } else { 1 },
                        );
                    }
                } else if select {
                    self.update_rack_slot_midi_channel();
                    self.page = Page::ConfigRackSlotEditor;
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotLowKey => {
                let (low, high) = if self.keyboard_parts_editing {
                    (1, 127)
                } else {
                    (
                        0,
                        self.focused_rack_slot()
                            .map_or(127, |slot| usize::from(slot.midi_note_high)),
                    )
                };
                if previous || next {
                    self.performance_value_index = if previous {
                        self.performance_value_index.saturating_sub(1).max(low)
                    } else {
                        (self.performance_value_index + 1).min(high)
                    };
                } else if select {
                    if self.keyboard_parts_editing {
                        self.update_keyboard_split_key();
                    } else {
                        self.update_rack_slot_low_key();
                    }
                    self.page = Page::ConfigRackSlotEditor;
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotHighKey => {
                let low = self
                    .focused_rack_slot()
                    .map_or(0, |slot| slot.midi_note_low);
                if previous || next {
                    self.performance_value_index = if previous {
                        self.performance_value_index
                            .saturating_sub(1)
                            .max(usize::from(low))
                    } else {
                        (self.performance_value_index + 1).min(127)
                    };
                } else if select {
                    self.update_rack_slot_high_key();
                    self.page = Page::ConfigRackSlotEditor;
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotOctave => {
                if previous || next {
                    self.performance_value_index = if previous {
                        self.performance_value_index.saturating_sub(1)
                    } else {
                        (self.performance_value_index + 1).min(8)
                    };
                } else if select {
                    self.update_rack_slot_octave();
                    self.page = Page::ConfigRackSlotEditor;
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotMidiOutput => {
                if select || back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotAudioOutput => {
                if back || select {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotLevel => {
                if previous || next {
                    self.performance_value_index = if previous {
                        self.performance_value_index.saturating_sub(1)
                    } else {
                        (self.performance_value_index + 1).min(100)
                    };
                } else if select {
                    self.update_rack_slot_level();
                    self.page = Page::ConfigRackSlotEditor;
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigRackSlotPan => {
                if previous || next {
                    self.performance_value_index = if previous {
                        self.performance_value_index.saturating_sub(1)
                    } else {
                        (self.performance_value_index + 1).min(200)
                    };
                } else if select {
                    self.update_rack_slot_pan();
                    self.page = Page::ConfigRackSlotEditor;
                } else if back {
                    self.page = Page::ConfigRackSlotEditor;
                }
            }
            Page::ConfigSongParts => self.apply_song_parts_input(previous, next, select, back),
            Page::ConfigSongPartEditor => {
                self.apply_song_part_editor_input(previous, next, select, back)
            }
            Page::ConfigSongPartRack => {
                let len = self.performance_library.racks.len();
                if len == 0 {
                    return;
                }
                if previous || next {
                    move_wrapped(
                        &mut self.performance_child_editor_index,
                        len,
                        if previous { -1 } else { 1 },
                    );
                } else if back {
                    self.page = Page::ConfigSongPartEditor;
                } else if select {
                    let rack_id = self.performance_library.racks
                        [self.performance_child_editor_index]
                        .id
                        .clone();
                    if let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_mut()
                        && let Some(part) = song.parts.get_mut(self.performance_child_index)
                    {
                        part.rack_id = rack_id;
                        // LITTLE exposes a compact single-Rack picker rather
                        // than the visual graph editor. Choosing a Rack is an
                        // explicit request to replace any graph-backed Part
                        // with that legacy-compatible single-Rack route.
                        part.content = None;
                        self.performance_dirty = true;
                    }
                    self.page = Page::ConfigSongPartEditor;
                }
            }
            Page::ConfigSetlistEntries => {
                self.apply_setlist_entries_input(previous, next, select, back)
            }
            Page::ConfigSetlistEntryEditor => {
                self.apply_setlist_entry_editor_input(previous, next, select, back)
            }
            Page::ConfigSetlistEntrySong => {
                let len = self.performance_library.songs.len();
                if len == 0 {
                    return;
                }
                if previous || next {
                    move_wrapped(
                        &mut self.performance_child_editor_index,
                        len,
                        if previous { -1 } else { 1 },
                    );
                } else if back {
                    self.page = Page::ConfigSetlistEntries;
                } else if select {
                    let song_id = self.performance_library.songs
                        [self.performance_child_editor_index]
                        .id
                        .clone();
                    if let Some(PerformanceDraft::Setlist(setlist)) =
                        self.performance_draft.as_mut()
                        && let Some(entry) = setlist.entries.get_mut(self.performance_child_index)
                    {
                        entry.song_id = song_id;
                        self.performance_dirty = true;
                    }
                    self.page = Page::ConfigSetlistEntries;
                }
            }
            _ => {}
        }
    }

    fn begin_performance_draft(&mut self) {
        self.performance_editor_index = 0;
        self.performance_child_index = 0;
        self.performance_child_editor_index = 0;
        self.performance_dirty = false;
        self.performance_draft = match self.page {
            Page::ConfigRacks if self.performance_list_index == 0 => {
                Some(PerformanceDraft::Rack(RackDefinition {
                    schema_version: PERFORMANCE_SCHEMA_VERSION,
                    id: RackId::new(next_available_id(
                        "rack.user",
                        self.performance_library
                            .racks
                            .iter()
                            .map(|value| value.id.as_str()),
                    ))
                    .unwrap(),
                    name: "NEW RACK".into(),
                    enabled: true,
                    keyboard_parts: None,
                    slots: Vec::new(),
                    graph: None,
                }))
            }
            Page::ConfigRacks => self
                .performance_library
                .racks
                .get(self.performance_list_index - 1)
                .cloned()
                .map(PerformanceDraft::Rack),
            Page::ConfigSongs if self.performance_list_index == 0 => {
                let Some(rack) = self.performance_library.racks.first() else {
                    self.show_performance_error("CREATE A RACK FIRST");
                    return;
                };
                Some(PerformanceDraft::Song(SongDefinition {
                    schema_version: PERFORMANCE_SCHEMA_VERSION,
                    id: SongId::new(next_available_id(
                        "song.user",
                        self.performance_library
                            .songs
                            .iter()
                            .map(|value| value.id.as_str()),
                    ))
                    .unwrap(),
                    name: "NEW SONG".into(),
                    enabled: true,
                    parts: vec![SongPart {
                        id: SongPartId::new("part.001").unwrap(),
                        name: "MAIN".into(),
                        rack_id: rack.id.clone(),
                        content: None,
                        patterns: Vec::new(),
                    }],
                }))
            }
            Page::ConfigSongs => self
                .performance_library
                .songs
                .get(self.performance_list_index - 1)
                .cloned()
                .map(PerformanceDraft::Song),
            Page::ConfigSetlists if self.performance_list_index == 0 => {
                let Some(song) = self.performance_library.songs.first() else {
                    self.show_performance_error("CREATE A SONG FIRST");
                    return;
                };
                Some(PerformanceDraft::Setlist(SetlistDefinition {
                    schema_version: PERFORMANCE_SCHEMA_VERSION,
                    id: SetlistId::new(next_available_id(
                        "setlist.user",
                        self.performance_library
                            .setlists
                            .iter()
                            .map(|value| value.id.as_str()),
                    ))
                    .unwrap(),
                    name: "NEW SETLIST".into(),
                    enabled: true,
                    entries: vec![SetlistEntry {
                        id: SetlistEntryId::new("entry.001").unwrap(),
                        song_id: song.id.clone(),
                    }],
                }))
            }
            Page::ConfigSetlists => self
                .performance_library
                .setlists
                .get(self.performance_list_index - 1)
                .cloned()
                .map(PerformanceDraft::Setlist),
            _ => None,
        };
        if let Some(draft) = &self.performance_draft {
            self.performance_name.set_value(draft.name());
            self.page = self.performance_editor_page();
        }
    }

    fn performance_editor_page(&self) -> Page {
        match self.performance_draft {
            Some(PerformanceDraft::Rack(_)) => Page::ConfigRackEditor,
            Some(PerformanceDraft::Song(_)) => Page::ConfigSongEditor,
            Some(PerformanceDraft::Setlist(_)) => Page::ConfigSetlistEditor,
            None => self.performance_return_page,
        }
    }

    fn performance_list_page(&self) -> Page {
        match self.performance_draft {
            Some(PerformanceDraft::Rack(_)) => Page::ConfigRacks,
            Some(PerformanceDraft::Song(_)) => Page::ConfigSongs,
            Some(PerformanceDraft::Setlist(_)) => Page::ConfigSetlists,
            None => Page::Config,
        }
    }

    fn leave_performance_editor(&mut self) {
        self.performance_return_page = self.performance_list_page();
        if self.performance_dirty {
            self.performance_confirm = performance_confirmation();
            self.page = Page::PerformanceUnsaved;
        } else {
            self.performance_draft = None;
            self.page = self.performance_return_page;
        }
    }

    fn activate_performance_editor_item(&mut self) {
        match self.performance_editor_index {
            0 => {
                self.performance_name.set_value(
                    self.performance_draft
                        .as_ref()
                        .map_or("NEW ITEM", PerformanceDraft::name),
                );
                self.page = match self.performance_draft {
                    Some(PerformanceDraft::Rack(_)) => Page::ConfigRackName,
                    Some(PerformanceDraft::Song(_)) => Page::ConfigSongName,
                    _ => Page::ConfigSetlistName,
                };
            }
            1 => match self.performance_draft {
                Some(PerformanceDraft::Rack(_)) => {
                    self.performance_child_index = 0;
                    self.page = Page::ConfigRackSlots;
                }
                Some(PerformanceDraft::Song(_)) => {
                    self.performance_child_index = 0;
                    self.page = Page::ConfigSongParts;
                }
                Some(PerformanceDraft::Setlist(_)) => {
                    self.performance_child_index = 0;
                    self.page = Page::ConfigSetlistEntries;
                }
                None => {}
            },
            2 => {
                if let Some(draft) = self.performance_draft.as_mut() {
                    draft.set_enabled(!draft.enabled());
                    self.performance_dirty = true;
                }
            }
            3 => self.save_performance_draft(),
            _ => {
                if self.performance_draft_exists() {
                    self.performance_confirm = delete_confirmation();
                    self.page = Page::PerformanceDelete;
                } else {
                    let page = self.performance_list_page();
                    self.performance_draft = None;
                    self.performance_dirty = false;
                    self.page = page;
                }
            }
        }
    }

    fn performance_draft_exists(&self) -> bool {
        match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Rack(value)) => {
                self.performance_library.rack(&value.id).is_some()
            }
            Some(PerformanceDraft::Song(value)) => {
                self.performance_library.song(&value.id).is_some()
            }
            Some(PerformanceDraft::Setlist(value)) => {
                self.performance_library.setlist(&value.id).is_some()
            }
            None => false,
        }
    }

    fn save_performance_draft(&mut self) {
        let Some(draft) = self.performance_draft.clone() else {
            return;
        };
        if !self.keyboard_parts_editing {
            self.performance_return_page = self.performance_list_page();
        }
        self.pending_command = Some(MenuCommand::EditPerformance {
            expected_revision: self.performance_revision.clone(),
            edit: draft.into_edit(),
        });
        self.page = Page::PerformanceBusy;
    }

    fn delete_performance_draft(&mut self) {
        let Some(draft) = self.performance_draft.as_ref() else {
            return;
        };
        let edit = match draft {
            PerformanceDraft::Rack(value) => PerformanceEdit::DeleteRack {
                rack_id: value.id.clone(),
            },
            PerformanceDraft::Song(value) => PerformanceEdit::DeleteSong {
                song_id: value.id.clone(),
            },
            PerformanceDraft::Setlist(value) => PerformanceEdit::DeleteSetlist {
                setlist_id: value.id.clone(),
            },
        };
        self.performance_return_page = self.performance_list_page();
        self.pending_command = Some(MenuCommand::EditPerformance {
            expected_revision: self.performance_revision.clone(),
            edit,
        });
        self.page = Page::PerformanceBusy;
    }

    fn show_performance_error(&mut self, message: &str) {
        self.performance_result = Some((false, message.into()));
        self.performance_return_page = self.page;
        self.page = Page::PerformanceResult;
    }

    fn apply_rack_slots_input(&mut self, previous: bool, next: bool, select: bool, back: bool) {
        let len = match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Rack(rack)) => rack.slots.len() + 1,
            _ => return,
        };
        if previous || next {
            move_wrapped(
                &mut self.performance_child_index,
                len,
                if previous { -1 } else { 1 },
            );
        } else if back {
            self.page = Page::ConfigRackEditor;
        } else if select && self.performance_child_index == 0 {
            let state_id = self
                .plugin_active_sound_id
                .clone()
                .or_else(|| self.plugin_sounds.first().map(|sound| sound.id.clone()));
            if let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_mut() {
                let id =
                    next_available_id("slot", rack.slots.iter().map(|value| value.id.as_str()));
                rack.slots.push(RackSlot {
                    id: RackSlotId::new(id).unwrap(),
                    name: format!("SLOT {}", rack.slots.len() + 1),
                    plugin_id: self.active_plugin_id.clone(),
                    state: None,
                    legacy_program_id: state_id,
                    enabled: true,
                    midi_input_channel: None,
                    midi_note_low: 0,
                    midi_note_high: 127,
                    midi_transpose: 0,
                    midi_output: MidiOutputRoute::None,
                    audio_output_bus: "main".into(),
                    level_per_mille: 1_000,
                    pan_per_mille: 0,
                });
                self.performance_child_index = rack.slots.len();
                self.performance_dirty = true;
            }
        } else if select {
            self.performance_child_index -= 1;
            self.performance_child_editor_index = 0;
            self.page = Page::ConfigRackSlotEditor;
        }
    }

    fn apply_rack_slot_editor_input(
        &mut self,
        previous: bool,
        next: bool,
        select: bool,
        back: bool,
    ) {
        if self.keyboard_parts_editing {
            self.apply_keyboard_part_editor_input(previous, next, select, back);
            return;
        }
        if previous || next {
            move_wrapped(
                &mut self.performance_child_editor_index,
                RACK_SLOT_EDITOR_ITEMS.len(),
                if previous { -1 } else { 1 },
            );
        } else if back {
            if self.keyboard_parts_editing {
                self.performance_return_page = Page::KeyboardParts;
                if self.performance_dirty {
                    self.performance_confirm = performance_confirmation();
                    self.page = Page::PerformanceUnsaved;
                } else {
                    self.performance_draft = None;
                    self.keyboard_parts_editing = false;
                    self.page = Page::KeyboardParts;
                }
            } else {
                self.performance_child_index += 1;
                self.page = Page::ConfigRackSlots;
            }
        } else if select {
            match self.performance_child_editor_index {
                0 => {
                    if let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_ref() {
                        self.performance_child_name
                            .set_value(&rack.slots[self.performance_child_index].name);
                    }
                    self.page = Page::ConfigRackSlotName;
                }
                1 => {
                    let plugin_id = self.focused_rack_slot().map(|slot| slot.plugin_id.clone());
                    if let Some(index) = plugin_id.and_then(|id| {
                        self.play_plugins
                            .iter()
                            .position(|plugin| plugin.plugin_id == id)
                    }) {
                        self.play_index = index;
                    }
                    self.page = Page::ConfigRackSlotPlugin;
                }
                2 => {
                    self.performance_value_index = self
                        .focused_rack_slot()
                        .and_then(|slot| slot.midi_input_channel)
                        .map_or(0, usize::from);
                    self.page = Page::ConfigRackSlotMidiChannel;
                }
                3 => {
                    self.performance_value_index = self
                        .focused_rack_slot()
                        .map_or(0, |slot| usize::from(slot.midi_note_low));
                    self.page = Page::ConfigRackSlotLowKey;
                }
                4 => {
                    self.performance_value_index = self
                        .focused_rack_slot()
                        .map_or(127, |slot| usize::from(slot.midi_note_high));
                    self.page = Page::ConfigRackSlotHighKey;
                }
                5 => {
                    self.performance_value_index = self.focused_rack_slot().map_or(4, |slot| {
                        usize::try_from(i16::from(slot.midi_transpose) / 12 + 4).unwrap_or(4)
                    });
                    self.page = Page::ConfigRackSlotOctave;
                }
                6 => {
                    self.page = Page::ConfigRackSlotMidiOutput;
                }
                7 => self.page = Page::ConfigRackSlotAudioOutput,
                8 => {
                    self.performance_value_index = self
                        .focused_rack_slot()
                        .map_or(100, |slot| usize::from(slot.level_per_mille / 10));
                    self.page = Page::ConfigRackSlotLevel;
                }
                9 => {
                    self.performance_value_index = self.focused_rack_slot().map_or(100, |slot| {
                        (i32::from(slot.pan_per_mille) / 10 + 100) as usize
                    });
                    self.page = Page::ConfigRackSlotPan;
                }
                10 => {
                    if let Some(slot) = self.focused_rack_slot_mut() {
                        slot.enabled = !slot.enabled;
                        self.performance_dirty = true;
                    }
                }
                _ => {
                    if let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_mut() {
                        rack.slots.remove(self.performance_child_index);
                        self.performance_dirty = true;
                        self.performance_child_index = self
                            .performance_child_index
                            .min(rack.slots.len().saturating_sub(1))
                            + 1;
                        self.page = Page::ConfigRackSlots;
                    }
                }
            }
        }
    }

    fn apply_keyboard_part_editor_input(
        &mut self,
        previous: bool,
        next: bool,
        select: bool,
        back: bool,
    ) {
        if previous || next {
            move_wrapped(
                &mut self.performance_child_editor_index,
                KEYBOARD_PART_EDITOR_ITEMS.len(),
                if previous { -1 } else { 1 },
            );
        } else if back {
            self.performance_return_page = Page::KeyboardParts;
            if self.performance_dirty {
                self.performance_confirm = performance_confirmation();
                self.page = Page::PerformanceUnsaved;
            } else {
                self.performance_draft = None;
                self.keyboard_parts_editing = false;
                self.page = Page::KeyboardParts;
            }
        } else if select {
            match self.performance_child_editor_index {
                0 => {
                    self.performance_value_index =
                        usize::from(self.keyboard_part_config().midi_channel);
                    self.page = Page::ConfigRackSlotMidiChannel;
                }
                1 => {
                    self.performance_value_index = usize::from(self.keyboard_split_key());
                    self.page = Page::ConfigRackSlotLowKey;
                }
                2 => {
                    self.performance_value_index =
                        usize::try_from(i16::from(self.keyboard_part_config().transpose) / 12 + 4)
                            .unwrap_or(4);
                    self.page = Page::ConfigRackSlotOctave;
                }
                _ => {}
            }
        }
    }

    fn focused_rack_slot(&self) -> Option<&RackSlot> {
        match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Rack(rack)) => rack.slots.get(self.performance_child_index),
            _ => None,
        }
    }

    fn focused_rack_slot_mut(&mut self) -> Option<&mut RackSlot> {
        match self.performance_draft.as_mut() {
            Some(PerformanceDraft::Rack(rack)) => rack.slots.get_mut(self.performance_child_index),
            _ => None,
        }
    }

    fn update_rack_slot_midi_channel(&mut self) {
        let value = self.performance_value_index;
        if self.keyboard_parts_editing {
            if let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_mut() {
                let parts = rack.keyboard_parts.get_or_insert_default();
                if self.keyboard_parts_index == 1 {
                    parts.part_2.midi_channel = value.clamp(1, 16) as u8;
                } else {
                    parts.part_1.midi_channel = value.clamp(1, 16) as u8;
                }
                self.performance_dirty = true;
            }
            return;
        }
        if let Some(slot) = self.focused_rack_slot_mut() {
            slot.midi_input_channel = (value != 0).then_some(value as u8);
            self.performance_dirty = true;
        }
    }

    fn update_rack_slot_low_key(&mut self) {
        let value = self.performance_value_index as u8;
        if let Some(slot) = self.focused_rack_slot_mut() {
            slot.midi_note_low = value.min(slot.midi_note_high);
            self.performance_dirty = true;
        }
    }

    fn keyboard_split_key(&self) -> u8 {
        self.keyboard_parts_config().split_key.unwrap_or(60)
    }

    fn update_keyboard_split_key(&mut self) {
        let split = (self.performance_value_index as u8).clamp(1, 127);
        let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_mut() else {
            return;
        };
        rack.keyboard_parts.get_or_insert_default().split_key = Some(split);
        self.performance_dirty = true;
    }

    fn update_rack_slot_high_key(&mut self) {
        let value = self.performance_value_index as u8;
        if let Some(slot) = self.focused_rack_slot_mut() {
            slot.midi_note_high = value.max(slot.midi_note_low).min(127);
            self.performance_dirty = true;
        }
    }

    fn update_rack_slot_octave(&mut self) {
        let value = (self.performance_value_index as i8 - 4) * 12;
        if self.keyboard_parts_editing {
            if let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_mut() {
                let parts = rack.keyboard_parts.get_or_insert_default();
                if self.keyboard_parts_index == 1 {
                    parts.part_2.transpose = value;
                } else {
                    parts.part_1.transpose = value;
                }
                self.performance_dirty = true;
            }
            return;
        }
        if let Some(slot) = self.focused_rack_slot_mut() {
            slot.midi_transpose = value;
            self.performance_dirty = true;
        }
    }

    fn update_rack_slot_level(&mut self) {
        let value = self.performance_value_index as u16 * 10;
        if let Some(slot) = self.focused_rack_slot_mut() {
            slot.level_per_mille = value;
            self.performance_dirty = true;
        }
    }

    fn update_rack_slot_pan(&mut self) {
        let value = (self.performance_value_index as i16 - 100) * 10;
        if let Some(slot) = self.focused_rack_slot_mut() {
            slot.pan_per_mille = value;
            self.performance_dirty = true;
        }
    }

    fn apply_song_parts_input(&mut self, previous: bool, next: bool, select: bool, back: bool) {
        let len = match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Song(song)) => song.parts.len() + 1,
            _ => return,
        };
        if previous || next {
            move_wrapped(
                &mut self.performance_child_index,
                len,
                if previous { -1 } else { 1 },
            );
        } else if back {
            self.page = Page::ConfigSongEditor;
        } else if select && self.performance_child_index == 0 {
            let Some(rack) = self.performance_library.racks.first() else {
                return;
            };
            if let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_mut() {
                let id =
                    next_available_id("part", song.parts.iter().map(|value| value.id.as_str()));
                song.parts.push(SongPart {
                    id: SongPartId::new(id).unwrap(),
                    name: format!("PART {}", song.parts.len() + 1),
                    rack_id: rack.id.clone(),
                    content: None,
                    patterns: Vec::new(),
                });
                self.performance_child_index = song.parts.len();
                self.performance_dirty = true;
            }
        } else if select {
            self.performance_child_index -= 1;
            self.performance_child_editor_index = 0;
            self.page = Page::ConfigSongPartEditor;
        }
    }

    fn apply_song_part_editor_input(
        &mut self,
        previous: bool,
        next: bool,
        select: bool,
        back: bool,
    ) {
        if previous || next {
            move_wrapped(
                &mut self.performance_child_editor_index,
                PART_EDITOR_ITEMS.len(),
                if previous { -1 } else { 1 },
            );
        } else if back {
            self.performance_child_index += 1;
            self.page = Page::ConfigSongParts;
        } else if select {
            match self.performance_child_editor_index {
                0 => {
                    if let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_ref() {
                        self.performance_child_name
                            .set_value(&song.parts[self.performance_child_index].name);
                    }
                    self.page = Page::ConfigSongPartName;
                }
                1 => {
                    if let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_ref() {
                        self.performance_child_editor_index = self
                            .performance_library
                            .racks
                            .iter()
                            .position(|rack| {
                                rack.id == song.parts[self.performance_child_index].rack_id
                            })
                            .unwrap_or(0);
                    }
                    self.page = Page::ConfigSongPartRack;
                }
                2 => self.move_song_part(-1),
                3 => self.move_song_part(1),
                _ => {
                    if let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_mut()
                        && song.parts.len() > 1
                    {
                        song.parts.remove(self.performance_child_index);
                        self.performance_dirty = true;
                        self.performance_child_index =
                            self.performance_child_index.min(song.parts.len() - 1) + 1;
                        self.page = Page::ConfigSongParts;
                    }
                }
            }
        }
    }

    fn move_song_part(&mut self, delta: isize) {
        if let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_mut() {
            let target = (self.performance_child_index as isize + delta)
                .clamp(0, song.parts.len().saturating_sub(1) as isize)
                as usize;
            if target != self.performance_child_index {
                song.parts.swap(self.performance_child_index, target);
                self.performance_child_index = target;
                self.performance_dirty = true;
            }
        }
    }

    fn apply_setlist_entries_input(
        &mut self,
        previous: bool,
        next: bool,
        select: bool,
        back: bool,
    ) {
        let len = match self.performance_draft.as_ref() {
            Some(PerformanceDraft::Setlist(value)) => value.entries.len() + 1,
            _ => return,
        };
        if previous || next {
            move_wrapped(
                &mut self.performance_child_index,
                len,
                if previous { -1 } else { 1 },
            );
        } else if back {
            self.page = Page::ConfigSetlistEditor;
        } else if select && self.performance_child_index == 0 {
            let Some(song) = self.performance_library.songs.first() else {
                return;
            };
            if let Some(PerformanceDraft::Setlist(setlist)) = self.performance_draft.as_mut() {
                let id = next_available_id(
                    "entry",
                    setlist.entries.iter().map(|value| value.id.as_str()),
                );
                setlist.entries.push(SetlistEntry {
                    id: SetlistEntryId::new(id).unwrap(),
                    song_id: song.id.clone(),
                });
                self.performance_dirty = true;
                self.performance_child_index = setlist.entries.len();
            }
        } else if select {
            self.performance_child_index -= 1;
            self.performance_child_editor_index = 0;
            self.page = Page::ConfigSetlistEntryEditor;
        }
    }

    fn apply_setlist_entry_editor_input(
        &mut self,
        previous: bool,
        next: bool,
        select: bool,
        back: bool,
    ) {
        if previous || next {
            move_wrapped(
                &mut self.performance_child_editor_index,
                ENTRY_EDITOR_ITEMS.len(),
                if previous { -1 } else { 1 },
            );
        } else if back {
            self.performance_child_index += 1;
            self.page = Page::ConfigSetlistEntries;
        } else if select {
            match self.performance_child_editor_index {
                0 => {
                    if let Some(PerformanceDraft::Setlist(setlist)) =
                        self.performance_draft.as_ref()
                    {
                        self.performance_child_editor_index = self
                            .performance_library
                            .songs
                            .iter()
                            .position(|song| {
                                song.id == setlist.entries[self.performance_child_index].song_id
                            })
                            .unwrap_or(0);
                    }
                    self.page = Page::ConfigSetlistEntrySong;
                }
                1 => self.move_setlist_entry(-1),
                2 => self.move_setlist_entry(1),
                _ => {
                    if let Some(PerformanceDraft::Setlist(setlist)) =
                        self.performance_draft.as_mut()
                        && setlist.entries.len() > 1
                    {
                        setlist.entries.remove(self.performance_child_index);
                        self.performance_dirty = true;
                        self.performance_child_index =
                            self.performance_child_index.min(setlist.entries.len() - 1) + 1;
                        self.page = Page::ConfigSetlistEntries;
                    }
                }
            }
        }
    }

    fn move_setlist_entry(&mut self, delta: isize) {
        if let Some(PerformanceDraft::Setlist(setlist)) = self.performance_draft.as_mut() {
            let target = (self.performance_child_index as isize + delta)
                .clamp(0, setlist.entries.len().saturating_sub(1) as isize)
                as usize;
            if target != self.performance_child_index {
                setlist.entries.swap(self.performance_child_index, target);
                self.performance_child_index = target;
                self.performance_dirty = true;
            }
        }
    }

    pub fn apply_input_and_render(&mut self, input: Input) -> Screen {
        self.apply_input(input);
        self.render()
    }

    pub fn apply(&mut self, action: Action) {
        if self.is_plugin_loading() {
            return;
        }
        if self.is_performance_config_page() {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_performance_input(input);
            return;
        }
        if matches!(
            self.page,
            Page::SystemWifi
                | Page::SystemWifiNetworks
                | Page::SystemWifiKnown
                | Page::SystemWifiKnownActions
                | Page::SystemWifiDiscovered
                | Page::SystemWifiDiscoveredActions
                | Page::SystemWifiPassword
                | Page::SystemWifiBusy
                | Page::SystemWifiResult
        ) {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            if self.page == Page::SystemWifi {
                self.apply_system_wifi_input(input);
            } else {
                self.apply_wifi_network_input(input);
            }
            return;
        }
        if matches!(
            self.page,
            Page::Audio
                | Page::AudioOutput
                | Page::AudioRate
                | Page::AudioLatency
                | Page::AudioBusy
                | Page::AudioResult
        ) {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_audio_input(input);
            return;
        }
        if matches!(
            self.page,
            Page::ProgramEditorRoot | Page::ProgramEditorSection | Page::ProgramEditorSound
        ) {
            self.apply_generic_editor_action(action);
            return;
        }
        if self.page == Page::ProgramEditorField {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_generic_editor_field_input(input);
            return;
        }
        if self.page == Page::PluginParameterValue {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_plugin_parameter_input(input);
            return;
        }
        if self.page == Page::PluginUnsavedChanges {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_unsaved_changes_input(input);
            return;
        }
        if matches!(
            self.page,
            Page::PluginPresetName | Page::PluginPresetBusy | Page::PluginPresetResult
        ) {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_plugin_preset_input(input);
            return;
        }
        if self.page == Page::PluginName {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_program_name_input(input);
            return;
        }
        if self.page == Page::PluginProgramOutput {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_program_output_input(input);
            return;
        }
        if self.is_layer_parameter_page() {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_layer_parameter_input(input);
            return;
        }
        match action {
            Action::Previous => self.move_selection(-1),
            Action::Next => self.move_selection(1),
            Action::Back => {
                self.page = match self.page {
                    Page::LiveRacks | Page::LiveSongs | Page::LiveSetlists => Page::Live,
                    Page::LiveSongParts => Page::LiveSongs,
                    Page::LiveSetlistEntries => Page::LiveSetlists,
                    Page::PluginPlay if self.plugin_banks().len() > 1 => Page::PluginPrograms,
                    Page::PluginPlay | Page::PluginPresets | Page::PluginPrograms => {
                        Page::PluginLibrary
                    }
                    Page::PluginParameters if self.plugin_parameter_pages().len() > 1 => {
                        Page::PluginParameterPages
                    }
                    Page::PluginParameters | Page::PluginParameterPages => Page::PluginLibrary,
                    Page::PluginLibrary
                        if self.plugin_play_context == PluginPlayContext::RackSlot =>
                    {
                        self.plugin_play_context = PluginPlayContext::Standalone;
                        Page::ConfigRackSlotPlugin
                    }
                    Page::PluginLibrary => Page::Play,
                    Page::PluginCustomPrograms | Page::PluginConfigUnavailable => Page::Plugins,
                    Page::PluginName | Page::PluginSharedFx | Page::PluginProgramOutput => {
                        Page::PluginProgramSections
                    }
                    Page::PluginLayerMenu => Page::PluginProgramSections,
                    Page::PluginTimbre
                    | Page::PluginEnvelope
                    | Page::PluginPitchEnvelope
                    | Page::PluginRange
                    | Page::PluginLfo
                    | Page::PluginTuning
                    | Page::PluginLayerLevel => Page::PluginLayerMenu,
                    Page::PluginProgramSections => {
                        if self.program_draft.as_ref().is_some_and(|draft| draft.dirty) {
                            self.open_unsaved_changes(ProgramExitDestination::CustomPrograms);
                            return;
                        }
                        if let Some(draft_id) =
                            self.program_draft.as_ref().map(|draft| draft.draft_id)
                        {
                            self.pending_command =
                                Some(MenuCommand::CancelProgramEdit { draft_id });
                            return;
                        }
                        Page::PluginCustomPrograms
                    }
                    Page::Plugins => Page::Config,
                    Page::Audio => Page::Config,
                    Page::AudioOutput | Page::AudioRate | Page::AudioLatency => Page::Audio,
                    Page::AudioBusy | Page::AudioResult => Page::Audio,
                    Page::SystemWeb => Page::System,
                    Page::SystemWifi => Page::System,
                    Page::SystemWifiNetworks | Page::SystemWifiBusy | Page::SystemWifiResult => {
                        Page::SystemWifi
                    }
                    Page::SystemWifiKnown | Page::SystemWifiDiscovered => Page::SystemWifiNetworks,
                    Page::SystemWifiKnownActions => Page::SystemWifiKnown,
                    Page::SystemWifiDiscoveredActions | Page::SystemWifiPassword => {
                        Page::SystemWifiDiscovered
                    }
                    Page::System => Page::Config,
                    Page::Home => Page::Home,
                    _ => Page::Home,
                };
            }
            Action::Select => {
                if self.page == Page::PluginPlay {
                    if let Some(sound) = self
                        .filtered_sounds()
                        .get(self.plugin_play_index)
                        .map(|sound| (*sound).clone())
                    {
                        if self.plugin_play_context == PluginPlayContext::RackSlot {
                            if let Some(slot) = self.focused_rack_slot_mut() {
                                // The selected native program only initializes this draft. Core
                                // materializes it into an opaque plugin state before persistence.
                                slot.state = None;
                                slot.legacy_program_id = Some(sound.id.clone());
                                self.performance_dirty = true;
                            }
                            if let Some(PerformanceDraft::Rack(rack)) =
                                self.performance_draft.clone()
                            {
                                self.pending_command = Some(MenuCommand::PreviewRack { rack });
                            }
                        } else {
                            self.plugin_active_preset_id = None;
                            self.pending_command = Some(MenuCommand::SelectSound {
                                id: sound.id.clone(),
                            });
                        }
                    }
                    return;
                }
                if let Some(location) = self.focused_live_location()
                    && matches!(
                        self.page,
                        Page::LiveRacks | Page::LiveSongParts | Page::LiveSetlistEntries
                    )
                {
                    self.pending_command = Some(MenuCommand::ActivateLiveTarget { location });
                    return;
                }
                self.page = match self.page {
                    Page::Home => match self.home_index {
                        0 => {
                            self.active_mode = ActiveMode::Live;
                            self.pending_command = Some(MenuCommand::SetActiveMode {
                                mode: ActiveMode::Live,
                            });
                            Page::Live
                        }
                        1 => {
                            self.active_mode = ActiveMode::Play;
                            self.pending_command = Some(MenuCommand::SetActiveMode {
                                mode: ActiveMode::Play,
                            });
                            Page::Play
                        }
                        _ => Page::Config,
                    },
                    Page::Play if !self.play_plugins.is_empty() => {
                        if let Some(plugin) = self.play_plugins.get(self.play_index).cloned()
                            && self.active_plugin_instance_id.as_deref()
                                != Some(plugin.instance_id.as_str())
                        {
                            self.pending_command = Some(MenuCommand::SelectPlugin {
                                instance_id: plugin.instance_id.clone(),
                            });
                            self.begin_plugin_loading(plugin.instance_id);
                        }
                        self.plugin_play_context = PluginPlayContext::Standalone;
                        Page::PluginLibrary
                    }
                    Page::Live => {
                        let mode = match self.live_index {
                            0 => LiveBrowseMode::Rack,
                            1 => LiveBrowseMode::Song,
                            _ => LiveBrowseMode::Setlist,
                        };
                        self.live_performance.mode = mode;
                        self.pending_command = Some(MenuCommand::SetLiveBrowseMode { mode });
                        match mode {
                            LiveBrowseMode::Rack => Page::LiveRacks,
                            LiveBrowseMode::Song => Page::LiveSongs,
                            LiveBrowseMode::Setlist => Page::LiveSetlists,
                        }
                    }
                    Page::LiveSongs if !self.live_songs().is_empty() => {
                        self.live_song_part_index = 0;
                        Page::LiveSongParts
                    }
                    Page::LiveSetlists if !self.live_setlists().is_empty() => {
                        self.live_setlist_entry_index = 0;
                        Page::LiveSetlistEntries
                    }
                    Page::PluginLibrary => {
                        if self.plugin_play_context == PluginPlayContext::Standalone
                            && self.plugin_library_index == 0
                        {
                            Page::PluginPresets
                        } else if self.plugin_play_context == PluginPlayContext::Standalone
                            && self.plugin_library_index == 2
                            && !self.plugin_parameter_pages().is_empty()
                        {
                            self.plugin_parameter_page_index = 0;
                            self.plugin_parameter_index = 0;
                            if self.plugin_parameter_pages().len() > 1 {
                                Page::PluginParameterPages
                            } else {
                                Page::PluginParameters
                            }
                        } else {
                            self.plugin_program_bank_index = 0;
                            self.plugin_play_index = 0;
                            if self.plugin_banks().len() > 1 {
                                Page::PluginPrograms
                            } else {
                                Page::PluginPlay
                            }
                        }
                    }
                    Page::PluginPresets if self.plugin_preset_index == 0 => {
                        self.open_plugin_preset_name();
                        self.page
                    }
                    Page::PluginPresets => {
                        if let Some(preset) = self
                            .plugin_preset_index
                            .checked_sub(1)
                            .and_then(|index| self.plugin_presets.get(index))
                        {
                            self.pending_command = Some(MenuCommand::LoadPluginPreset {
                                preset_id: preset.id.clone(),
                            });
                        }
                        Page::PluginPresets
                    }
                    Page::PluginPrograms => {
                        self.plugin_play_index = 0;
                        Page::PluginPlay
                    }
                    Page::PluginParameterPages => {
                        self.plugin_parameter_index = 0;
                        Page::PluginParameters
                    }
                    Page::PluginParameters if !self.current_plugin_parameters().is_empty() => {
                        self.open_plugin_parameter();
                        self.page
                    }
                    Page::Config if self.config_index == 0 => {
                        self.performance_list_index = 0;
                        Page::ConfigRacks
                    }
                    Page::Config if self.config_index == 1 => {
                        self.performance_list_index = 0;
                        Page::ConfigSongs
                    }
                    Page::Config if self.config_index == 2 => {
                        self.performance_list_index = 0;
                        Page::ConfigSetlists
                    }
                    Page::Config if self.config_index == 3 => Page::Plugins,
                    Page::Config if self.config_index == 4 => Page::Audio,
                    Page::Config if self.config_index == 5 => Page::System,
                    Page::Plugins if self.plugin_index == 0 => {
                        if self.active_plugin_config_available {
                            Page::PluginCustomPrograms
                        } else {
                            Page::PluginConfigUnavailable
                        }
                    }
                    Page::System if self.system_index == 0 => Page::SystemWeb,
                    Page::System if self.system_index == 1 && self.wifi_settings.available => {
                        Page::SystemWifi
                    }
                    Page::PluginCustomPrograms => {
                        self.begin_program_edit();
                        Page::PluginCustomPrograms
                    }
                    Page::PluginProgramSections => match self.plugin_section_index {
                        0 => Page::PluginName,
                        1 | 2 => {
                            self.plugin_layer_index = self.plugin_section_index - 1;
                            self.plugin_layer_option_index = 0;
                            self.sync_layer_editors();
                            Page::PluginLayerMenu
                        }
                        3 => Page::PluginSharedFx,
                        4 => Page::PluginProgramOutput,
                        _ => {
                            if let Some(draft_id) =
                                self.program_draft.as_ref().map(|draft| draft.draft_id)
                            {
                                self.pending_command =
                                    Some(MenuCommand::SaveProgramDraft { draft_id });
                            }
                            Page::PluginProgramSections
                        }
                    },
                    Page::PluginLayerMenu => {
                        if self.plugin_layer_index == 1 && self.plugin_layer_option_index == 0 {
                            self.toggle_layer_b();
                            Page::PluginLayerMenu
                        } else {
                            let section_index = self.plugin_layer_option_index
                                - usize::from(self.plugin_layer_index == 1);
                            match section_index {
                                0 => {
                                    if let Some(source_id) =
                                        self.program_layer_source_id(self.plugin_layer_index)
                                        && let Some(index) = self
                                            .base_sounds()
                                            .iter()
                                            .position(|sound| sound.id == source_id)
                                    {
                                        self.plugin_timbre_index = index;
                                    }
                                    Page::PluginTimbre
                                }
                                1 => Page::PluginEnvelope,
                                2 => Page::PluginPitchEnvelope,
                                3 => Page::PluginLfo,
                                4 => Page::PluginTuning,
                                5 => Page::PluginRange,
                                _ => Page::PluginLayerLevel,
                            }
                        }
                    }
                    Page::PluginTimbre => {
                        let sound_id = self
                            .base_sounds()
                            .get(self.plugin_timbre_index)
                            .map(|sound| sound.id.clone());
                        if let Some(sound_id) = sound_id
                            && let Some(draft_id) =
                                self.program_draft.as_ref().map(|draft| draft.draft_id)
                        {
                            self.pending_command = Some(MenuCommand::EditProgramDraftField {
                                draft_id,
                                field_id: format!(
                                    "layer.{}.sound",
                                    if self.plugin_layer_index == 0 {
                                        "a"
                                    } else {
                                        "b"
                                    }
                                ),
                                value: ProgramEditorValue::SoundId(sound_id),
                                preview: false,
                            });
                        }
                        Page::PluginTimbre
                    }
                    page => page,
                };
            }
        }
    }

    pub fn complete_return_to_active_mode(
        &mut self,
        mode: ActiveMode,
        focus_sound_id: Option<&str>,
    ) {
        self.active_mode = mode;
        match mode {
            ActiveMode::Idle => {
                self.page = Page::Home;
            }
            ActiveMode::Live => {
                self.page = match self.live_performance.mode {
                    LiveBrowseMode::Rack if !self.live_racks().is_empty() => Page::LiveRacks,
                    LiveBrowseMode::Song if self.selected_live_song().is_some() => {
                        Page::LiveSongParts
                    }
                    LiveBrowseMode::Setlist if !self.selected_setlist_parts().is_empty() => {
                        Page::LiveSetlistEntries
                    }
                    _ => Page::Live,
                };
            }
            ActiveMode::Play => {
                self.plugin_play_context = PluginPlayContext::Standalone;
                let focus_sound_id = focus_sound_id.or(self.play_anchor_sound_id.as_deref());
                if let Some(sound) = focus_sound_id
                    .and_then(|id| self.plugin_sounds.iter().find(|sound| sound.id == id))
                {
                    self.plugin_program_bank_index =
                        self.plugin_bank_index(&sound.bank).unwrap_or(0);
                    self.plugin_play_index = self
                        .filtered_sounds()
                        .iter()
                        .position(|candidate| candidate.id == sound.id)
                        .unwrap_or(0);
                }
                self.page = Page::PluginPlay;
            }
        }
    }

    pub fn render(&self) -> Screen {
        let mut screen = match self.page {
            Page::Home => {
                let [line_1, line_2] = render_home(self.home_index);
                Screen::with_header(HOME_HEADER, line_1, line_2)
            }
            Page::Live => simple_screen(
                indexed_title("LIVE", self.live_index, LIVE_ITEMS.len()),
                &LIVE_ITEMS,
                &LIVE_DETAILS,
                self.live_index,
            ),
            Page::LiveRacks => self.render_live_racks(),
            Page::LiveSongs => self.render_live_songs(),
            Page::LiveSongParts => self.render_live_song_parts(),
            Page::LiveSetlists => self.render_live_setlists(),
            Page::LiveSetlistEntries => self.render_live_setlist_entries(),
            Page::Play => Screen::with_header(
                indexed_title("PLAY", self.play_index, self.play_plugins.len().max(1)),
                self.play_plugins
                    .get(self.play_index)
                    .map_or(self.active_plugin_name.as_str(), |plugin| {
                        plugin.name.as_str()
                    }),
                "Plugin PLAY",
            ),
            Page::Config => {
                let detail = if self.config_index == 4 {
                    self.audio_device_name()
                        .unwrap_or_else(|| "Detecting output".into())
                } else {
                    CONFIG_DETAILS[self.config_index].into()
                };
                Screen::with_header(
                    indexed_title("CONFIG", self.config_index, CONFIG_ITEMS.len()),
                    CONFIG_ITEMS[self.config_index],
                    detail,
                )
            }
            Page::ConfigRacks => self.render_performance_list("RACKS"),
            Page::ConfigSongs => self.render_performance_list("SONGS"),
            Page::ConfigSetlists => self.render_performance_list("SETLISTS"),
            Page::ConfigRackEditor => self.render_performance_editor("RACK", &RACK_EDITOR_ITEMS),
            Page::ConfigSongEditor => self.render_performance_editor("SONG", &SONG_EDITOR_ITEMS),
            Page::ConfigSetlistEditor => {
                self.render_performance_editor("SETLIST", &SETLIST_EDITOR_ITEMS)
            }
            Page::ConfigRackName | Page::ConfigSongName | Page::ConfigSetlistName => {
                let [line_1, line_2] = component_lines(&self.performance_name, false);
                Screen::with_header("NAME", line_1, line_2)
            }
            Page::ConfigRackSlots => self.render_rack_slots(),
            Page::ConfigRackSlotEditor => self.render_rack_slot_editor(),
            Page::ConfigRackSlotName => {
                let [line_1, line_2] = component_lines(&self.performance_child_name, false);
                Screen::with_header("SLOT NAME", line_1, line_2)
            }
            Page::ConfigRackSlotPlugin => self.render_rack_slot_plugin(),
            Page::ConfigRackSlotMidiChannel => self.render_rack_slot_value(
                "MIDI CH",
                if self.performance_value_index == 0 {
                    "OMNI".into()
                } else {
                    self.performance_value_index.to_string()
                },
                "Input channel",
            ),
            Page::ConfigRackSlotLowKey => self.render_rack_slot_value(
                if self.keyboard_parts_editing {
                    "SPLIT KEY"
                } else {
                    "LOW KEY"
                },
                midi_note_name(self.performance_value_index as u8),
                if self.keyboard_parts_editing {
                    "Right zone starts"
                } else {
                    "Lower zone limit"
                },
            ),
            Page::ConfigRackSlotHighKey => self.render_rack_slot_value(
                "HIGH KEY",
                midi_note_name(self.performance_value_index as u8),
                "Upper zone limit",
            ),
            Page::ConfigRackSlotOctave => {
                let octave = self.performance_value_index as i8 - 4;
                self.render_rack_slot_value(
                    "OCTAVE",
                    if octave == 0 {
                        "ORIGINAL".into()
                    } else {
                        format!("{octave:+}")
                    },
                    "Zone transpose",
                )
            }
            Page::ConfigRackSlotMidiOutput => {
                self.render_rack_slot_value("MIDI OUT", "N/A", "Plugin has no MIDI output")
            }
            Page::ConfigRackSlotAudioOutput => {
                self.render_rack_slot_value("AUDIO OUT", "MAIN", "Audio destination")
            }
            Page::ConfigRackSlotLevel => self.render_rack_slot_value(
                "LEVEL",
                format!("{}%", self.performance_value_index),
                "Slot mix level",
            ),
            Page::ConfigRackSlotPan => {
                let pan = self.performance_value_index as i16 - 100;
                self.render_rack_slot_value(
                    "PAN",
                    if pan == 0 {
                        "CENTER".into()
                    } else if pan < 0 {
                        format!("L{}", -pan)
                    } else {
                        format!("R{pan}")
                    },
                    "Slot stereo pan",
                )
            }
            Page::ConfigSongParts => self.render_song_parts(),
            Page::ConfigSongPartEditor => self.render_song_part_editor(),
            Page::ConfigSongPartName => {
                let [line_1, line_2] = component_lines(&self.performance_child_name, false);
                Screen::with_header("PART NAME", line_1, line_2)
            }
            Page::ConfigSongPartRack => self.render_reference_picker("RACK"),
            Page::ConfigSetlistEntries => self.render_setlist_entries(),
            Page::ConfigSetlistEntryEditor => self.render_setlist_entry_editor(),
            Page::ConfigSetlistEntrySong => self.render_reference_picker("SONG"),
            Page::PerformanceUnsaved | Page::PerformanceDelete => {
                let [line_1, line_2] = component_lines(&self.performance_confirm, true);
                Screen::with_header("PERFORMANCE", line_1, line_2)
            }
            Page::PerformanceBusy => {
                let [line_1, line_2] = component_lines(&self.performance_spinner, false);
                Screen::with_header("PERFORMANCE", line_1, line_2)
            }
            Page::PerformanceResult => {
                let (success, message) = self
                    .performance_result
                    .as_ref()
                    .cloned()
                    .unwrap_or((false, "UNKNOWN".into()));
                Screen::with_header(
                    "PERFORMANCE",
                    if success { "SUCCESS" } else { "ERROR" },
                    message,
                )
            }
            Page::KeyboardParts => self.render_keyboard_parts(),
            Page::Plugins => Screen::with_header(
                indexed_title("PLUGINS", self.plugin_index, 1),
                &self.active_plugin_name,
                "Plugin settings",
            ),
            Page::PluginConfigUnavailable => {
                self.plugin_screen("CONFIG", "CONFIG UNAVAILABLE", "USE PLAY + PRESETS")
            }
            Page::System => {
                let (item, detail) = self.system_item();
                Screen::with_header(
                    indexed_title("SYSTEM", self.system_index, self.system_item_count()),
                    item,
                    detail,
                )
            }
            Page::Audio => self.render_audio(),
            Page::AudioOutput | Page::AudioRate | Page::AudioLatency => self.render_audio_value(),
            Page::AudioBusy => {
                let [line_1, line_2] = component_lines(&self.audio_spinner, false);
                Screen::with_header("AUDIO", line_1, line_2)
            }
            Page::AudioResult => self.render_audio_result(),
            Page::SystemWeb => self.render_system_web(),
            Page::SystemWifi => self.render_system_wifi(),
            Page::SystemWifiNetworks => self.render_wifi_networks(),
            Page::SystemWifiKnown => self.render_wifi_known(),
            Page::SystemWifiKnownActions => self.render_wifi_known_actions(),
            Page::SystemWifiDiscovered => self.render_wifi_discovered(),
            Page::SystemWifiDiscoveredActions => self.render_wifi_discovered_actions(),
            Page::SystemWifiPassword => {
                let [line_1, line_2] = component_lines(&self.wifi_password, false);
                Screen::with_header("WI-FI PASSWORD", line_1, line_2)
            }
            Page::SystemWifiBusy => {
                let [line_1, line_2] = component_lines(&self.wifi_spinner, false);
                Screen::with_header("WI-FI", line_1, line_2)
            }
            Page::SystemWifiResult => self.render_wifi_result(),
            Page::PluginLibrary
            | Page::PluginPresets
            | Page::PluginPrograms
            | Page::PluginPlay
            | Page::PluginParameterPages
            | Page::PluginParameters
            | Page::PluginParameterValue
                if self.pending_plugin_instance_id.is_some() =>
            {
                self.render_plugin_loading()
            }
            Page::PluginLibrary => self.render_plugin_library(),
            Page::PluginPresets => self.render_plugin_presets(),
            Page::PluginPresetName => {
                let [line_1, line_2] = component_lines(&self.plugin_preset_name, false);
                self.plugin_screen("SAVE PRESET", line_1, line_2)
            }
            Page::PluginPresetBusy => {
                let [line_1, line_2] = component_lines(&self.plugin_spinner, false);
                self.plugin_screen("PRESETS", line_1, line_2)
            }
            Page::PluginPresetResult => {
                let (success, message) = self
                    .plugin_preset_result
                    .as_ref()
                    .map_or((false, "SAVE FAILED"), |(success, message)| {
                        (*success, message.as_str())
                    });
                self.plugin_screen(
                    "PRESETS",
                    if success { "SUCCESS" } else { "ERROR" },
                    message,
                )
            }
            Page::PluginPrograms => self.render_plugin_programs(),
            Page::PluginPlay => self.render_plugin_play(),
            Page::PluginParameterPages => self.render_plugin_parameter_pages(),
            Page::PluginParameters => self.render_plugin_parameters(),
            Page::PluginParameterValue => self.render_plugin_parameter_value(),
            Page::PluginCustomPrograms => self.render_custom_programs(),
            Page::PluginProgramSections => self.plugin_simple_screen(
                format!(
                    "EDIT {}/{}",
                    self.plugin_section_index + 1,
                    PLUGIN_PROGRAM_SECTIONS.len()
                ),
                &PLUGIN_PROGRAM_SECTIONS,
                &PLUGIN_SECTION_DETAILS,
                self.plugin_section_index,
            ),
            Page::PluginName => {
                let [line_1, line_2] = component_lines(&self.program_name, false);
                self.plugin_screen("NAME", line_1, line_2)
            }
            Page::PluginLayerMenu => self.render_layer_menu(),
            Page::PluginTimbre => self.render_timbre(),
            Page::PluginEnvelope => {
                let [line_1, line_2] = component_lines(&self.envelope, true);
                self.plugin_screen("AMP ENV", line_1, line_2)
            }
            Page::PluginPitchEnvelope => {
                let [line_1, line_2] = component_lines(&self.pitch_envelope, true);
                self.plugin_screen(self.layer_header("PITCH ENV"), line_1, line_2)
            }
            Page::PluginLfo => {
                let [line_1, line_2] = component_lines(&self.lfo, true);
                self.plugin_screen(self.layer_header("LFO"), line_1, line_2)
            }
            Page::PluginTuning => {
                let [line_1, line_2] = component_lines(&self.tuning, true);
                self.plugin_screen(self.layer_header("TUNING"), line_1, line_2)
            }
            Page::PluginRange => {
                let [line_1, line_2] = component_lines(&self.range, true);
                self.plugin_screen(self.layer_header("RANGE"), line_1, line_2)
            }
            Page::PluginLayerLevel => {
                let [line_1, line_2] = component_lines(&self.layer_level, true);
                self.plugin_screen(self.layer_header("VOLUME"), line_1, line_2)
            }
            Page::PluginSharedFx => self.plugin_screen("FX", "NO FX", "Chain is empty"),
            Page::PluginProgramOutput => {
                let [line_1, line_2] = component_lines(&self.program_output, true);
                self.plugin_screen("OUTPUT", line_1, line_2)
            }
            Page::PluginUnsavedChanges => {
                let [line_1, line_2] = component_lines(&self.unsaved_changes, true);
                self.plugin_screen("UNSAVED", line_1, line_2)
            }
            Page::ProgramEditorRoot => self.render_generic_editor_root(),
            Page::ProgramEditorSection => self.render_generic_editor_page(),
            Page::ProgramEditorField => self.render_generic_editor_field(),
            Page::ProgramEditorSound => self.render_generic_editor_sound(),
        };
        screen.footer = standard_footer(self.pressed_button);
        screen
    }

    fn render_performance_list(&self, title: &str) -> Screen {
        let (count, name, detail) = match self.page {
            Page::ConfigRacks => {
                let item = self
                    .performance_list_index
                    .checked_sub(1)
                    .and_then(|index| self.performance_library.racks.get(index));
                (
                    self.performance_library.racks.len() + 1,
                    item.map_or("+ ADD NEW", |value| value.name.as_str()),
                    item.map_or("Create Rack".into(), |value| {
                        format!(
                            "{} SLOT{}",
                            value.slots.len(),
                            if value.slots.len() == 1 { "" } else { "S" }
                        )
                    }),
                )
            }
            Page::ConfigSongs => {
                let item = self
                    .performance_list_index
                    .checked_sub(1)
                    .and_then(|index| self.performance_library.songs.get(index));
                (
                    self.performance_library.songs.len() + 1,
                    item.map_or("+ ADD NEW", |value| value.name.as_str()),
                    item.map_or("Create Song".into(), |value| {
                        format!(
                            "{} PART{}",
                            value.parts.len(),
                            if value.parts.len() == 1 { "" } else { "S" }
                        )
                    }),
                )
            }
            _ => {
                let item = self
                    .performance_list_index
                    .checked_sub(1)
                    .and_then(|index| self.performance_library.setlists.get(index));
                (
                    self.performance_library.setlists.len() + 1,
                    item.map_or("+ ADD NEW", |value| value.name.as_str()),
                    item.map_or("Create Setlist".into(), |value| {
                        format!(
                            "{} SONG{}",
                            value.entries.len(),
                            if value.entries.len() == 1 { "" } else { "S" }
                        )
                    }),
                )
            }
        };
        Screen::with_header(
            indexed_title(title, self.performance_list_index, count),
            normalized_display_text(name, "+ ADD NEW"),
            detail,
        )
    }

    fn render_performance_editor(&self, title: &str, items: &[&str]) -> Screen {
        let item = items[self.performance_editor_index];
        let detail = match self.performance_editor_index {
            0 => self
                .performance_draft
                .as_ref()
                .map_or("UNNAMED".into(), |value| value.name().into()),
            1 => match self.performance_draft.as_ref() {
                Some(PerformanceDraft::Rack(rack)) => format!(
                    "{} SLOT{}",
                    rack.slots.len(),
                    if rack.slots.len() == 1 { "" } else { "S" }
                ),
                Some(PerformanceDraft::Song(song)) => format!(
                    "{} PART{}",
                    song.parts.len(),
                    if song.parts.len() == 1 { "" } else { "S" }
                ),
                Some(PerformanceDraft::Setlist(setlist)) => format!(
                    "{} SONG{}",
                    setlist.entries.len(),
                    if setlist.entries.len() == 1 { "" } else { "S" }
                ),
                None => "EMPTY".into(),
            },
            2 => {
                if self
                    .performance_draft
                    .as_ref()
                    .is_some_and(PerformanceDraft::enabled)
                {
                    "ON".into()
                } else {
                    "OFF".into()
                }
            }
            3 => {
                if self.performance_dirty || !self.performance_draft_exists() {
                    "Store changes".into()
                } else {
                    "No changes".into()
                }
            }
            _ => {
                if self.performance_draft_exists() {
                    "Remove permanently".into()
                } else {
                    "Discard draft".into()
                }
            }
        };
        Screen::with_header(
            indexed_title(title, self.performance_editor_index, items.len()),
            item,
            normalized_display_text(&detail, " "),
        )
    }

    fn render_rack_slots(&self) -> Screen {
        let Some(PerformanceDraft::Rack(rack)) = self.performance_draft.as_ref() else {
            return Screen::with_header("SLOTS", "EMPTY", "BACK");
        };
        if self.performance_child_index == 0 {
            return Screen::with_header(
                indexed_title("SLOTS", 0, rack.slots.len() + 1),
                "+ ADD SLOT",
                "Add plugin",
            );
        }
        let slot = &rack.slots[self.performance_child_index - 1];
        Screen::with_header(
            indexed_title("SLOTS", self.performance_child_index, rack.slots.len() + 1),
            normalized_display_text(&slot.name, "UNNAMED SLOT"),
            " ",
        )
    }

    fn render_keyboard_parts(&self) -> Screen {
        if self.keyboard_parts_rack().is_none() {
            return Screen::with_header("KEYBOARD", "NO ACTIVE RACK", "BACK");
        }
        let count = 2;
        let parts = self.keyboard_parts_config();
        let part = parts.part(self.keyboard_parts_index);
        let range = match (self.keyboard_parts_index, parts.split_key) {
            (0, Some(split)) => format!(
                "{}..{} CH {}",
                midi_note_name(0),
                midi_note_name(split - 1),
                part.midi_channel
            ),
            (1, Some(split)) => format!(
                "{}..{} CH {}",
                midi_note_name(split),
                midi_note_name(127),
                part.midi_channel
            ),
            (0, None) => format!("FULL RANGE CH {}", part.midi_channel),
            _ => format!("NO SPLIT CH {}", part.midi_channel),
        };
        Screen::with_header(
            indexed_title("KEYBOARD", self.keyboard_parts_index, count),
            format!("PART {}", self.keyboard_parts_index + 1),
            normalized_display_text(&range, " "),
        )
    }

    fn render_rack_slot_editor(&self) -> Screen {
        if self.keyboard_parts_editing {
            let part = self.keyboard_part_config();
            let detail = match self.performance_child_editor_index {
                0 => part.midi_channel.to_string(),
                1 => midi_note_name(self.keyboard_split_key()),
                2 => {
                    let octave = part.transpose / 12;
                    if octave == 0 {
                        "ORIGINAL".into()
                    } else {
                        format!("{octave:+}")
                    }
                }
                _ => " ".into(),
            };
            return Screen::with_header(
                indexed_title(
                    &format!("PART {}", self.keyboard_parts_index + 1),
                    self.performance_child_editor_index,
                    KEYBOARD_PART_EDITOR_ITEMS.len(),
                ),
                KEYBOARD_PART_EDITOR_ITEMS[self.performance_child_editor_index],
                normalized_display_text(&detail, " "),
            );
        }
        let Some(slot) = self.focused_rack_slot() else {
            return Screen::with_header("SLOT", "EMPTY", "BACK");
        };
        let detail = match self.performance_child_editor_index {
            0 => slot.name.clone(),
            1 if slot.plugin_id == self.active_plugin_id => self.active_plugin_name.clone(),
            1 => slot.plugin_id.clone(),
            2 => slot
                .midi_input_channel
                .map_or("OMNI".into(), |channel| channel.to_string()),
            3 => midi_note_name(slot.midi_note_low),
            4 => midi_note_name(slot.midi_note_high),
            5 => {
                let octave = slot.midi_transpose / 12;
                if octave == 0 {
                    "ORIGINAL".into()
                } else {
                    format!("{octave:+}")
                }
            }
            6 => match &slot.midi_output {
                MidiOutputRoute::None => "NONE".into(),
                MidiOutputRoute::Bus { bus_id } => bus_id.clone(),
            },
            7 => slot.audio_output_bus.clone(),
            8 => format!("{}%", slot.level_per_mille / 10),
            9 => {
                let pan = slot.pan_per_mille / 10;
                if pan == 0 {
                    "CENTER".into()
                } else if pan < 0 {
                    format!("L{}", -pan)
                } else {
                    format!("R{pan}")
                }
            }
            10 => {
                if slot.enabled {
                    "ON".into()
                } else {
                    "OFF".into()
                }
            }
            _ => "Remove Slot".into(),
        };
        Screen::with_header(
            indexed_title(
                "SLOT",
                self.performance_child_editor_index,
                RACK_SLOT_EDITOR_ITEMS.len(),
            ),
            RACK_SLOT_EDITOR_ITEMS[self.performance_child_editor_index],
            normalized_display_text(&detail, " "),
        )
    }

    fn render_rack_slot_plugin(&self) -> Screen {
        let Some(plugin) = self.play_plugins.get(self.play_index) else {
            return Screen::with_header("PLUGIN", "NO PLUGINS", "BACK");
        };
        Screen::with_header(
            indexed_title("PLUGIN", self.play_index, self.play_plugins.len()),
            &plugin.name,
            "OK opens PLAY",
        )
    }

    fn render_rack_slot_value(
        &self,
        title: &str,
        value: impl Into<String>,
        detail: &str,
    ) -> Screen {
        Screen::with_header(title, normalized_display_text(&value.into(), " "), detail)
    }

    fn render_song_parts(&self) -> Screen {
        let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_ref() else {
            return Screen::with_header("PARTS", "EMPTY", "BACK");
        };
        if self.performance_child_index == 0 {
            return Screen::with_header(
                indexed_title("PARTS", 0, song.parts.len() + 1),
                "+ ADD PART",
                "Append new Part",
            );
        }
        let part = &song.parts[self.performance_child_index - 1];
        let rack = if part.content.is_some() {
            "PART GRAPH"
        } else {
            self.performance_library
                .rack(&part.rack_id)
                .map_or("MISSING RACK", |value| value.name.as_str())
        };
        Screen::with_header(
            indexed_title("PARTS", self.performance_child_index, song.parts.len() + 1),
            &part.name,
            rack,
        )
    }

    fn render_song_part_editor(&self) -> Screen {
        let Some(PerformanceDraft::Song(song)) = self.performance_draft.as_ref() else {
            return Screen::with_header("PART", "EMPTY", "BACK");
        };
        let part = &song.parts[self.performance_child_index];
        let detail = match self.performance_child_editor_index {
            0 => part.name.clone(),
            1 if part.content.is_some() => "Replace graph with Rack".into(),
            1 => self
                .performance_library
                .rack(&part.rack_id)
                .map_or("MISSING RACK".into(), |value| value.name.clone()),
            2 => {
                if self.performance_child_index == 0 {
                    "Already first".into()
                } else {
                    "Move earlier".into()
                }
            }
            3 => {
                if self.performance_child_index + 1 == song.parts.len() {
                    "Already last".into()
                } else {
                    "Move later".into()
                }
            }
            _ => {
                if song.parts.len() == 1 {
                    "One Part required".into()
                } else {
                    "Remove Part".into()
                }
            }
        };
        Screen::with_header(
            indexed_title(
                "PART",
                self.performance_child_editor_index,
                PART_EDITOR_ITEMS.len(),
            ),
            PART_EDITOR_ITEMS[self.performance_child_editor_index],
            detail,
        )
    }

    fn render_setlist_entries(&self) -> Screen {
        let Some(PerformanceDraft::Setlist(setlist)) = self.performance_draft.as_ref() else {
            return Screen::with_header("SONGS", "EMPTY", "BACK");
        };
        if self.performance_child_index == 0 {
            return Screen::with_header(
                indexed_title("SONGS", 0, setlist.entries.len() + 1),
                "+ ADD SONG",
                "Append entry",
            );
        }
        let entry = &setlist.entries[self.performance_child_index - 1];
        let name = self
            .performance_library
            .song(&entry.song_id)
            .map_or("MISSING SONG", |value| value.name.as_str());
        Screen::with_header(
            indexed_title(
                "SONGS",
                self.performance_child_index,
                setlist.entries.len() + 1,
            ),
            name,
            format!("POSITION {}", self.performance_child_index),
        )
    }

    fn render_setlist_entry_editor(&self) -> Screen {
        let Some(PerformanceDraft::Setlist(setlist)) = self.performance_draft.as_ref() else {
            return Screen::with_header("ENTRY", "EMPTY", "BACK");
        };
        let entry = &setlist.entries[self.performance_child_index];
        let detail = match self.performance_child_editor_index {
            0 => self
                .performance_library
                .song(&entry.song_id)
                .map_or("MISSING SONG".into(), |value| value.name.clone()),
            1 => {
                if self.performance_child_index == 0 {
                    "Already first".into()
                } else {
                    "Move earlier".into()
                }
            }
            2 => {
                if self.performance_child_index + 1 == setlist.entries.len() {
                    "Already last".into()
                } else {
                    "Move later".into()
                }
            }
            _ => {
                if setlist.entries.len() == 1 {
                    "One Song required".into()
                } else {
                    "Remove entry".into()
                }
            }
        };
        Screen::with_header(
            indexed_title(
                "ENTRY",
                self.performance_child_editor_index,
                ENTRY_EDITOR_ITEMS.len(),
            ),
            ENTRY_EDITOR_ITEMS[self.performance_child_editor_index],
            detail,
        )
    }

    fn render_reference_picker(&self, kind: &str) -> Screen {
        let (name, count) = if kind == "RACK" {
            (
                self.performance_library
                    .racks
                    .get(self.performance_child_editor_index)
                    .map(|value| value.name.as_str()),
                self.performance_library.racks.len(),
            )
        } else {
            (
                self.performance_library
                    .songs
                    .get(self.performance_child_editor_index)
                    .map(|value| value.name.as_str()),
                self.performance_library.songs.len(),
            )
        };
        Screen::with_header(
            indexed_title(kind, self.performance_child_editor_index, count),
            name.unwrap_or("EMPTY"),
            "OK TO SELECT",
        )
    }

    fn live_racks(&self) -> Vec<&rackforge_performance_api::RackDefinition> {
        self.performance_library
            .racks
            .iter()
            .filter(|rack| rack.enabled)
            .collect()
    }

    fn live_songs(&self) -> Vec<&rackforge_performance_api::SongDefinition> {
        self.performance_library
            .songs
            .iter()
            .filter(|song| song.enabled)
            .collect()
    }

    fn live_setlists(&self) -> Vec<&rackforge_performance_api::SetlistDefinition> {
        self.performance_library
            .setlists
            .iter()
            .filter(|setlist| setlist.enabled)
            .collect()
    }

    fn selected_live_song(&self) -> Option<&rackforge_performance_api::SongDefinition> {
        self.live_songs().get(self.live_song_index).copied()
    }

    fn selected_setlist_parts(
        &self,
    ) -> Vec<(
        &rackforge_performance_api::SetlistEntry,
        &rackforge_performance_api::SongDefinition,
        &rackforge_performance_api::SongPart,
    )> {
        let Some(setlist) = self.live_setlists().get(self.live_setlist_index).copied() else {
            return Vec::new();
        };
        setlist
            .entries
            .iter()
            .filter_map(|entry| {
                self.performance_library
                    .song(&entry.song_id)
                    .filter(|song| song.enabled)
                    .map(|song| (entry, song))
            })
            .flat_map(|(entry, song)| song.parts.iter().map(move |part| (entry, song, part)))
            .collect()
    }

    fn focused_live_location(&self) -> Option<LiveLocation> {
        match self.page {
            Page::LiveRacks => {
                self.live_racks()
                    .get(self.live_rack_index)
                    .map(|rack| LiveLocation::Rack {
                        rack_id: rack.id.clone(),
                    })
            }
            Page::LiveSongParts => self.selected_live_song().and_then(|song| {
                song.parts
                    .get(self.live_song_part_index)
                    .map(|part| LiveLocation::Song {
                        song_id: song.id.clone(),
                        part_id: part.id.clone(),
                    })
            }),
            Page::LiveSetlistEntries => self
                .selected_setlist_parts()
                .get(self.live_setlist_entry_index)
                .map(|(entry, _, part)| LiveLocation::Setlist {
                    setlist_id: self.live_setlists()[self.live_setlist_index].id.clone(),
                    entry_id: entry.id.clone(),
                    part_id: part.id.clone(),
                }),
            _ => None,
        }
    }

    fn render_live_racks(&self) -> Screen {
        let racks = self.live_racks();
        let Some(rack) = racks.get(self.live_rack_index) else {
            return Screen::with_header("RACK 0/0", "EMPTY", "CONFIG > RACKS");
        };
        let active = self.live_performance.active_rack_id.as_ref() == Some(&rack.id);
        Screen::with_header(
            indexed_title("RACK", self.live_rack_index, racks.len()),
            normalized_display_text(&rack.name, "UNNAMED RACK"),
            if active { "ACTIVE" } else { "OK TO LOAD" },
        )
    }

    fn render_live_songs(&self) -> Screen {
        let songs = self.live_songs();
        let Some(song) = songs.get(self.live_song_index) else {
            return Screen::with_header("SONG 0/0", "EMPTY", "CONFIG > SONGS");
        };
        Screen::with_header(
            indexed_title("SONG", self.live_song_index, songs.len()),
            normalized_display_text(&song.name, "UNNAMED SONG"),
            format!(
                "{} PART{}",
                song.parts.len(),
                if song.parts.len() == 1 { "" } else { "S" }
            ),
        )
    }

    fn render_live_song_parts(&self) -> Screen {
        let Some(song) = self.selected_live_song() else {
            return Screen::with_header("SONG", "EMPTY", "BACK");
        };
        let Some(part) = song.parts.get(self.live_song_part_index) else {
            return Screen::with_header("SONG", "NO PARTS", "BACK");
        };
        let location = LiveLocation::Song {
            song_id: song.id.clone(),
            part_id: part.id.clone(),
        };
        let rack_name = self
            .performance_library
            .rack(&part.rack_id)
            .map(|rack| rack.name.as_str())
            .unwrap_or("MISSING RACK");
        let detail = if self.live_performance.active.as_ref() == Some(&location) {
            "ACTIVE".to_owned()
        } else {
            rack_name.to_owned()
        };
        Screen::with_header(
            indexed_title(
                &normalized_display_text(&song.name, "SONG"),
                self.live_song_part_index,
                song.parts.len(),
            ),
            normalized_display_text(&part.name, "UNNAMED PART"),
            normalized_display_text(&detail, "OK TO LOAD"),
        )
    }

    fn render_live_setlists(&self) -> Screen {
        let setlists = self.live_setlists();
        let Some(setlist) = setlists.get(self.live_setlist_index) else {
            return Screen::with_header("SETLIST 0/0", "EMPTY", "CONFIG > SETS");
        };
        Screen::with_header(
            indexed_title("SETLIST", self.live_setlist_index, setlists.len()),
            normalized_display_text(&setlist.name, "UNNAMED SETLIST"),
            format!(
                "{} SONG{}",
                setlist.entries.len(),
                if setlist.entries.len() == 1 { "" } else { "S" }
            ),
        )
    }

    fn render_live_setlist_entries(&self) -> Screen {
        let setlists = self.live_setlists();
        let Some(setlist) = setlists.get(self.live_setlist_index) else {
            return Screen::with_header("SETLIST", "EMPTY", "BACK");
        };
        let parts = self.selected_setlist_parts();
        let Some((entry, song, part)) = parts.get(self.live_setlist_entry_index) else {
            return Screen::with_header("SETLIST", "NO SONGS", "BACK");
        };
        let location = LiveLocation::Setlist {
            setlist_id: setlist.id.clone(),
            entry_id: entry.id.clone(),
            part_id: part.id.clone(),
        };
        let detail = if self.live_performance.active.as_ref() == Some(&location) {
            format!("{} ACTIVE", part.name)
        } else {
            part.name.clone()
        };
        Screen::with_header(
            indexed_title(
                &normalized_display_text(&setlist.name, "SETLIST"),
                self.live_setlist_entry_index,
                parts.len(),
            ),
            normalized_display_text(&song.name, "UNNAMED SONG"),
            normalized_display_text(&detail, "UNNAMED PART"),
        )
    }

    fn render_audio(&self) -> Screen {
        let detail = match (&self.audio_state, self.audio_index) {
            (Some(state), 0) => normalized_display_text(&state.active_device.name, "OUTPUT"),
            (Some(state), 1) => format!("{} HZ", state.active_profile.sample_rate_hz),
            (Some(state), _) => format!(
                "{:.1} MS {}/{}",
                state.active_profile.nominal_buffer_latency_ms(),
                state.active_profile.period_frames,
                state.active_profile.buffer_frames
            ),
            (None, _) => "UNAVAILABLE".into(),
        };
        Screen::with_header(
            indexed_title("AUDIO", self.audio_index, AUDIO_ITEMS.len()),
            AUDIO_ITEMS[self.audio_index],
            normalized_display_text(&detail, "UNAVAILABLE"),
        )
    }

    fn render_audio_value(&self) -> Screen {
        let (title, value, count) = match self.page {
            Page::AudioOutput => {
                let devices = self.compatible_audio_devices();
                let value = devices
                    .get(self.audio_value_index)
                    .map(|device| device.name.clone())
                    .unwrap_or_else(|| "NO OUTPUTS".into());
                ("OUTPUT", value, devices.len())
            }
            Page::AudioRate => {
                let rates = self.audio_rates();
                let value = rates
                    .get(self.audio_value_index)
                    .map(|rate| format!("{rate} HZ"))
                    .unwrap_or_else(|| "NO RATES".into());
                ("SAMPLE RATE", value, rates.len())
            }
            Page::AudioLatency => {
                let latencies = self.audio_latencies();
                let value = latencies
                    .get(self.audio_value_index)
                    .map(|(label, _, _)| (*label).to_owned())
                    .unwrap_or_else(|| "NO PRESETS".into());
                ("LATENCY", value, latencies.len())
            }
            _ => unreachable!(),
        };
        let value = normalized_display_text(&value, "UNAVAILABLE")
            .chars()
            .take(DISPLAY_COLUMNS.saturating_sub(2))
            .collect::<String>();
        Screen::with_header(
            indexed_title(
                title,
                self.audio_value_index.min(count.saturating_sub(1)),
                count.max(1),
            ),
            format!("[{value}]"),
            "OK TO APPLY",
        )
    }

    fn render_audio_result(&self) -> Screen {
        let (success, message) = self
            .audio_result
            .as_ref()
            .map(|(success, message)| (*success, message.as_str()))
            .unwrap_or((false, "UNKNOWN RESULT"));
        Screen::with_header(
            "AUDIO",
            if success { "APPLIED" } else { "FAILED" },
            normalized_display_text(message, "UNKNOWN ERROR"),
        )
    }

    fn render_system_web(&self) -> Screen {
        let settings = if self.system_web_editing {
            self.web_edit_candidate
        } else {
            self.web_settings
        };
        let value = match self.system_web_index {
            0 => {
                if settings.enabled {
                    "ON".to_owned()
                } else {
                    "OFF".to_owned()
                }
            }
            1 => match settings.access {
                WebAccess::Local => "LOCAL ONLY".to_owned(),
                WebAccess::Lan => "LAN".to_owned(),
            },
            2 => match (settings.access, settings.lan_ip) {
                (WebAccess::Lan, Some([a, b, c, d])) => {
                    format!("{a}.{b}.{c}.{d}:{}", settings.port)
                }
                (WebAccess::Lan, None) => "NO LAN ADDRESS".to_owned(),
                (WebAccess::Local, _) => "ENABLE LAN FIRST".to_owned(),
            },
            3 => settings.port.to_string(),
            4 => {
                if settings.pin_set {
                    "SET".to_owned()
                } else {
                    "NOT SET".to_owned()
                }
            }
            _ => {
                if settings.service_online {
                    "ONLINE".to_owned()
                } else {
                    "OFFLINE".to_owned()
                }
            }
        };
        let value = if self.system_web_editing {
            format!("[{value}]")
        } else {
            value
        };
        Screen::with_header(
            indexed_title("WEB", self.system_web_index, SYSTEM_WEB_ITEMS.len()),
            SYSTEM_WEB_ITEMS[self.system_web_index],
            value,
        )
    }

    fn render_system_wifi(&self) -> Screen {
        let value = match self.system_wifi_index {
            0 => match (
                self.wifi_settings.connected,
                self.wifi_settings.ssid.as_deref(),
                self.wifi_settings.signal_percent,
            ) {
                (true, Some(ssid), Some(signal)) => format!("{ssid} {signal}%"),
                (true, Some(ssid), None) => ssid.to_owned(),
                (true, None, _) => "CONNECTED".into(),
                (false, _, _) if self.wifi_settings.enabled => "NOT CONNECTED".into(),
                _ => "RADIO OFF".into(),
            },
            1 => format!("{} KNOWN", self.wifi_settings.saved_networks.len()),
            _ => {
                let enabled = if self.wifi_radio_editing {
                    self.wifi_radio_candidate
                } else {
                    self.wifi_settings.enabled
                };
                let value = if enabled { "ON" } else { "OFF" };
                if self.wifi_radio_editing {
                    format!("[{value}]")
                } else {
                    value.into()
                }
            }
        };
        Screen::with_header(
            indexed_title("WI-FI", self.system_wifi_index, SYSTEM_WIFI_ITEMS.len()),
            SYSTEM_WIFI_ITEMS[self.system_wifi_index],
            value,
        )
    }

    fn render_wifi_networks(&self) -> Screen {
        let detail = if self.wifi_networks_index == 0 {
            format!("{} SAVED", self.wifi_settings.saved_networks.len())
        } else if self.wifi_settings.enabled {
            "SCAN FOR NETWORKS".to_owned()
        } else {
            "RADIO OFF".to_owned()
        };
        Screen::with_header(
            indexed_title(
                "NETWORKS",
                self.wifi_networks_index,
                WIFI_NETWORK_GROUPS.len(),
            ),
            WIFI_NETWORK_GROUPS[self.wifi_networks_index],
            detail,
        )
    }

    fn render_wifi_known(&self) -> Screen {
        let Some(network) = self.wifi_settings.saved_networks.get(self.wifi_saved_index) else {
            return Screen::with_header("KNOWN", "NO NETWORKS", "BACK TO RETURN");
        };
        let name = network.ssid.as_deref().unwrap_or(&network.name);
        Screen::with_header(
            indexed_title(
                "KNOWN",
                self.wifi_saved_index,
                self.wifi_settings.saved_networks.len(),
            ),
            normalized_display_text(name, "UNNAMED"),
            if network.active { "CONNECTED" } else { "SAVED" },
        )
    }

    fn render_wifi_known_actions(&self) -> Screen {
        let Some(network) = self.wifi_settings.saved_networks.get(self.wifi_saved_index) else {
            return Screen::with_header("KNOWN", "NO NETWORK", "BACK TO RETURN");
        };
        let actions = if network.active {
            &WIFI_ACTIVE_ACTIONS
        } else {
            &WIFI_KNOWN_ACTIONS
        };
        Screen::with_header(
            normalized_display_text(network.ssid.as_deref().unwrap_or(&network.name), "KNOWN"),
            actions[self.wifi_known_action_index],
            if self.wifi_known_action_index == 0 {
                "PRESS OK"
            } else {
                "REMOVE PROFILE"
            },
        )
    }

    fn render_wifi_discovered(&self) -> Screen {
        let Some(network) = self
            .wifi_discovered_networks
            .get(self.wifi_discovered_index)
        else {
            return Screen::with_header("DISCOVERED", "NO NEW NETWORKS", "BACK TO RETURN");
        };
        Screen::with_header(
            indexed_title(
                "DISCOVERED",
                self.wifi_discovered_index,
                self.wifi_discovered_networks.len(),
            ),
            normalized_display_text(&network.ssid, "UNNAMED"),
            format!(
                "{}% {}",
                network.signal_percent,
                if network.secured { "SECURED" } else { "OPEN" }
            ),
        )
    }

    fn render_wifi_discovered_actions(&self) -> Screen {
        let Some(network) = self
            .wifi_discovered_networks
            .get(self.wifi_discovered_index)
        else {
            return Screen::with_header("DISCOVERED", "NO NETWORK", "BACK TO RETURN");
        };
        Screen::with_header(
            normalized_display_text(&network.ssid, "WI-FI"),
            "CONNECT",
            if network.secured {
                "PASSWORD REQUIRED"
            } else {
                "OPEN NETWORK"
            },
        )
    }

    fn render_wifi_result(&self) -> Screen {
        let Some((success, message)) = self.wifi_result.as_ref() else {
            return Screen::with_header("WI-FI", "READY", "BACK TO RETURN");
        };
        Screen::with_header(
            "WI-FI",
            if *success { "SUCCESS" } else { "FAILED" },
            normalized_display_text(message, "UNKNOWN ERROR"),
        )
    }

    fn apply_audio_input(&mut self, input: Input) {
        match self.page {
            Page::Audio => match input {
                Input::Button2 | Input::EncoderLeft => {
                    self.audio_index =
                        (self.audio_index + AUDIO_ITEMS.len() - 1) % AUDIO_ITEMS.len();
                }
                Input::Button3 | Input::EncoderRight => {
                    self.audio_index = (self.audio_index + 1) % AUDIO_ITEMS.len();
                }
                Input::Button4 => self.page = Page::Config,
                Input::Button1 | Input::EncoderPress => {
                    self.audio_value_index = self.current_audio_value_index();
                    self.page = match self.audio_index {
                        0 => Page::AudioOutput,
                        1 => Page::AudioRate,
                        _ => Page::AudioLatency,
                    };
                }
                _ => {}
            },
            Page::AudioOutput | Page::AudioRate | Page::AudioLatency => match input {
                Input::Button2 | Input::EncoderLeft => {
                    let len = self.audio_value_count();
                    if len > 0 {
                        self.audio_value_index = (self.audio_value_index + len - 1) % len;
                    }
                }
                Input::Button3 | Input::EncoderRight => {
                    let len = self.audio_value_count();
                    if len > 0 {
                        self.audio_value_index = (self.audio_value_index + 1) % len;
                    }
                }
                Input::Button4 => self.page = Page::Audio,
                Input::Button1 | Input::EncoderPress => {
                    if let Some(profile) = self.selected_audio_profile() {
                        self.pending_command = Some(MenuCommand::ApplyAudioOutput { profile });
                    }
                }
                _ => {}
            },
            Page::AudioResult => {
                if matches!(input, Input::Button1 | Input::Button4 | Input::EncoderPress) {
                    self.page = Page::Audio;
                }
            }
            Page::AudioBusy => {}
            _ => {}
        }
    }

    fn compatible_audio_devices(&self) -> Vec<&AudioDeviceDescriptor> {
        self.audio_state
            .as_ref()
            .into_iter()
            .flat_map(|state| &state.devices)
            .filter(|device| {
                device.playback.as_ref().is_some_and(|playback| {
                    playback.sample_formats.contains(&AudioSampleFormat::S32Le)
                        && playback.channels.contains(2)
                })
            })
            .collect()
    }

    fn audio_rates(&self) -> Vec<u32> {
        self.audio_state
            .as_ref()
            .and_then(|state| state.active_device.playback.as_ref())
            .map(|playback| playback.sample_rates_hz.clone())
            .unwrap_or_default()
    }

    fn audio_latencies(&self) -> Vec<(&'static str, u32, u32)> {
        let Some(playback) = self
            .audio_state
            .as_ref()
            .and_then(|state| state.active_device.playback.as_ref())
        else {
            return Vec::new();
        };
        AUDIO_LATENCIES
            .into_iter()
            .filter(|(_, period, buffer)| {
                playback.period_frames.contains(*period) && playback.buffer_frames.contains(*buffer)
            })
            .collect()
    }

    fn current_audio_value_index(&self) -> usize {
        let Some(state) = &self.audio_state else {
            return 0;
        };
        match self.audio_index {
            0 => self
                .compatible_audio_devices()
                .iter()
                .position(|device| device.id == state.active_device.id)
                .unwrap_or(0),
            1 => self
                .audio_rates()
                .iter()
                .position(|rate| *rate == state.active_profile.sample_rate_hz)
                .unwrap_or(0),
            _ => self
                .audio_latencies()
                .iter()
                .position(|(_, period, buffer)| {
                    *period == state.active_profile.period_frames
                        && *buffer == state.active_profile.buffer_frames
                })
                .unwrap_or(0),
        }
    }

    fn audio_value_count(&self) -> usize {
        match self.page {
            Page::AudioOutput => self.compatible_audio_devices().len(),
            Page::AudioRate => self.audio_rates().len(),
            Page::AudioLatency => self.audio_latencies().len(),
            _ => 0,
        }
    }

    fn selected_audio_profile(&self) -> Option<AudioOutputProfile> {
        let state = self.audio_state.as_ref()?;
        let mut profile = state.active_profile.clone();
        match self.page {
            Page::AudioOutput => {
                let devices = self.compatible_audio_devices();
                let device = devices.get(self.audio_value_index)?;
                let playback = device.playback.as_ref()?;
                profile.device = AudioDeviceSelector::Id {
                    id: device.id.clone(),
                };
                profile.fallback = AudioFallbackPolicy::None;
                if !playback.sample_rates_hz.contains(&profile.sample_rate_hz) {
                    profile.sample_rate_hz = playback
                        .sample_rates_hz
                        .iter()
                        .copied()
                        .find(|rate| *rate == 48_000)
                        .or_else(|| playback.sample_rates_hz.first().copied())?;
                }
                if !playback.period_frames.contains(profile.period_frames)
                    || !playback.buffer_frames.contains(profile.buffer_frames)
                {
                    let (_, period, buffer) =
                        AUDIO_LATENCIES.into_iter().find(|(_, period, buffer)| {
                            playback.period_frames.contains(*period)
                                && playback.buffer_frames.contains(*buffer)
                        })?;
                    profile.period_frames = period;
                    profile.buffer_frames = buffer;
                }
            }
            Page::AudioRate => {
                profile.sample_rate_hz = *self.audio_rates().get(self.audio_value_index)?;
            }
            Page::AudioLatency => {
                let (_, period, buffer) = *self.audio_latencies().get(self.audio_value_index)?;
                profile.period_frames = period;
                profile.buffer_frames = buffer;
            }
            _ => return None,
        }
        Some(profile)
    }

    fn apply_system_web_input(&mut self, input: Input) {
        if self.system_web_editing {
            match input {
                Input::Button2 | Input::EncoderLeft => self.adjust_web_candidate(input, -1),
                Input::Button3 | Input::EncoderRight => self.adjust_web_candidate(input, 1),
                Input::Button1 | Input::EncoderPress => {
                    self.system_web_editing = false;
                    self.web_settings = self.web_edit_candidate;
                    self.pending_command = match self.system_web_index {
                        0 => Some(MenuCommand::SetWebEnabled {
                            enabled: self.web_settings.enabled,
                        }),
                        1 => Some(MenuCommand::SetWebAccess {
                            access: self.web_settings.access,
                        }),
                        3 => Some(MenuCommand::SetWebPort {
                            port: self.web_settings.port,
                        }),
                        _ => None,
                    };
                }
                Input::Button4 => {
                    self.system_web_editing = false;
                    self.web_edit_candidate = self.web_settings;
                }
                _ => {}
            }
            return;
        }

        match input {
            Input::Button2 | Input::EncoderLeft => {
                self.system_web_index = self
                    .system_web_index
                    .checked_sub(1)
                    .unwrap_or(SYSTEM_WEB_ITEMS.len() - 1);
            }
            Input::Button3 | Input::EncoderRight => {
                self.system_web_index = (self.system_web_index + 1) % SYSTEM_WEB_ITEMS.len();
            }
            Input::Button4 => self.page = Page::System,
            Input::Button1 | Input::EncoderPress => match self.system_web_index {
                0 | 1 | 3 => {
                    self.web_edit_candidate = self.web_settings;
                    self.system_web_editing = true;
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn adjust_web_candidate(&mut self, input: Input, direction: i32) {
        match self.system_web_index {
            0 => self.web_edit_candidate.enabled = !self.web_edit_candidate.enabled,
            1 => {
                self.web_edit_candidate.access = match self.web_edit_candidate.access {
                    WebAccess::Local => WebAccess::Lan,
                    WebAccess::Lan => WebAccess::Local,
                };
            }
            3 => {
                let step = if matches!(input, Input::EncoderLeft | Input::EncoderRight) {
                    10
                } else {
                    1
                };
                self.web_edit_candidate.port = i32::from(self.web_edit_candidate.port)
                    .saturating_add(direction * step)
                    .clamp(1024, i32::from(u16::MAX))
                    as u16;
            }
            _ => {}
        }
    }

    fn apply_system_wifi_input(&mut self, input: Input) {
        if self.wifi_radio_editing {
            match input {
                Input::Button2 | Input::Button3 | Input::EncoderLeft | Input::EncoderRight => {
                    self.wifi_radio_candidate = !self.wifi_radio_candidate;
                }
                Input::Button1 | Input::EncoderPress => {
                    self.wifi_radio_editing = false;
                    self.pending_command = Some(MenuCommand::SetWifiEnabled {
                        enabled: self.wifi_radio_candidate,
                    });
                }
                Input::Button4 => {
                    self.wifi_radio_editing = false;
                    self.wifi_radio_candidate = self.wifi_settings.enabled;
                }
                _ => {}
            }
            return;
        }

        match input {
            Input::Button2 | Input::EncoderLeft => {
                self.system_wifi_index = self
                    .system_wifi_index
                    .checked_sub(1)
                    .unwrap_or(SYSTEM_WIFI_ITEMS.len() - 1);
            }
            Input::Button3 | Input::EncoderRight => {
                self.system_wifi_index = (self.system_wifi_index + 1) % SYSTEM_WIFI_ITEMS.len();
            }
            Input::Button4 => self.page = Page::System,
            Input::Button1 | Input::EncoderPress => match self.system_wifi_index {
                1 => {
                    self.wifi_networks_index = 0;
                    self.page = Page::SystemWifiNetworks;
                }
                2 => {
                    self.wifi_radio_candidate = self.wifi_settings.enabled;
                    self.wifi_radio_editing = true;
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn apply_wifi_network_input(&mut self, input: Input) {
        if self.page == Page::SystemWifiPassword {
            match self.wifi_password.handle(input) {
                ComponentEvent::EditCommitted(_) => {
                    if let Some(network) = self
                        .wifi_discovered_networks
                        .get(self.wifi_discovered_index)
                    {
                        self.pending_command = Some(MenuCommand::ConnectDiscoveredWifi {
                            ssid: network.ssid.clone(),
                            passphrase: Some(self.wifi_password.take_secret()),
                        });
                    }
                }
                ComponentEvent::EditCancelled(_) | ComponentEvent::ExitRequested(_) => {
                    self.wifi_password.clear();
                    self.page = Page::SystemWifiDiscoveredActions;
                }
                _ => {}
            }
            return;
        }

        if self.page == Page::SystemWifiResult {
            if matches!(input, Input::Button1 | Input::EncoderPress | Input::Button4) {
                self.wifi_result = None;
                self.page = Page::SystemWifiNetworks;
            }
            return;
        }

        match self.page {
            Page::SystemWifiNetworks => match input {
                Input::Button2 | Input::EncoderLeft => {
                    self.wifi_networks_index = self
                        .wifi_networks_index
                        .checked_sub(1)
                        .unwrap_or(WIFI_NETWORK_GROUPS.len() - 1);
                }
                Input::Button3 | Input::EncoderRight => {
                    self.wifi_networks_index =
                        (self.wifi_networks_index + 1) % WIFI_NETWORK_GROUPS.len();
                }
                Input::Button4 => self.page = Page::SystemWifi,
                Input::Button1 | Input::EncoderPress => {
                    if self.wifi_networks_index == 0 {
                        self.wifi_saved_index = self
                            .wifi_saved_index
                            .min(self.wifi_settings.saved_networks.len().saturating_sub(1));
                        self.page = Page::SystemWifiKnown;
                    } else if self.wifi_settings.enabled {
                        self.wifi_discovered_networks.clear();
                        self.wifi_discovered_index = 0;
                        self.page = Page::SystemWifiDiscovered;
                        self.pending_command = Some(MenuCommand::ScanWifi);
                    }
                }
                _ => {}
            },
            Page::SystemWifiKnown => match input {
                Input::Button2 | Input::EncoderLeft => {
                    let len = self.wifi_settings.saved_networks.len();
                    if len > 0 {
                        self.wifi_saved_index =
                            self.wifi_saved_index.checked_sub(1).unwrap_or(len - 1);
                    }
                }
                Input::Button3 | Input::EncoderRight => {
                    let len = self.wifi_settings.saved_networks.len();
                    if len > 0 {
                        self.wifi_saved_index = (self.wifi_saved_index + 1) % len;
                    }
                }
                Input::Button4 => self.page = Page::SystemWifiNetworks,
                Input::Button1 | Input::EncoderPress
                    if !self.wifi_settings.saved_networks.is_empty() =>
                {
                    self.wifi_known_action_index = 0;
                    self.page = Page::SystemWifiKnownActions;
                }
                _ => {}
            },
            Page::SystemWifiKnownActions => match input {
                Input::Button2 | Input::Button3 | Input::EncoderLeft | Input::EncoderRight => {
                    self.wifi_known_action_index = 1 - self.wifi_known_action_index;
                }
                Input::Button4 => self.page = Page::SystemWifiKnown,
                Input::Button1 | Input::EncoderPress => {
                    if let Some(network) =
                        self.wifi_settings.saved_networks.get(self.wifi_saved_index)
                    {
                        self.pending_command = if self.wifi_known_action_index == 1 {
                            Some(MenuCommand::ForgetSavedWifi {
                                connection_id: network.id.clone(),
                            })
                        } else if network.active {
                            Some(MenuCommand::DisconnectWifi)
                        } else {
                            Some(MenuCommand::ActivateSavedWifi {
                                connection_id: network.id.clone(),
                            })
                        };
                    }
                }
                _ => {}
            },
            Page::SystemWifiDiscovered => match input {
                Input::Button2 | Input::EncoderLeft => {
                    let len = self.wifi_discovered_networks.len();
                    if len > 0 {
                        self.wifi_discovered_index =
                            self.wifi_discovered_index.checked_sub(1).unwrap_or(len - 1);
                    }
                }
                Input::Button3 | Input::EncoderRight => {
                    let len = self.wifi_discovered_networks.len();
                    if len > 0 {
                        self.wifi_discovered_index = (self.wifi_discovered_index + 1) % len;
                    }
                }
                Input::Button4 => self.page = Page::SystemWifiNetworks,
                Input::Button1 | Input::EncoderPress
                    if !self.wifi_discovered_networks.is_empty() =>
                {
                    self.page = Page::SystemWifiDiscoveredActions;
                }
                _ => {}
            },
            Page::SystemWifiDiscoveredActions => match input {
                Input::Button4 => self.page = Page::SystemWifiDiscovered,
                Input::Button1 | Input::EncoderPress => {
                    if let Some(network) = self
                        .wifi_discovered_networks
                        .get(self.wifi_discovered_index)
                    {
                        if network.secured {
                            self.wifi_password.clear();
                            self.page = Page::SystemWifiPassword;
                        } else {
                            self.pending_command = Some(MenuCommand::ConnectDiscoveredWifi {
                                ssid: network.ssid.clone(),
                                passphrase: None,
                            });
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let (selection, len) = match self.page {
            Page::Home => (&mut self.home_index, HOME_ITEMS.len()),
            Page::Live => (&mut self.live_index, LIVE_ITEMS.len()),
            Page::LiveRacks => {
                let len = self.live_racks().len();
                if len == 0 {
                    return;
                }
                (&mut self.live_rack_index, len)
            }
            Page::LiveSongs => {
                let len = self.live_songs().len();
                if len == 0 {
                    return;
                }
                (&mut self.live_song_index, len)
            }
            Page::LiveSongParts => {
                let len = self.selected_live_song().map_or(0, |song| song.parts.len());
                if len == 0 {
                    return;
                }
                (&mut self.live_song_part_index, len)
            }
            Page::LiveSetlists => {
                let len = self.live_setlists().len();
                if len == 0 {
                    return;
                }
                (&mut self.live_setlist_index, len)
            }
            Page::LiveSetlistEntries => {
                let len = self.selected_setlist_parts().len();
                if len == 0 {
                    return;
                }
                (&mut self.live_setlist_entry_index, len)
            }
            Page::Play if self.play_plugins.is_empty() => return,
            Page::Play => (&mut self.play_index, self.play_plugins.len()),
            Page::Config => (&mut self.config_index, CONFIG_ITEMS.len()),
            Page::Plugins => (&mut self.plugin_index, 1),
            Page::PluginConfigUnavailable => return,
            Page::System => {
                let len = self.system_item_count();
                (&mut self.system_index, len)
            }
            Page::SystemWeb => (&mut self.system_web_index, SYSTEM_WEB_ITEMS.len()),
            Page::Audio => (&mut self.audio_index, AUDIO_ITEMS.len()),
            Page::PluginLibrary => {
                let len = if self.plugin_play_context == PluginPlayContext::RackSlot {
                    1
                } else {
                    2 + usize::from(!self.plugin_parameter_pages().is_empty())
                };
                (&mut self.plugin_library_index, len)
            }
            Page::PluginPresets => (&mut self.plugin_preset_index, self.plugin_presets.len() + 1),
            Page::PluginPrograms => {
                let len = self.plugin_banks().len();
                if len == 0 {
                    return;
                }
                (&mut self.plugin_program_bank_index, len)
            }
            Page::PluginPlay if self.filtered_sounds().is_empty() => return,
            Page::PluginPlay => {
                let len = self.filtered_sounds().len();
                (&mut self.plugin_play_index, len)
            }
            Page::PluginParameterPages => {
                let len = self.plugin_parameter_pages().len();
                if len == 0 {
                    return;
                }
                (&mut self.plugin_parameter_page_index, len)
            }
            Page::PluginParameters => {
                let len = self.current_plugin_parameters().len();
                if len == 0 {
                    return;
                }
                (&mut self.plugin_parameter_index, len)
            }
            Page::PluginCustomPrograms => {
                let len = self.editable_sounds().len() + 1;
                (&mut self.plugin_custom_index, len)
            }
            Page::PluginProgramSections => (
                &mut self.plugin_section_index,
                PLUGIN_PROGRAM_SECTIONS.len(),
            ),
            Page::PluginLayerMenu => {
                let len = self.layer_menu_len();
                (&mut self.plugin_layer_option_index, len)
            }
            Page::PluginTimbre if self.base_sounds().is_empty() => return,
            Page::PluginTimbre => {
                let len = self.base_sounds().len();
                (&mut self.plugin_timbre_index, len)
            }
            Page::PluginName => return,
            Page::ConfigRackSlotPlugin if self.play_plugins.is_empty() => return,
            Page::ConfigRackSlotPlugin => (&mut self.play_index, self.play_plugins.len()),
            Page::ConfigRacks
            | Page::ConfigRackEditor
            | Page::ConfigRackName
            | Page::ConfigRackSlots
            | Page::ConfigRackSlotEditor
            | Page::ConfigRackSlotName
            | Page::ConfigRackSlotMidiChannel
            | Page::ConfigRackSlotLowKey
            | Page::ConfigRackSlotHighKey
            | Page::ConfigRackSlotOctave
            | Page::ConfigRackSlotMidiOutput
            | Page::ConfigRackSlotAudioOutput
            | Page::ConfigRackSlotLevel
            | Page::ConfigRackSlotPan
            | Page::ConfigSongs
            | Page::ConfigSongEditor
            | Page::ConfigSongName
            | Page::ConfigSongParts
            | Page::ConfigSongPartEditor
            | Page::ConfigSongPartName
            | Page::ConfigSongPartRack
            | Page::ConfigSetlists
            | Page::ConfigSetlistEditor
            | Page::ConfigSetlistName
            | Page::ConfigSetlistEntries
            | Page::ConfigSetlistEntryEditor
            | Page::ConfigSetlistEntrySong
            | Page::PerformanceUnsaved
            | Page::PerformanceDelete
            | Page::PerformanceBusy
            | Page::PerformanceResult
            | Page::KeyboardParts
            | Page::PluginPresetName
            | Page::PluginPresetBusy
            | Page::PluginPresetResult => return,
            Page::PluginTuning
            | Page::PluginPitchEnvelope
            | Page::PluginLayerLevel
            | Page::PluginSharedFx
            | Page::PluginProgramOutput
            | Page::PluginRange
            | Page::PluginLfo
            | Page::PluginEnvelope
            | Page::PluginUnsavedChanges
            | Page::ProgramEditorRoot
            | Page::ProgramEditorSection
            | Page::ProgramEditorField
            | Page::PluginParameterValue
            | Page::ProgramEditorSound
            | Page::SystemWifi
            | Page::SystemWifiNetworks
            | Page::SystemWifiKnown
            | Page::SystemWifiKnownActions
            | Page::SystemWifiDiscovered
            | Page::SystemWifiDiscoveredActions
            | Page::SystemWifiPassword
            | Page::SystemWifiBusy
            | Page::SystemWifiResult => return,
            Page::AudioOutput
            | Page::AudioRate
            | Page::AudioLatency
            | Page::AudioBusy
            | Page::AudioResult => return,
        };
        *selection = ((*selection as isize + delta).rem_euclid(len as isize)) as usize;
    }

    fn apply_generic_editor_action(&mut self, action: Action) {
        match self.page {
            Page::ProgramEditorSound => match action {
                Action::Previous => self.move_editor_sound(-1),
                Action::Next => self.move_editor_sound(1),
                Action::Back => self.page = Page::ProgramEditorSection,
                Action::Select => self.select_editor_sound(),
            },
            Page::ProgramEditorRoot | Page::ProgramEditorSection => match action {
                Action::Previous => self.move_editor_selection(-1),
                Action::Next => self.move_editor_selection(1),
                Action::Back => self.back_from_generic_editor(),
                Action::Select => self.select_generic_editor_item(),
            },
            _ => {}
        }
    }

    fn move_editor_selection(&mut self, delta: isize) {
        let len = self.generic_editor_item_count();
        if len == 0 {
            return;
        }
        let depth = self.editor_path.len();
        if self.editor_selections.len() <= depth {
            self.editor_selections.resize(depth + 1, 0);
        }
        let selected = &mut self.editor_selections[depth];
        *selected = ((*selected as isize + delta).rem_euclid(len as isize)) as usize;
    }

    fn generic_editor_item_count(&self) -> usize {
        if self.page == Page::ProgramEditorRoot {
            return self
                .program_draft
                .as_ref()
                .map_or(0, |draft| draft.editor.pages.len() + 2);
        }
        self.current_editor_page().map_or(0, |page| {
            page.fields.len() + if page.enabled { page.pages.len() } else { 0 }
        })
    }

    fn current_editor_selection(&self) -> usize {
        self.editor_selections
            .get(self.editor_path.len())
            .copied()
            .unwrap_or(0)
    }

    fn current_editor_page(&self) -> Option<&ProgramEditorPage> {
        let draft = self.program_draft.as_ref()?;
        let mut pages = draft.editor.pages.as_slice();
        let mut current = None;
        for index in &self.editor_path {
            let page = pages.get(*index)?;
            current = Some(page);
            pages = page.pages.as_slice();
        }
        current
    }

    fn select_generic_editor_item(&mut self) {
        if self.page == Page::ProgramEditorRoot {
            let Some(draft) = self.program_draft.as_ref() else {
                return;
            };
            let selected = self.current_editor_selection();
            if selected == 0 {
                self.program_name.set_value(&draft.name);
                self.page = Page::PluginName;
            } else if selected == draft.editor.pages.len() + 1 {
                self.pending_command = Some(MenuCommand::SaveProgramDraft {
                    draft_id: draft.draft_id,
                });
            } else {
                let page = draft.editor.pages[selected - 1].clone();
                self.editor_path.clear();
                self.editor_path.push(selected - 1);
                self.editor_selections.resize(2, 0);
                self.editor_selections[1] = 0;
                self.page = Page::ProgramEditorSection;
                if page.enabled && page.pages.is_empty() && page.fields.len() == 1 {
                    self.open_generic_editor_field(page.fields[0].clone(), false);
                }
            }
            return;
        }

        let selected = self.current_editor_selection();
        let Some(page) = self.current_editor_page().cloned() else {
            return;
        };
        if let Some(field) = page.fields.get(selected) {
            if !page.enabled && !matches!(field.kind, ProgramEditorFieldKind::Toggle) {
                return;
            }
            self.open_generic_editor_field(field.clone(), true);
            return;
        }
        if !page.enabled {
            return;
        }
        let child_index = selected.saturating_sub(page.fields.len());
        if child_index < page.pages.len() {
            let child = page.pages[child_index].clone();
            self.editor_path.push(child_index);
            let depth = self.editor_path.len();
            self.editor_selections.resize(depth + 1, 0);
            self.editor_selections[depth] = 0;
            self.page = Page::ProgramEditorSection;
            if child.pages.is_empty() && child.fields.len() == 1 {
                self.open_generic_editor_field(child.fields[0].clone(), false);
            }
        }
    }

    fn open_generic_editor_field(&mut self, field: ProgramEditorField, begin_edit: bool) {
        match (&field.kind, &field.value) {
            (ProgramEditorFieldKind::Toggle, ProgramEditorValue::Boolean(value)) => {
                self.emit_generic_editor_field(
                    field.id,
                    ProgramEditorValue::Boolean(!value),
                    false,
                );
            }
            (ProgramEditorFieldKind::Sound { bank }, ProgramEditorValue::SoundId(sound_id)) => {
                self.editor_field_id = Some(field.id);
                let matching = self
                    .plugin_sounds
                    .iter()
                    .filter(|sound| bank.as_ref().is_none_or(|bank| sound.bank == *bank))
                    .collect::<Vec<_>>();
                self.plugin_timbre_index = matching
                    .iter()
                    .position(|sound| sound.id == *sound_id)
                    .unwrap_or(0);
                self.page = Page::ProgramEditorSound;
            }
            (ProgramEditorFieldKind::Number { .. }, _)
            | (ProgramEditorFieldKind::Choice { .. }, _) => {
                self.editor_field_id = Some(field.id.clone());
                self.editor_field = editor_field_carousel(&field);
                if self.editor_field.is_some() {
                    self.page = Page::ProgramEditorField;
                    if begin_edit && let Some(editor) = self.editor_field.as_mut() {
                        let _ = editor.handle(Input::Button1);
                    }
                }
            }
            _ => {}
        }
    }

    fn apply_generic_editor_field_input(&mut self, input: Input) {
        let Some(editor) = self.editor_field.as_mut() else {
            self.page = Page::ProgramEditorSection;
            return;
        };
        let event = editor.handle(input);
        match event {
            ComponentEvent::Changed(_) if editor.is_editing() => {
                if self
                    .active_editor_field()
                    .is_some_and(|field| field.live_preview)
                {
                    self.emit_current_generic_editor_value(true);
                }
            }
            ComponentEvent::EditCommitted(_) => {
                self.emit_current_generic_editor_value(false);
                self.close_generic_editor_field();
            }
            ComponentEvent::EditCancelled(_) => {
                self.restore_program_preview();
                self.close_generic_editor_field();
            }
            ComponentEvent::ExitRequested(_) => self.close_generic_editor_field(),
            _ => {}
        }
    }

    fn close_generic_editor_field(&mut self) {
        self.editor_field = None;
        self.editor_field_id = None;
        self.page = Page::ProgramEditorSection;
    }

    fn active_editor_field(&self) -> Option<&ProgramEditorField> {
        let id = self.editor_field_id.as_deref()?;
        self.program_draft
            .as_ref()?
            .editor
            .pages
            .iter()
            .find_map(|page| find_editor_field(page, id))
    }

    fn emit_current_generic_editor_value(&mut self, preview: bool) {
        let Some(field) = self.active_editor_field().cloned() else {
            return;
        };
        let Some(item) = self.editor_field.as_ref().map(ValueCarousel::selected_item) else {
            return;
        };
        let Some(value) = editor_value_from_item(&field, item) else {
            return;
        };
        self.emit_generic_editor_field(field.id, value, preview);
    }

    fn emit_generic_editor_field(
        &mut self,
        field_id: String,
        value: ProgramEditorValue,
        preview: bool,
    ) {
        if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
            self.pending_command = Some(MenuCommand::EditProgramDraftField {
                draft_id,
                field_id,
                value,
                preview,
            });
        }
    }

    fn back_from_generic_editor(&mut self) {
        if self.page == Page::ProgramEditorRoot {
            if self.program_draft.as_ref().is_some_and(|draft| draft.dirty) {
                self.open_unsaved_changes(ProgramExitDestination::CustomPrograms);
            } else if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
                self.pending_command = Some(MenuCommand::CancelProgramEdit { draft_id });
            }
            return;
        }
        self.editor_path.pop();
        self.editor_selections
            .truncate(self.editor_path.len().saturating_add(1));
        self.page = if self.editor_path.is_empty() {
            Page::ProgramEditorRoot
        } else {
            Page::ProgramEditorSection
        };
    }

    fn editor_sound_choices(&self) -> Vec<&PlaySound> {
        let bank = self
            .active_editor_field()
            .and_then(|field| match &field.kind {
                ProgramEditorFieldKind::Sound { bank } => bank.as_deref(),
                _ => None,
            });
        self.plugin_sounds
            .iter()
            .filter(|sound| bank.is_none_or(|bank| sound.bank == bank))
            .collect()
    }

    fn move_editor_sound(&mut self, delta: isize) {
        let len = self.editor_sound_choices().len();
        if len > 0 {
            self.plugin_timbre_index =
                ((self.plugin_timbre_index as isize + delta).rem_euclid(len as isize)) as usize;
        }
    }

    fn select_editor_sound(&mut self) {
        let sound_id = self
            .editor_sound_choices()
            .get(self.plugin_timbre_index)
            .map(|sound| sound.id.clone());
        if let (Some(field_id), Some(sound_id)) = (self.editor_field_id.clone(), sound_id) {
            self.emit_generic_editor_field(field_id, ProgramEditorValue::SoundId(sound_id), false);
        }
    }

    fn apply_layer_parameter_input(&mut self, input: Input) {
        let event = match self.page {
            Page::PluginEnvelope => self.envelope.handle(input),
            Page::PluginPitchEnvelope => self.pitch_envelope.handle(input),
            Page::PluginLfo => self.lfo.handle(input),
            Page::PluginTuning => self.tuning.handle(input),
            Page::PluginRange => self.range.handle(input),
            Page::PluginLayerLevel => self.layer_level.handle(input),
            _ => ComponentEvent::Ignored,
        };
        match event {
            ComponentEvent::Changed(_) if self.layer_editor_is_editing() => {
                self.emit_layer_parameter(true);
            }
            ComponentEvent::EditCommitted(_) => self.emit_layer_parameter(false),
            ComponentEvent::EditCancelled(_) => self.restore_program_preview(),
            ComponentEvent::ExitRequested(_) => self.page = Page::PluginLayerMenu,
            _ => {}
        }
    }

    fn toggle_layer_b(&mut self) {
        let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) else {
            return;
        };
        let enabled = self.program_layer_enabled(1);
        self.pending_command = Some(MenuCommand::EditProgramDraftField {
            draft_id,
            field_id: "layer.b.enabled".into(),
            value: ProgramEditorValue::Boolean(!enabled),
            preview: false,
        });
    }

    fn emit_layer_parameter(&mut self, preview: bool) {
        let item = match self.page {
            Page::PluginEnvelope => self.envelope.selected_item(),
            Page::PluginPitchEnvelope => self.pitch_envelope.selected_item(),
            Page::PluginLfo => self.lfo.selected_item(),
            Page::PluginTuning => self.tuning.selected_item(),
            Page::PluginRange => self.range.selected_item(),
            Page::PluginLayerLevel => self.layer_level.selected_item(),
            _ => return,
        };
        let value = if item.id() == "lfo.enabled" {
            ProgramEditorValue::Choice(match item.value().choice_index() {
                Some(0) => "inherit".into(),
                Some(1) => "on".into(),
                Some(2) => "off".into(),
                _ => return,
            })
        } else if let Some(value) = item.value().as_optional_f64() {
            value.map_or(ProgramEditorValue::Inherited, |value| {
                ProgramEditorValue::Integer(editor_scaled_value(item.id(), value))
            })
        } else if let Some(value) = item.value().as_i64() {
            ProgramEditorValue::Integer(editor_scaled_value(item.id(), value as f64))
        } else if let Some(value) = item.value().as_f64() {
            ProgramEditorValue::Integer(editor_scaled_value(item.id(), value))
        } else {
            return;
        };
        if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
            self.pending_command = Some(MenuCommand::EditProgramDraftField {
                draft_id,
                field_id: editor_layer_field_id(self.plugin_layer_index, item.id()),
                value,
                preview,
            });
        }
    }

    fn layer_editor_is_editing(&self) -> bool {
        match self.page {
            Page::PluginEnvelope => self.envelope.is_editing(),
            Page::PluginPitchEnvelope => self.pitch_envelope.is_editing(),
            Page::PluginLfo => self.lfo.is_editing(),
            Page::PluginTuning => self.tuning.is_editing(),
            Page::PluginRange => self.range.is_editing(),
            Page::PluginLayerLevel => self.layer_level.is_editing(),
            _ => false,
        }
    }

    fn restore_program_preview(&mut self) {
        if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
            self.pending_command = Some(MenuCommand::RestoreProgramDraftPreview { draft_id });
        }
    }

    fn is_layer_parameter_page(&self) -> bool {
        matches!(
            self.page,
            Page::PluginEnvelope
                | Page::PluginPitchEnvelope
                | Page::PluginLfo
                | Page::PluginTuning
                | Page::PluginRange
                | Page::PluginLayerLevel
        )
    }

    fn apply_program_output_input(&mut self, input: Input) {
        match self.program_output.handle(input) {
            ComponentEvent::Changed(_) if self.program_output.is_editing() => {
                self.emit_program_output(true);
            }
            ComponentEvent::EditCommitted(_) => {
                self.emit_program_output(false);
            }
            ComponentEvent::EditCancelled(_) => self.restore_program_preview(),
            ComponentEvent::ExitRequested(_) => self.page = Page::PluginProgramSections,
            _ => {}
        }
    }

    fn emit_program_output(&mut self, preview: bool) {
        let Some(value) = self.program_output.selected_item().value().as_f64() else {
            return;
        };
        if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
            self.pending_command = Some(MenuCommand::EditProgramDraftField {
                draft_id,
                field_id: "program.gain".into(),
                value: ProgramEditorValue::Integer((value * 100.0).round() as i64),
                preview,
            });
        }
    }

    fn apply_program_name_input(&mut self, input: Input) {
        match self.program_name.handle(input) {
            ComponentEvent::EditCommitted(_) => {
                if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
                    self.pending_command = Some(MenuCommand::SetProgramDraftName {
                        draft_id,
                        name: self.program_name.value().to_owned(),
                    });
                }
            }
            ComponentEvent::ExitRequested(_) => {
                self.page = Page::ProgramEditorRoot;
            }
            _ => {}
        }
    }

    fn open_unsaved_changes(&mut self, destination: ProgramExitDestination) {
        let return_page = if self.page == Page::PluginUnsavedChanges {
            self.pending_program_exit
                .as_ref()
                .map_or(Page::PluginProgramSections, |pending| pending.return_page)
        } else {
            self.page
        };
        self.unsaved_changes.set_selected(0);
        self.pending_program_exit = Some(PendingProgramExit {
            return_page,
            destination,
        });
        self.page = Page::PluginUnsavedChanges;
    }

    fn apply_unsaved_changes_input(&mut self, input: Input) {
        match self.unsaved_changes.handle(input) {
            ComponentEvent::Activated(_) => {
                let Some(pending) = self.pending_program_exit.as_ref() else {
                    self.page = Page::PluginProgramSections;
                    return;
                };
                let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) else {
                    self.pending_program_exit = None;
                    self.page = Page::PluginCustomPrograms;
                    return;
                };
                self.pending_command = Some(MenuCommand::ResolveProgramExit {
                    draft_id,
                    decision: if self.unsaved_changes.selected() == 0 {
                        ProgramExitDecision::Save
                    } else {
                        ProgramExitDecision::Discard
                    },
                    destination: pending.destination.clone(),
                });
            }
            ComponentEvent::ExitRequested(_) => {
                self.page = self
                    .pending_program_exit
                    .take()
                    .map_or(Page::PluginProgramSections, |pending| pending.return_page);
            }
            _ => {}
        }
    }

    fn render_generic_editor_root(&self) -> Screen {
        let Some(draft) = self.program_draft.as_ref() else {
            return Screen::with_header("PROGRAM", "NO DRAFT", " ");
        };
        let mut names = vec!["NAME".to_owned()];
        let mut details = vec!["Program name".to_owned()];
        names.extend(draft.editor.pages.iter().map(|page| page.label.clone()));
        details.extend(draft.editor.pages.iter().map(|page| page.detail.clone()));
        names.push("SAVE".into());
        details.push("Store program".into());
        let selected = self.current_editor_selection().min(names.len() - 1);
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let detail_refs = details.iter().map(String::as_str).collect::<Vec<_>>();
        simple_screen(
            indexed_title(&draft.editor.title, selected, names.len()),
            &name_refs,
            &detail_refs,
            selected,
        )
    }

    fn render_generic_editor_page(&self) -> Screen {
        let Some(page) = self.current_editor_page() else {
            return Screen::with_header("PROGRAM", "INVALID PAGE", " ");
        };
        let mut names = page
            .fields
            .iter()
            .map(|field| field.label.clone())
            .collect::<Vec<_>>();
        let mut details = page
            .fields
            .iter()
            .map(editor_field_summary)
            .collect::<Vec<_>>();
        if page.enabled {
            names.extend(page.pages.iter().map(|page| page.label.clone()));
            details.extend(page.pages.iter().map(|page| page.detail.clone()));
        }
        if names.is_empty() {
            return Screen::with_header(&page.label, "NO OPTIONS", " ");
        }
        let selected = self.current_editor_selection().min(names.len() - 1);
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let detail_refs = details.iter().map(String::as_str).collect::<Vec<_>>();
        simple_screen(
            indexed_title(&page.label, selected, names.len()),
            &name_refs,
            &detail_refs,
            selected,
        )
    }

    fn render_generic_editor_field(&self) -> Screen {
        let Some(field) = self.active_editor_field() else {
            return Screen::with_header("PROGRAM", "INVALID FIELD", " ");
        };
        let Some(editor) = self.editor_field.as_ref() else {
            return Screen::with_header(&field.label, "NO EDITOR", " ");
        };
        let [_redundant_label, value] = component_lines(editor, true);
        Screen::with_header(&field.label, value, " ")
    }

    fn render_generic_editor_sound(&self) -> Screen {
        let sounds = self.editor_sound_choices();
        let Some(field) = self.active_editor_field() else {
            return Screen::with_header("TIMBRE", "INVALID FIELD", " ");
        };
        if sounds.is_empty() {
            return Screen::with_header(&field.label, "NO SOUNDS", " ");
        }
        let selected_id = match &field.value {
            ProgramEditorValue::SoundId(id) => Some(id.as_str()),
            _ => None,
        };
        let mut carousel = SimpleCarousel::new(
            "program-editor-sound",
            sounds.iter().map(|sound| {
                let name = if selected_id == Some(sound.id.as_str()) {
                    format!(
                        "[{}]",
                        sound
                            .name
                            .chars()
                            .take(DISPLAY_COLUMNS - 2)
                            .collect::<String>()
                    )
                } else {
                    sound.name.clone()
                };
                CarouselItem::new(name, &sound.detail)
            }),
        );
        carousel.set_selected(self.plugin_timbre_index.min(sounds.len() - 1));
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        Screen::with_header(
            indexed_title(
                &field.label,
                self.plugin_timbre_index.min(sounds.len() - 1),
                sounds.len(),
            ),
            line_1,
            line_2,
        )
    }

    fn render_plugin_play(&self) -> Screen {
        let sounds = self.filtered_sounds();
        if sounds.is_empty() {
            return self.plugin_screen("PROGRAMS", "NO PROGRAMS", " ");
        }
        let mut carousel = SimpleCarousel::new(
            "plugin-sounds",
            sounds.iter().map(|sound| {
                let name = if self.plugin_play_selected_sound_id() == Some(sound.id.as_str()) {
                    let bounded = sound
                        .name
                        .chars()
                        .take(DISPLAY_COLUMNS - 2)
                        .collect::<String>();
                    format!("[{bounded}]")
                } else {
                    sound.name.clone()
                };
                CarouselItem::new(name, &sound.detail)
            }),
        );
        carousel.set_selected(self.plugin_play_index);
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        self.plugin_screen(
            compact_indexed_context("PROG", self.plugin_play_index, sounds.len()),
            line_1,
            line_2,
        )
    }

    fn render_plugin_loading(&self) -> Screen {
        let [line_1, line_2] = component_lines(&self.plugin_spinner, false);
        let plugin = self.pending_plugin_instance_id.as_deref().and_then(|id| {
            self.play_plugins
                .iter()
                .find(|plugin| plugin.instance_id == id)
        });
        Screen::with_plugin_header(
            plugin.map_or("PLUGIN", |plugin| plugin.full_name.as_str()),
            plugin.map_or("PLUGIN", |plugin| plugin.short_name.as_str()),
            "LOADING",
            line_1,
            line_2,
        )
    }

    fn render_plugin_library(&self) -> Screen {
        let (mut names, mut details): (Vec<&str>, Vec<String>) =
            if self.plugin_play_context == PluginPlayContext::RackSlot {
                (
                    vec!["PROGRAMS"],
                    vec![format!("{} AVAILABLE", self.plugin_sounds.len())],
                )
            } else {
                (
                    vec!["PRESETS", "PROGRAMS"],
                    vec![
                        format!("{} SAVED", self.plugin_presets.len()),
                        format!("{} AVAILABLE", self.plugin_sounds.len()),
                    ],
                )
            };
        if self.plugin_play_context == PluginPlayContext::Standalone
            && !self.plugin_parameter_pages().is_empty()
        {
            names.push("CONTROLS");
            details.push(format!(
                "{} PARAMETERS",
                self.plugin_parameter_schema
                    .as_ref()
                    .map_or(0, |schema| schema.parameters.len())
            ));
        }
        let detail_refs = details.iter().map(String::as_str).collect::<Vec<_>>();
        self.plugin_simple_screen(
            compact_indexed_context("PLAY", self.plugin_library_index, names.len()),
            &names,
            &detail_refs,
            self.plugin_library_index,
        )
    }

    fn render_plugin_presets(&self) -> Screen {
        let mut carousel = SimpleCarousel::new(
            "plugin-presets",
            std::iter::once(CarouselItem::new("SAVE CURRENT", "CAPTURE PLUGIN STATE")).chain(
                self.plugin_presets.iter().map(|preset| {
                    let name =
                        if self.plugin_active_preset_id.as_deref() == Some(preset.id.as_str()) {
                            let bounded = preset
                                .name
                                .chars()
                                .take(DISPLAY_COLUMNS - 2)
                                .collect::<String>();
                            format!("[{bounded}]")
                        } else {
                            preset.name.clone()
                        };
                    CarouselItem::new(name, &preset.detail)
                }),
            ),
        );
        carousel.set_selected(self.plugin_preset_index);
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        self.plugin_screen(
            compact_indexed_context(
                "PRESET",
                self.plugin_preset_index,
                self.plugin_presets.len() + 1,
            ),
            line_1,
            line_2,
        )
    }

    fn render_plugin_programs(&self) -> Screen {
        let banks = self.plugin_banks();
        if banks.is_empty() {
            return self.plugin_screen("PROGRAMS", "NO PROGRAMS", " ");
        }
        let names = banks
            .iter()
            .map(|bank| normalized_display_text(&bank.to_ascii_uppercase(), "PROGRAMS"))
            .collect::<Vec<_>>();
        let counts = banks
            .iter()
            .map(|bank| {
                format!(
                    "{} PROGRAMS",
                    self.plugin_sounds
                        .iter()
                        .filter(|sound| sound.bank == **bank)
                        .count()
                )
            })
            .collect::<Vec<_>>();
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let details = counts.iter().map(String::as_str).collect::<Vec<_>>();
        self.plugin_simple_screen(
            compact_indexed_context("BANK", self.plugin_program_bank_index, banks.len()),
            &name_refs,
            &details,
            self.plugin_program_bank_index,
        )
    }

    fn render_plugin_parameter_pages(&self) -> Screen {
        let pages = self.plugin_parameter_pages();
        if pages.is_empty() {
            return self.plugin_screen("CONTROLS", "NO CONTROLS", " ");
        }
        let names = pages
            .iter()
            .map(|page| normalized_display_text(&page.name, "CONTROLS").to_ascii_uppercase())
            .collect::<Vec<_>>();
        let details = pages
            .iter()
            .map(|page| {
                let count = self.plugin_parameter_schema.as_ref().map_or(0, |schema| {
                    schema
                        .parameters
                        .iter()
                        .filter(|parameter| parameter.page == page.id)
                        .count()
                });
                format!("{count} CONTROLS")
            })
            .collect::<Vec<_>>();
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let detail_refs = details.iter().map(String::as_str).collect::<Vec<_>>();
        self.plugin_simple_screen(
            compact_indexed_context("CTRL", self.plugin_parameter_page_index, pages.len()),
            &name_refs,
            &detail_refs,
            self.plugin_parameter_page_index,
        )
    }

    fn render_plugin_parameters(&self) -> Screen {
        let parameters = self.current_plugin_parameters();
        let Some(page) = self
            .plugin_parameter_pages()
            .get(self.plugin_parameter_page_index)
            .copied()
        else {
            return self.plugin_screen("CONTROLS", "NO CONTROLS", " ");
        };
        if parameters.is_empty() {
            return self.plugin_screen(&page.name, "NO CONTROLS", " ");
        }
        let mut carousel = SimpleCarousel::new(
            "plugin-live-parameters",
            parameters.iter().map(|parameter| {
                CarouselItem::new(
                    normalized_display_text(&parameter.name, "PARAMETER").to_ascii_uppercase(),
                    little_parameter_display(
                        parameter,
                        self.plugin_parameter_value(parameter),
                        self.plugin_parameter_schema
                            .as_ref()
                            .and_then(|schema| schema.display_decimals),
                    ),
                )
            }),
        );
        carousel.set_selected(self.plugin_parameter_index.min(parameters.len() - 1));
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        self.plugin_screen(
            compact_indexed_context(&page.name, self.plugin_parameter_index, parameters.len()),
            line_1,
            line_2,
        )
    }

    fn render_plugin_parameter_value(&self) -> Screen {
        let Some(editor) = &self.plugin_parameter_editor else {
            return self.plugin_screen("CONTROLS", "INVALID CONTROL", " ");
        };
        let [line_1, line_2] = component_lines(editor, false);
        self.plugin_screen("CONTROL", line_1, line_2)
    }

    fn render_custom_programs(&self) -> Screen {
        let custom = self.editable_sounds();
        let mut names = vec!["ADD NEW".to_owned()];
        let mut details = vec!["Create program".to_owned()];
        for program in custom {
            names.push(program.name.clone());
            details.push(program.detail.clone());
        }
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let detail_refs = details.iter().map(String::as_str).collect::<Vec<_>>();
        self.plugin_simple_screen(
            compact_indexed_context("CUSTOM", self.plugin_custom_index, names.len()),
            &name_refs,
            &detail_refs,
            self.plugin_custom_index,
        )
    }

    fn render_layer_menu(&self) -> Screen {
        if self.plugin_layer_index == 0 {
            return self.plugin_simple_screen(
                self.layer_header(""),
                &PLUGIN_LAYER_SECTIONS,
                &PLUGIN_LAYER_DETAILS,
                self.plugin_layer_option_index,
            );
        }
        if self.program_layer_enabled(1) {
            let mut sections = vec!["ENABLED"];
            sections.extend(PLUGIN_LAYER_SECTIONS);
            let mut details = vec!["ON"];
            details.extend(PLUGIN_LAYER_DETAILS);
            self.plugin_simple_screen(
                self.layer_header(""),
                &sections,
                &details,
                self.plugin_layer_option_index,
            )
        } else {
            self.plugin_simple_screen(
                self.layer_header(""),
                &["ENABLED"],
                &["OFF"],
                self.plugin_layer_option_index,
            )
        }
    }

    fn render_timbre(&self) -> Screen {
        let sounds = self.base_sounds();
        if sounds.is_empty() {
            return self.plugin_screen(self.layer_header("TIMBRE"), "NO SOUNDS", " ");
        }
        let selected_id = self.program_layer_source_id(self.plugin_layer_index);
        let mut carousel = SimpleCarousel::new(
            "plugin-timbre",
            sounds.iter().map(|sound| {
                let name = if selected_id.as_deref() == Some(sound.id.as_str()) {
                    let bounded = sound
                        .name
                        .chars()
                        .take(DISPLAY_COLUMNS - 2)
                        .collect::<String>();
                    format!("[{bounded}]")
                } else {
                    sound.name.clone()
                };
                CarouselItem::new(name, &sound.detail)
            }),
        );
        carousel.set_selected(self.plugin_timbre_index);
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        self.plugin_screen(
            compact_indexed_context("TIMBRE", self.plugin_timbre_index, sounds.len()),
            line_1,
            line_2,
        )
    }

    fn filtered_sounds(&self) -> Vec<&PlaySound> {
        let banks = self.plugin_banks();
        let Some(bank) = banks.get(self.plugin_program_bank_index) else {
            return Vec::new();
        };
        self.plugin_sounds
            .iter()
            .filter(|sound| sound.bank == **bank)
            .collect()
    }

    fn base_sounds(&self) -> Vec<&PlaySound> {
        self.plugin_sounds
            .iter()
            .filter(|sound| !sound.editable)
            .collect()
    }

    fn editable_sounds(&self) -> Vec<&PlaySound> {
        self.plugin_sounds
            .iter()
            .filter(|sound| sound.editable)
            .collect()
    }

    fn plugin_banks(&self) -> Vec<&str> {
        let mut banks = Vec::new();
        for sound in &self.plugin_sounds {
            if !banks.contains(&sound.bank.as_str()) {
                banks.push(sound.bank.as_str());
            }
        }
        banks
    }

    fn plugin_bank_index(&self, bank: &str) -> Option<usize> {
        self.plugin_banks()
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(bank))
    }

    fn plugin_parameter_pages(&self) -> Vec<&rackforge_plugin_api::PageDescriptor> {
        let Some(schema) = &self.plugin_parameter_schema else {
            return Vec::new();
        };
        let mut pages = schema
            .pages
            .iter()
            .filter(|page| {
                schema
                    .parameters
                    .iter()
                    .any(|parameter| parameter.page == page.id)
            })
            .collect::<Vec<_>>();
        pages.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.id.cmp(&right.id))
        });
        pages
    }

    fn current_plugin_parameters(&self) -> Vec<&ParameterDescriptor> {
        let Some(page) = self
            .plugin_parameter_pages()
            .get(self.plugin_parameter_page_index)
            .copied()
        else {
            return Vec::new();
        };
        let Some(schema) = &self.plugin_parameter_schema else {
            return Vec::new();
        };
        let mut parameters = schema
            .parameters
            .iter()
            .filter(|parameter| parameter.page == page.id)
            .collect::<Vec<_>>();
        parameters.sort_by(|left, right| {
            left.order
                .cmp(&right.order)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.index.cmp(&right.index))
        });
        parameters
    }

    fn plugin_parameter_value(&self, parameter: &ParameterDescriptor) -> f64 {
        self.plugin_parameter_values
            .get(&parameter.index)
            .copied()
            .unwrap_or_else(|| match &parameter.kind {
                ParameterKind::Float { default, .. } => *default,
                ParameterKind::Integer { default, .. } => *default as f64,
                ParameterKind::Boolean { default } => f64::from(u8::from(*default)),
                ParameterKind::Enum { default, .. } => f64::from(*default),
                ParameterKind::Trigger => 0.0,
                ParameterKind::Meter { minimum, .. } => *minimum,
            })
    }

    fn open_plugin_parameter(&mut self) {
        let Some(parameter) = self
            .current_plugin_parameters()
            .get(self.plugin_parameter_index)
            .cloned()
            .cloned()
        else {
            return;
        };
        if parameter.flags.read_only || matches!(parameter.kind, ParameterKind::Meter { .. }) {
            return;
        }
        if matches!(parameter.kind, ParameterKind::Trigger) {
            if let Some(instance_id) = self.active_plugin_instance_id.clone() {
                self.pending_command = Some(MenuCommand::TriggerPluginParameter {
                    instance_id,
                    parameter_index: parameter.index,
                });
            }
            return;
        }
        let Some(value) = little_parameter_editable_value(
            &parameter,
            self.plugin_parameter_value(&parameter),
            self.plugin_parameter_schema
                .as_ref()
                .and_then(|schema| schema.display_decimals),
        ) else {
            return;
        };
        let mut editor = ValueCarousel::new(
            "plugin-live-parameter",
            [ValueItem::new(
                parameter.index.to_string(),
                normalized_display_text(&parameter.name, "PARAMETER"),
                value,
            )],
        );
        editor.set_focused(true);
        let _ = editor.handle(Input::Button1);
        self.plugin_parameter_editor = Some(editor);
        self.page = Page::PluginParameterValue;
    }

    fn apply_plugin_parameter_input(&mut self, input: Input) {
        let Some(parameter) = self
            .current_plugin_parameters()
            .get(self.plugin_parameter_index)
            .cloned()
            .cloned()
        else {
            self.page = Page::PluginParameters;
            self.plugin_parameter_editor = None;
            return;
        };
        let event = self
            .plugin_parameter_editor
            .as_mut()
            .map_or(ComponentEvent::Ignored, |editor| editor.handle(input));
        match event {
            ComponentEvent::Changed(_) => self.emit_plugin_parameter_value(&parameter),
            ComponentEvent::EditCommitted(_) => {
                self.emit_plugin_parameter_value(&parameter);
                self.plugin_parameter_editor = None;
                self.page = Page::PluginParameters;
            }
            ComponentEvent::EditCancelled(_) => {
                self.emit_plugin_parameter_value(&parameter);
                self.plugin_parameter_editor = None;
                self.page = Page::PluginParameters;
            }
            ComponentEvent::ExitRequested(_) => {
                self.plugin_parameter_editor = None;
                self.page = Page::PluginParameters;
            }
            _ => {}
        }
    }

    fn emit_plugin_parameter_value(&mut self, parameter: &ParameterDescriptor) {
        let Some(editor) = &self.plugin_parameter_editor else {
            return;
        };
        let item = editor.selected_item();
        let value = match &parameter.kind {
            ParameterKind::Enum { choices, .. } => item
                .value()
                .choice_index()
                .and_then(|index| choices.get(index))
                .map(|choice| f64::from(choice.value)),
            ParameterKind::Float { step, .. } => item.value().as_f64().map(|value| {
                let scale = 10_f64.powi(parameter_decimals(*step) as i32);
                (value * scale).round() / scale
            }),
            _ => item.value().as_f64(),
        };
        let (Some(instance_id), Some(value)) = (self.active_plugin_instance_id.clone(), value)
        else {
            return;
        };
        self.plugin_parameter_values.insert(parameter.index, value);
        self.pending_command = Some(MenuCommand::SetPluginParameter {
            instance_id,
            parameter_index: parameter.index,
            value,
        });
    }

    fn sync_layer_editors(&mut self) {
        let layer = self.program_layer_value(self.plugin_layer_index);
        let layer = layer.as_ref();
        if !self.envelope.is_editing() {
            self.envelope = envelope_carousel(layer);
        }
        if !self.pitch_envelope.is_editing() {
            self.pitch_envelope = pitch_envelope_carousel(layer);
        }
        if !self.lfo.is_editing() {
            self.lfo = lfo_carousel(layer);
        }
        if !self.tuning.is_editing() {
            self.tuning = tuning_carousel(layer);
        }
        if !self.range.is_editing() {
            self.range = range_carousel(layer);
        }
        if !self.layer_level.is_editing() {
            self.layer_level = layer_level_carousel(layer);
        }
    }

    fn sync_program_output(&mut self) {
        if self.program_output.is_editing() {
            return;
        }
        let gain = self
            .program_draft
            .as_ref()
            .and_then(|draft| serde_json::from_str::<serde_json::Value>(&draft.document_json).ok())
            .and_then(|document| document.pointer("/payload/gain")?.as_f64())
            .unwrap_or(1.0);
        self.program_output = program_output_carousel(gain);
    }

    fn program_layer_value(&self, layer_index: usize) -> Option<serde_json::Value> {
        let draft = self.program_draft.as_ref()?;
        serde_json::from_str::<serde_json::Value>(&draft.document_json)
            .ok()?
            .pointer(&format!("/payload/layers/{layer_index}"))
            .cloned()
    }

    fn program_layer_count(&self) -> usize {
        self.program_draft
            .as_ref()
            .and_then(|draft| serde_json::from_str::<serde_json::Value>(&draft.document_json).ok())
            .and_then(|document| {
                document
                    .pointer("/payload/layers")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len)
                    .or_else(|| document.pointer("/payload/source").map(|_| 1))
            })
            .unwrap_or(0)
    }

    fn program_layer_enabled(&self, layer_index: usize) -> bool {
        self.program_layer_value(layer_index)
            .and_then(|layer| layer.get("enabled").and_then(serde_json::Value::as_bool))
            .unwrap_or(false)
    }

    fn layer_menu_len(&self) -> usize {
        if self.plugin_layer_index == 0 {
            PLUGIN_LAYER_SECTIONS.len()
        } else if !self.program_layer_enabled(1) {
            1
        } else {
            PLUGIN_LAYER_SECTIONS.len() + 1
        }
    }

    fn program_layer_source_id(&self, layer_index: usize) -> Option<String> {
        let draft = self.program_draft.as_ref()?;
        let document = serde_json::from_str::<serde_json::Value>(&draft.document_json).ok()?;
        let source = document
            .pointer(&format!("/payload/layers/{layer_index}/source"))
            .or_else(|| {
                (layer_index == 0)
                    .then(|| document.pointer("/payload/source"))
                    .flatten()
            })?;
        let bank = source.get("bank")?.as_u64()?;
        let program = source.get("program")?.as_u64()?;
        Some(format!("dls.b{bank:08x}.p{program:08x}"))
    }

    fn layer_header(&self, section: &str) -> String {
        let layer = if self.plugin_layer_index == 0 {
            "A"
        } else {
            "B"
        };
        format!("{layer} {section}").trim_end().to_owned()
    }

    fn is_program_edit_page(&self) -> bool {
        matches!(
            self.page,
            Page::PluginProgramSections
                | Page::PluginName
                | Page::PluginLayerMenu
                | Page::PluginTimbre
                | Page::PluginEnvelope
                | Page::PluginRange
                | Page::PluginLfo
                | Page::PluginTuning
                | Page::PluginPitchEnvelope
                | Page::PluginLayerLevel
                | Page::PluginSharedFx
                | Page::PluginProgramOutput
                | Page::PluginUnsavedChanges
                | Page::ProgramEditorRoot
                | Page::ProgramEditorSection
                | Page::ProgramEditorField
                | Page::ProgramEditorSound
        )
    }
}

fn editor_layer_field_id(layer_index: usize, legacy_parameter: &str) -> String {
    let layer = if layer_index == 0 { "a" } else { "b" };
    let parameter = match legacy_parameter {
        "amplitude_envelope.attack_seconds" => "amp.attack",
        "amplitude_envelope.decay_seconds" => "amp.decay",
        "amplitude_envelope.sustain_level" => "amp.sustain",
        "amplitude_envelope.release_seconds" => "amp.release",
        "pitch_envelope.attack_seconds" => "pitch.attack",
        "pitch_envelope.decay_seconds" => "pitch.decay",
        "pitch_envelope.sustain_level" => "pitch.sustain",
        "pitch_envelope.release_seconds" => "pitch.release",
        "pitch_envelope.depth_cents" => "pitch.depth",
        "lfo.enabled" => "lfo.enabled",
        "lfo.frequency_hz" => "lfo.frequency",
        "lfo.delay_seconds" => "lfo.delay",
        "lfo.pitch_depth_cents" => "lfo.pitch",
        "lfo.mod_wheel_pitch_depth_cents" => "lfo.mod-pitch",
        "lfo.attenuation_depth_centibels" => "lfo.amp",
        "lfo.mod_wheel_attenuation_depth_centibels" => "lfo.mod-amp",
        "transpose_semitones" => "transpose",
        "fine_tune_cents" => "fine",
        "pitch_bend_range_semitones" => "bend",
        "modulation_depth" => "mod-depth",
        "key_range.low" => "key-low",
        "key_range.high" => "key-high",
        "velocity_range.low" => "vel-low",
        "velocity_range.high" => "vel-high",
        "gain" => "gain",
        other => other,
    };
    format!("layer.{layer}.{parameter}")
}

fn editor_scaled_value(parameter: &str, value: f64) -> i64 {
    let scale = match parameter {
        "amplitude_envelope.attack_seconds"
        | "amplitude_envelope.decay_seconds"
        | "amplitude_envelope.sustain_level"
        | "amplitude_envelope.release_seconds"
        | "pitch_envelope.attack_seconds"
        | "pitch_envelope.decay_seconds"
        | "pitch_envelope.sustain_level"
        | "pitch_envelope.release_seconds"
        | "lfo.frequency_hz"
        | "lfo.delay_seconds"
        | "gain"
        | "modulation_depth" => 100.0,
        "pitch_bend_range_semitones" => 10.0,
        _ => 1.0,
    };
    (value * scale).round() as i64
}

fn little_parameter_editable_value(
    parameter: &ParameterDescriptor,
    value: f64,
    display_decimals: Option<u8>,
) -> Option<EditableValue> {
    match &parameter.kind {
        ParameterKind::Float {
            minimum,
            maximum,
            step,
            unit,
            ..
        } => Some(EditableValue::number(
            value.clamp(*minimum, *maximum),
            *minimum,
            *maximum,
            *step,
            parameter_display_decimals(*step, display_decimals),
            unit.as_deref().unwrap_or(""),
        )),
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            unit,
            ..
        } => Some(EditableValue::integer(
            (value.round() as i64).clamp(*minimum, *maximum),
            *minimum,
            *maximum,
            *step,
            unit.as_deref().unwrap_or(""),
        )),
        ParameterKind::Boolean { .. } => Some(EditableValue::toggle(value >= 0.5, "OFF", "ON")),
        ParameterKind::Enum { choices, .. } => {
            let selected = choices
                .iter()
                .position(|choice| f64::from(choice.value) == value)
                .unwrap_or(0);
            Some(EditableValue::choice(
                selected,
                choices.iter().map(|choice| choice.name.clone()),
            ))
        }
        ParameterKind::Trigger | ParameterKind::Meter { .. } => None,
    }
}

fn little_parameter_display(
    parameter: &ParameterDescriptor,
    value: f64,
    display_decimals: Option<u8>,
) -> String {
    match &parameter.kind {
        ParameterKind::Float { step, unit, .. } => {
            let value = format!(
                "{value:.precision$}",
                precision = parameter_display_decimals(*step, display_decimals)
            );
            unit.as_deref()
                .filter(|unit| !unit.is_empty())
                .map_or(value.clone(), |unit| format!("{value} {unit}"))
        }
        ParameterKind::Integer { unit, .. } => {
            let value = (value.round() as i64).to_string();
            unit.as_deref()
                .filter(|unit| !unit.is_empty())
                .map_or(value.clone(), |unit| format!("{value} {unit}"))
        }
        ParameterKind::Boolean { .. } => if value >= 0.5 { "ON" } else { "OFF" }.into(),
        ParameterKind::Enum { choices, .. } => choices
            .iter()
            .find(|choice| f64::from(choice.value) == value)
            .map_or_else(|| format!("{value:.0}"), |choice| choice.name.clone()),
        ParameterKind::Trigger => "PRESS OK".into(),
        ParameterKind::Meter { unit, .. } => {
            let precision = display_decimals.map_or(2, usize::from);
            unit.as_deref().map_or_else(
                || format!("{value:.precision$}"),
                |unit| format!("{value:.precision$} {unit}"),
            )
        }
    }
}

fn parameter_display_decimals(step: f64, display_decimals: Option<u8>) -> usize {
    display_decimals.map_or_else(|| parameter_decimals(step), usize::from)
}

fn parameter_decimals(step: f64) -> usize {
    if step >= 1.0 {
        0
    } else {
        let mut scaled = step.abs();
        let mut decimals = 0;
        while decimals < 6 && (scaled.round() - scaled).abs() > 1.0e-9 {
            scaled *= 10.0;
            decimals += 1;
        }
        decimals
    }
}

fn find_editor_field<'a>(
    page: &'a ProgramEditorPage,
    field_id: &str,
) -> Option<&'a ProgramEditorField> {
    page.fields
        .iter()
        .find(|field| field.id == field_id)
        .or_else(|| {
            page.pages
                .iter()
                .find_map(|page| find_editor_field(page, field_id))
        })
}

fn editor_field_carousel(field: &ProgramEditorField) -> Option<ValueCarousel> {
    let value = match (&field.kind, &field.value) {
        (
            ProgramEditorFieldKind::Number {
                minimum,
                maximum,
                step,
                decimals,
                unit,
                allow_inherited,
            },
            value,
        ) => {
            let scale = 10_f64.powi(i32::from(*decimals));
            let unit = unit.as_deref().unwrap_or("");
            if *allow_inherited {
                let current = match value {
                    ProgramEditorValue::Inherited => None,
                    ProgramEditorValue::Integer(value) => Some(*value as f64 / scale),
                    _ => return None,
                };
                EditableValue::optional_number(
                    current,
                    (*step as f64 / scale).clamp(*minimum as f64 / scale, *maximum as f64 / scale),
                    *minimum as f64 / scale,
                    *maximum as f64 / scale,
                    *step as f64 / scale,
                    usize::from(*decimals),
                    unit,
                )
            } else {
                let ProgramEditorValue::Integer(current) = value else {
                    return None;
                };
                if *decimals == 0 {
                    EditableValue::integer(*current, *minimum, *maximum, *step, unit)
                } else {
                    EditableValue::number(
                        *current as f64 / scale,
                        *minimum as f64 / scale,
                        *maximum as f64 / scale,
                        *step as f64 / scale,
                        usize::from(*decimals),
                        unit,
                    )
                }
            }
        }
        (ProgramEditorFieldKind::Choice { options }, ProgramEditorValue::Choice(current)) => {
            let selected = options
                .iter()
                .position(|option| option.value == *current)
                .unwrap_or(0);
            EditableValue::choice(selected, options.iter().map(|option| option.label.clone()))
        }
        _ => return None,
    };
    Some(focused_values(
        &format!("program-editor-{}", field.id),
        [ValueItem::new(&field.id, &field.label, value)],
    ))
}

fn editor_value_from_item(
    field: &ProgramEditorField,
    item: &ValueItem,
) -> Option<ProgramEditorValue> {
    match &field.kind {
        ProgramEditorFieldKind::Number {
            decimals,
            allow_inherited,
            ..
        } => {
            let scale = 10_f64.powi(i32::from(*decimals));
            if *allow_inherited {
                item.value().as_optional_f64().map(|value| {
                    value.map_or(ProgramEditorValue::Inherited, |value| {
                        ProgramEditorValue::Integer((value * scale).round() as i64)
                    })
                })
            } else if *decimals == 0 {
                item.value().as_i64().map(ProgramEditorValue::Integer)
            } else {
                item.value()
                    .as_f64()
                    .map(|value| ProgramEditorValue::Integer((value * scale).round() as i64))
            }
        }
        ProgramEditorFieldKind::Choice { options } => item
            .value()
            .choice_index()
            .and_then(|index| options.get(index))
            .map(|option| ProgramEditorValue::Choice(option.value.clone())),
        _ => None,
    }
}

fn editor_field_summary(field: &ProgramEditorField) -> String {
    match (&field.kind, &field.value) {
        (ProgramEditorFieldKind::Toggle, ProgramEditorValue::Boolean(value)) => {
            if *value {
                "ON".into()
            } else {
                "OFF".into()
            }
        }
        (ProgramEditorFieldKind::Choice { options }, ProgramEditorValue::Choice(value)) => options
            .iter()
            .find(|option| option.value == *value)
            .map_or_else(|| field.detail.clone(), |option| option.label.clone()),
        (ProgramEditorFieldKind::Sound { .. }, _) => field.detail.clone(),
        (ProgramEditorFieldKind::Number { .. }, ProgramEditorValue::Inherited) => "INHERIT".into(),
        (
            ProgramEditorFieldKind::Number { decimals, unit, .. },
            ProgramEditorValue::Integer(value),
        ) => {
            let scale = 10_f64.powi(i32::from(*decimals));
            format!(
                "{:.*} {}",
                usize::from(*decimals),
                *value as f64 / scale,
                unit.as_deref().unwrap_or("")
            )
            .trim_end()
            .to_owned()
        }
        _ => field.detail.clone(),
    }
}

fn normalized_display_text(value: &str, fallback: &str) -> String {
    clean_surface_text(value, fallback)
        .chars()
        .take(DISPLAY_COLUMNS)
        .collect()
}

fn clean_surface_text(value: &str, fallback: &str) -> String {
    let mut normalized = value
        .chars()
        .filter(|character| !character.is_control())
        .map(|character| if character.is_ascii() { character } else { '?' })
        .collect::<String>();
    if normalized.trim().is_empty() {
        normalized = fallback.into();
    }
    normalized
}

fn compact_plugin_name(value: &str) -> String {
    let normalized = clean_surface_text(value, "PLUGIN").to_ascii_uppercase();
    let compact = normalized
        .chars()
        .filter(|character| !character.is_whitespace())
        .take(8)
        .collect::<String>();
    if compact.is_empty() {
        "PLUGIN".into()
    } else {
        compact
    }
}

fn fit_surface_text(value: &str, columns: usize) -> String {
    clean_surface_text(value, " ")
        .chars()
        .take(columns)
        .collect()
}

fn responsive_plugin_header(name: &str, short_name: &str, context: &str, columns: usize) -> String {
    if columns == 0 {
        return String::new();
    }
    let full_name = clean_surface_text(name, "PLUGIN").to_ascii_uppercase();
    let short_name = compact_plugin_name(short_name);
    let context = clean_surface_text(context, " ").to_ascii_uppercase();
    if context.trim().is_empty() {
        // Some character displays center short header payloads. Always fill
        // the complete viewport so the plugin identity remains anchored to
        // the first column even when there is no page-local context.
        let identity = fit_surface_text(&full_name, columns);
        return format!("{identity:<columns$}");
    }
    if columns == 1 {
        return full_name.chars().take(1).collect();
    }

    // Prefer the complete identity whenever both semantic values fit. When
    // they do not, split the available space responsively. At the KeyLab's
    // 18 columns this naturally becomes 8 + separator + 9; narrower and wider
    // controllers get a proportional allocation without another layout ID.
    if full_name.chars().count() + 1 + context.chars().count() <= columns {
        let gap = columns - full_name.chars().count() - context.chars().count();
        return format!("{full_name}{}{context}", " ".repeat(gap.max(1)));
    }
    let name_columns = ((columns - 1) / 2).max(1);
    let context_columns = columns - name_columns - 1;
    let identity = if full_name.chars().count() <= name_columns {
        full_name
    } else {
        short_name
    };
    let identity = identity.chars().take(name_columns).collect::<String>();
    let context = context.chars().take(context_columns).collect::<String>();
    format!("{identity:<width$} {context}", width = name_columns)
}

fn compact_indexed_context(label: &str, index: usize, len: usize) -> String {
    let counter = format!("{}/{}", index.saturating_add(1), len.max(1));
    format!("{label} {counter}")
}

fn initialize_keyboard_parts(rack: &mut RackDefinition) -> bool {
    if rack.keyboard_parts.is_some() {
        return false;
    }
    let mut parts = RackKeyboardParts::default();
    if let [left, right, ..] = rack.slots.as_mut_slice()
        && left.midi_note_low == 0
        && right.midi_note_high == 127
        && left.midi_note_high < 127
        && left.midi_note_high.saturating_add(1) == right.midi_note_low
    {
        parts.split_key = Some(right.midi_note_low);
        left.midi_note_high = 127;
        right.midi_note_low = 0;
    }
    rack.keyboard_parts = Some(parts);
    true
}

fn midi_note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i16::from(note / 12) - 1;
    format!("{}{octave}", NAMES[usize::from(note % 12)])
}

fn render_home(selected: usize) -> [String; 2] {
    let mut frame = Frame::new(DISPLAY_COLUMNS, 2);
    for (index, (item, area)) in [
        ("LIVE", Rect::new(0, 0, 8, 1)),
        ("PLAY", Rect::new(10, 0, 8, 1)),
        ("CONFIG", Rect::new(4, 1, 10, 1)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut button = Button::new(format!("home-{index}"), item);
        button.set_focused(index == selected);
        button.render(&mut frame, area);
    }
    [
        TextFallback::default().row(&frame, 0),
        TextFallback::default().row(&frame, 1),
    ]
}

fn indexed_title(title: &str, index: usize, len: usize) -> String {
    let counter = format!("{}/{}", index + 1, len);
    let maximum_title = DISPLAY_COLUMNS.saturating_sub(counter.len() + 1);
    let title = title.chars().take(maximum_title).collect::<String>();
    let spacing = DISPLAY_COLUMNS.saturating_sub(title.len() + counter.len());
    format!("{title}{}{counter}", " ".repeat(spacing))
}

fn simple_carousel(items: &[&str], details: &[&str], selected: usize) -> [String; 2] {
    assert_eq!(items.len(), details.len());
    let mut carousel = SimpleCarousel::new(
        "page-selection",
        items
            .iter()
            .zip(details)
            .map(|(item, detail)| CarouselItem::new(*item, *detail)),
    );
    carousel.set_selected(selected);
    carousel.set_focused(true);
    component_lines(&carousel, false)
}

fn simple_screen(
    header: impl Into<String>,
    items: &[&str],
    details: &[&str],
    selected: usize,
) -> Screen {
    let [line_1, line_2] = simple_carousel(items, details, selected);
    Screen::with_header(header, line_1, line_2)
}

fn component_lines(component: &impl Component, mark_focus: bool) -> [String; 2] {
    let mut frame = Frame::new(DISPLAY_COLUMNS, 2);
    component.render(&mut frame, Rect::new(0, 0, DISPLAY_COLUMNS, 2));
    let text = TextFallback::new(mark_focus);
    [text.row(&frame, 0), text.row(&frame, 1)]
}

fn envelope_carousel(layer: Option<&serde_json::Value>) -> ValueCarousel {
    focused_values(
        "rf-dls-envelope",
        [
            optional_value(
                "amplitude_envelope.attack_seconds",
                "ATTACK",
                json_optional_number(layer, "/parameters/amplitude_envelope/attack_seconds"),
                0.01,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
            optional_value(
                "amplitude_envelope.decay_seconds",
                "DECAY",
                json_optional_number(layer, "/parameters/amplitude_envelope/decay_seconds"),
                0.5,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
            optional_value(
                "amplitude_envelope.sustain_level",
                "SUSTAIN",
                json_optional_number(layer, "/parameters/amplitude_envelope/sustain_level"),
                1.0,
                0.0,
                1.0,
                0.01,
                2,
                "x",
            ),
            optional_value(
                "amplitude_envelope.release_seconds",
                "RELEASE",
                json_optional_number(layer, "/parameters/amplitude_envelope/release_seconds"),
                0.1,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
        ],
    )
}

fn pitch_envelope_carousel(layer: Option<&serde_json::Value>) -> ValueCarousel {
    focused_values(
        "rf-dls-pitch-envelope",
        [
            optional_value(
                "pitch_envelope.attack_seconds",
                "ATTACK",
                json_optional_number(layer, "/parameters/pitch_envelope/attack_seconds"),
                0.01,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
            optional_value(
                "pitch_envelope.decay_seconds",
                "DECAY",
                json_optional_number(layer, "/parameters/pitch_envelope/decay_seconds"),
                0.5,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
            optional_value(
                "pitch_envelope.sustain_level",
                "SUSTAIN",
                json_optional_number(layer, "/parameters/pitch_envelope/sustain_level"),
                1.0,
                0.0,
                1.0,
                0.01,
                2,
                "x",
            ),
            optional_value(
                "pitch_envelope.release_seconds",
                "RELEASE",
                json_optional_number(layer, "/parameters/pitch_envelope/release_seconds"),
                0.1,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
            optional_value(
                "pitch_envelope.depth_cents",
                "DEPTH",
                json_optional_number(layer, "/parameters/pitch_envelope/depth_cents"),
                100.0,
                -4_800.0,
                4_800.0,
                10.0,
                0,
                "ct",
            ),
        ],
    )
}

fn lfo_carousel(layer: Option<&serde_json::Value>) -> ValueCarousel {
    focused_values(
        "rf-dls-lfo",
        [
            ValueItem::new(
                "lfo.enabled",
                "MODE",
                EditableValue::choice(
                    match layer.and_then(|layer| layer.pointer("/parameters/lfo/enabled")) {
                        Some(serde_json::Value::Bool(true)) => 1,
                        Some(serde_json::Value::Bool(false)) => 2,
                        _ => 0,
                    },
                    ["INHERIT", "ON", "OFF"],
                ),
            ),
            optional_value(
                "lfo.frequency_hz",
                "RATE",
                json_optional_number(layer, "/parameters/lfo/frequency_hz"),
                5.0,
                0.01,
                50.0,
                0.01,
                2,
                "Hz",
            ),
            optional_value(
                "lfo.delay_seconds",
                "DELAY",
                json_optional_number(layer, "/parameters/lfo/delay_seconds"),
                0.0,
                0.0,
                60.0,
                0.01,
                2,
                "s",
            ),
            optional_value(
                "lfo.pitch_depth_cents",
                "PITCH",
                json_optional_number(layer, "/parameters/lfo/pitch_depth_cents"),
                10.0,
                -2_400.0,
                2_400.0,
                1.0,
                0,
                "ct",
            ),
            optional_value(
                "lfo.mod_wheel_pitch_depth_cents",
                "WHEEL PITCH",
                json_optional_number(layer, "/parameters/lfo/mod_wheel_pitch_depth_cents"),
                50.0,
                -2_400.0,
                2_400.0,
                1.0,
                0,
                "ct",
            ),
            optional_value(
                "lfo.attenuation_depth_centibels",
                "AMP",
                json_optional_number(layer, "/parameters/lfo/attenuation_depth_centibels"),
                10.0,
                -10_000.0,
                10_000.0,
                1.0,
                0,
                "cb",
            ),
            optional_value(
                "lfo.mod_wheel_attenuation_depth_centibels",
                "WHEEL AMP",
                json_optional_number(
                    layer,
                    "/parameters/lfo/mod_wheel_attenuation_depth_centibels",
                ),
                50.0,
                -10_000.0,
                10_000.0,
                1.0,
                0,
                "cb",
            ),
        ],
    )
}

fn tuning_carousel(layer: Option<&serde_json::Value>) -> ValueCarousel {
    focused_values(
        "rf-dls-tuning",
        [
            ValueItem::new(
                "transpose_semitones",
                "TRANSPOSE",
                EditableValue::integer(
                    json_integer(layer, "/parameters/transpose_semitones", 0),
                    -48,
                    48,
                    1,
                    "st",
                ),
            ),
            ValueItem::new(
                "fine_tune_cents",
                "FINE TUNE",
                EditableValue::number(
                    json_number(layer, "/parameters/fine_tune_cents", 0.0),
                    -100.0,
                    100.0,
                    1.0,
                    0,
                    "ct",
                ),
            ),
            ValueItem::new(
                "pitch_bend_range_semitones",
                "BEND RANGE",
                EditableValue::number(
                    json_number(layer, "/parameters/pitch_bend_range_semitones", 2.0),
                    0.0,
                    24.0,
                    1.0,
                    0,
                    "st",
                ),
            ),
        ],
    )
}

fn range_carousel(layer: Option<&serde_json::Value>) -> ValueCarousel {
    focused_values(
        "rf-dls-range",
        [
            ValueItem::new(
                "key_range.low",
                "KEY LOW",
                EditableValue::integer(json_integer(layer, "/key_range/low", 0), 0, 127, 1, ""),
            ),
            ValueItem::new(
                "key_range.high",
                "KEY HIGH",
                EditableValue::integer(json_integer(layer, "/key_range/high", 127), 0, 127, 1, ""),
            ),
            ValueItem::new(
                "velocity_range.low",
                "VEL LOW",
                EditableValue::integer(
                    json_integer(layer, "/velocity_range/low", 0),
                    0,
                    127,
                    1,
                    "",
                ),
            ),
            ValueItem::new(
                "velocity_range.high",
                "VEL HIGH",
                EditableValue::integer(
                    json_integer(layer, "/velocity_range/high", 127),
                    0,
                    127,
                    1,
                    "",
                ),
            ),
        ],
    )
}

fn layer_level_carousel(layer: Option<&serde_json::Value>) -> ValueCarousel {
    focused_values(
        "rf-dls-layer-level",
        [
            ValueItem::new(
                "gain",
                "VOLUME",
                EditableValue::number(
                    json_number(layer, "/parameters/gain", 1.0),
                    0.0,
                    2.0,
                    0.01,
                    2,
                    "x",
                ),
            ),
            ValueItem::new(
                "modulation_depth",
                "MOD DEPTH",
                EditableValue::number(
                    json_number(layer, "/parameters/modulation_depth", 1.0),
                    0.0,
                    2.0,
                    0.01,
                    2,
                    "x",
                ),
            ),
        ],
    )
}

fn program_output_carousel(gain: f64) -> ValueCarousel {
    focused_values(
        "rf-dls-program-output",
        [ValueItem::new(
            "gain",
            "GAIN",
            EditableValue::number(gain, 0.0, 2.0, 0.01, 2, "x"),
        )],
    )
}

fn json_optional_number(layer: Option<&serde_json::Value>, pointer: &str) -> Option<f64> {
    layer
        .and_then(|layer| layer.pointer(pointer))
        .and_then(serde_json::Value::as_f64)
}

fn json_number(layer: Option<&serde_json::Value>, pointer: &str, default: f64) -> f64 {
    json_optional_number(layer, pointer).unwrap_or(default)
}

fn json_integer(layer: Option<&serde_json::Value>, pointer: &str, default: i64) -> i64 {
    layer
        .and_then(|layer| layer.pointer(pointer))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(default)
}

#[allow(clippy::too_many_arguments)] // Mirrors EditableValue::optional_number plus id and label.
fn optional_value(
    id: &str,
    label: &str,
    value: Option<f64>,
    initial: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    decimals: usize,
    unit: &str,
) -> ValueItem {
    ValueItem::new(
        id,
        label,
        EditableValue::optional_number(value, initial, minimum, maximum, step, decimals, unit),
    )
}

fn focused_values(id: &str, items: impl IntoIterator<Item = ValueItem>) -> ValueCarousel {
    let mut carousel = ValueCarousel::new(id, items);
    carousel.set_focused(true);
    carousel
}

fn program_name_editor(name: &str) -> TextEditor {
    let mut editor = TextEditor::new("rf-dls-program-name", "NAME", name, 16);
    editor.set_focused(true);
    editor
}

fn plugin_preset_name_editor(name: &str) -> TextEditor {
    let mut editor = TextEditor::new("plugin-preset-name", "PRESET NAME", name, 24);
    editor.set_focused(true);
    editor
}

fn performance_text_editor(id: &str, label: &str, name: &str) -> TextEditor {
    let mut editor = TextEditor::new(id, label, name, 24);
    editor.set_focused(true);
    editor
}

fn performance_confirmation() -> ConfirmationDialog {
    let mut dialog =
        ConfirmationDialog::new("performance-unsaved", "SAVE CHANGES?", ["SAVE", "DISCARD"]);
    dialog.set_focused(true);
    dialog
}

fn delete_confirmation() -> ConfirmationDialog {
    let mut dialog =
        ConfirmationDialog::new("performance-delete", "DELETE ITEM?", ["DELETE", "CANCEL"]);
    dialog.set_focused(true);
    dialog
}

fn move_wrapped(selection: &mut usize, len: usize, delta: isize) {
    if len > 0 {
        *selection = (*selection as isize + delta).rem_euclid(len as isize) as usize;
    }
}

fn rack_slot_sound_hint(slot: &RackSlot) -> Option<&str> {
    slot.state
        .as_ref()
        .and_then(|state| state.selected_sound_id.as_deref())
        .or(slot.legacy_program_id.as_deref())
}

fn next_available_id<'a>(prefix: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let existing = existing.collect::<std::collections::BTreeSet<_>>();
    (1_u32..)
        .map(|index| format!("{prefix}.{index:03}"))
        .find(|candidate| !existing.contains(candidate.as_str()))
        .expect("finite performance library always has a free id")
}

fn wifi_password_editor() -> SecretEditor {
    let mut editor = SecretEditor::new("system-wifi-password", "PASSWORD", 64);
    editor.set_focused(true);
    editor
}

fn unsaved_changes_dialog() -> ConfirmationDialog {
    let mut dialog =
        ConfirmationDialog::new("plugin-unsaved", "SAVE CHANGES?", ["SAVE", "DISCARD"]);
    dialog.set_focused(true);
    dialog
}

pub fn demo_frames() -> Vec<Screen> {
    let mut menu = Menu::default();
    let mut screens = vec![menu.render()];
    for input in Input::PHYSICAL {
        menu.apply_input(input);
        screens.push(menu.render());
    }
    for action in [
        Action::Select,
        Action::Next,
        Action::Previous,
        Action::Next,
        Action::Back,
        Action::Next,
        Action::Select,
        Action::Back,
        Action::Next,
        Action::Select,
        Action::Select,
        Action::Next,
    ] {
        menu.apply(action);
        screens.push(menu.render());
    }
    screens
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_audio_api::{
        AUDIO_DEVICE_SCHEMA_VERSION, AUDIO_OUTPUT_STATE_SCHEMA_VERSION, AudioBackend,
        AudioDeviceId, AudioStreamCapabilities, AudioTransport, AudioValueRange,
    };
    use rackforge_performance_api::{
        MidiOutputRoute, PERFORMANCE_SCHEMA_VERSION, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
        RackDefinition, RackId, RackSlot, RackSlotId, SetlistDefinition, SetlistEntry,
        SetlistEntryId, SetlistId, SongDefinition, SongId, SongPart, SongPartId,
    };
    use rackforge_session_api::InstanceId;

    fn assert_plugin_header(screen: &Screen, expected_name: &str, expected_context: &str) {
        assert!(matches!(
            &screen.header,
            Header::Plugin { name, context, .. }
                if name == expected_name && context == expected_context
        ));
    }

    #[test]
    fn plugin_header_projects_responsively_without_new_layout_ids() {
        let header = Header::Plugin {
            name: "RackForge Concert Grand".into(),
            short_name: "RF-GRAND".into(),
            context: "PROGRAM 12/128".into(),
        };
        assert_eq!(header.text(18).as_deref(), Some("RF-GRAND PROGRAM 1"));
        assert_eq!(header.text(12).as_deref(), Some("RF-GR PROGRA"));
        assert_eq!(
            header.text(40).as_deref(),
            Some("RACKFORGE CONCERT GRAND   PROGRAM 12/128")
        );

        let identity_only = Header::Plugin {
            name: "RF-106".into(),
            short_name: "RF-106".into(),
            context: " ".into(),
        };
        assert_eq!(
            identity_only.text(18).as_deref(),
            Some("RF-106            ")
        );

        let rf_106_control = Header::Plugin {
            name: "RF-106".into(),
            short_name: "RF-106".into(),
            context: "CTRL 1/9".into(),
        };
        let projected = rf_106_control.text(18).unwrap();
        assert_eq!(projected.chars().count(), 18);
        assert!(projected.starts_with("RF-106"));
        assert_eq!(projected.as_bytes()[0], b'R');
    }

    #[test]
    fn play_plugin_carousel_selects_a_real_runtime_instance() {
        let mut menu = Menu::default();
        menu.set_play_plugins(
            vec![
                PlayPlugin::new("live.main.instrument.1", "org.rackforge.rf-dls", "RF-DLS"),
                PlayPlugin::new(
                    "play.org.rackforge.rf-kr106",
                    "org.rackforge.rf-kr106",
                    "RF-KR106",
                ),
            ],
            Some("live.main.instrument.1"),
        );
        menu.set_active_plugin("org.rackforge.rf-dls", "RF-DLS");
        menu.page = Page::Play;
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "RF-KR106");
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SelectPlugin {
                instance_id: "play.org.rackforge.rf-kr106".into(),
            })
        );
        assert_eq!(menu.page, Page::PluginLibrary);
    }

    #[test]
    fn plugin_switch_hides_stale_content_and_blocks_input_until_target_snapshot_arrives() {
        let mut menu = Menu::default();
        let plugins = vec![
            PlayPlugin::new("play.piano", "org.example.piano", "PIANO"),
            PlayPlugin::new("play.synth", "org.example.synth", "SYNTH"),
        ];
        menu.set_play_plugins(plugins.clone(), Some("play.piano"));
        assert!(menu.sync_active_plugin(
            "play.piano",
            "org.example.piano",
            "PIANO",
            vec![PlaySound::new("piano.old", "OLD PIANO", "keys", "STALE")],
            Some("piano.old"),
        ));
        menu.page = Page::Play;
        menu.apply(Action::Next);
        menu.apply(Action::Select);

        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SelectPlugin {
                instance_id: "play.synth".into(),
            })
        );
        assert!(menu.is_plugin_loading());
        let loading = menu.render();
        assert_plugin_header(&loading, "SYNTH", "LOADING");
        assert_eq!(loading.line_1.trim(), "LOADING PLUGIN");
        assert!(loading.line_2.contains("PLEASE WAIT"));
        assert!(!loading.line_1.contains("OLD PIANO"));

        let page = menu.page;
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button4);
        assert_eq!(menu.page, page);
        assert_eq!(menu.take_command(), None);
        assert!(menu.advance_plugin_spinner());
        assert_ne!(menu.render().line_2, loading.line_2);

        // A late snapshot for the old instance cannot reveal stale programs or unlock input.
        menu.set_play_plugins(plugins.clone(), Some("play.piano"));
        assert!(!menu.sync_active_plugin(
            "play.piano",
            "org.example.piano",
            "PIANO",
            vec![PlaySound::new("piano.old", "OLD PIANO", "keys", "STALE")],
            Some("piano.old"),
        ));
        assert!(menu.is_plugin_loading());
        assert_eq!(menu.render().line_1.trim(), "LOADING PLUGIN");

        menu.set_play_plugins(plugins, Some("play.synth"));
        assert!(menu.sync_active_plugin(
            "play.synth",
            "org.example.synth",
            "SYNTH",
            vec![PlaySound::new("synth.new", "NEW SYNTH", "leads", "READY")],
            Some("synth.new"),
        ));
        assert!(!menu.is_plugin_loading());
        assert_eq!(menu.render().line_1.trim(), "PRESETS");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1.trim(), "PROGRAMS");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "[NEW SYNTH]");
    }

    #[test]
    fn plugin_without_config_mode_shows_an_explicit_little_message() {
        let mut menu = Menu::default();
        menu.set_play_plugins(
            vec![
                PlayPlugin::new(
                    "play.org.rackforge.rf-kr106",
                    "org.rackforge.rf-kr106",
                    "RF-KR106",
                )
                .config_available(false),
            ],
            Some("play.org.rackforge.rf-kr106"),
        );
        menu.set_active_plugin("org.rackforge.rf-kr106", "RF-KR106");
        menu.page = Page::Plugins;

        menu.apply(Action::Select);

        assert_eq!(menu.page, Page::PluginConfigUnavailable);
        let screen = menu.render();
        assert_eq!(screen.line_1, "CONFIG UNAVAILABLE");
        assert_eq!(screen.line_2, "USE PLAY + PRESETS");

        menu.apply(Action::Back);
        assert_eq!(menu.page, Page::Plugins);
    }

    #[test]
    fn plugin_with_config_mode_keeps_its_little_editor() {
        let mut menu = Menu::default();
        menu.set_play_plugins(
            vec![
                PlayPlugin::new("live.main.instrument.1", "org.rackforge.rf-dls", "RF-DLS")
                    .config_available(true),
            ],
            Some("live.main.instrument.1"),
        );
        menu.set_active_plugin("org.rackforge.rf-dls", "RF-DLS");
        menu.page = Page::Plugins;

        menu.apply(Action::Select);

        assert_eq!(menu.page, Page::PluginCustomPrograms);
    }

    fn test_live_parameter_schema() -> ParameterSchema {
        use rackforge_plugin_api::{
            EnumChoice, PARAMETER_SCHEMA_VERSION, PageDescriptor, ParameterFlags, SuggestedControl,
        };

        let parameter =
            |index, id: &str, name: &str, page: &str, order, kind| ParameterDescriptor {
                index,
                id: id.into(),
                name: name.into(),
                page: page.into(),
                group: None,
                order,
                kind,
                flags: ParameterFlags {
                    automatable: true,
                    modulatable: false,
                    read_only: false,
                    advanced: false,
                },
                suggested_control: SuggestedControl::Automatic,
            };
        ParameterSchema {
            schema_version: PARAMETER_SCHEMA_VERSION,
            display_decimals: Some(2),
            pages: vec![
                PageDescriptor {
                    id: "level".into(),
                    name: "Level".into(),
                    order: 0,
                    header: None,
                },
                PageDescriptor {
                    id: "mode".into(),
                    name: "Mode".into(),
                    order: 1,
                    header: None,
                },
            ],
            parameters: vec![
                parameter(
                    0,
                    "gain",
                    "Gain",
                    "level",
                    0,
                    ParameterKind::Float {
                        minimum: 0.0,
                        maximum: 1.0,
                        default: 0.5,
                        step: 0.1,
                        unit: None,
                    },
                ),
                parameter(
                    1,
                    "enabled",
                    "Enabled",
                    "mode",
                    0,
                    ParameterKind::Boolean { default: true },
                ),
                parameter(
                    2,
                    "shape",
                    "Shape",
                    "mode",
                    1,
                    ParameterKind::Enum {
                        default: 10,
                        choices: vec![
                            EnumChoice {
                                value: 10,
                                name: "Soft".into(),
                            },
                            EnumChoice {
                                value: 20,
                                name: "Hard".into(),
                            },
                        ],
                    },
                ),
                parameter(3, "fire", "Fire", "mode", 2, ParameterKind::Trigger),
            ],
            semantic_controls: Vec::new(),
        }
    }

    #[test]
    fn little_exposes_plugin_parameter_pages_and_dispatches_normal_parameter_events() {
        let mut menu = Menu::default();
        menu.set_play_plugins(
            vec![PlayPlugin::new("play.synth", "org.example.synth", "SYNTH")],
            Some("play.synth"),
        );
        assert!(menu.sync_active_plugin(
            "play.synth",
            "org.example.synth",
            "SYNTH",
            vec![PlaySound::new("factory.0", "Init", "Factory", "Init")],
            Some("factory.0"),
        ));
        menu.sync_plugin_parameters(
            test_live_parameter_schema(),
            [(0, 0.5), (1, 1.0), (2, 10.0), (3, 0.0)],
        );
        menu.page = Page::PluginLibrary;

        assert_eq!(menu.render().line_1.trim(), "PRESETS");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1.trim(), "PROGRAMS");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1.trim(), "CONTROLS");
        assert_eq!(menu.render().line_2.trim(), "4 PARAMETERS");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "LEVEL");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "GAIN");
        menu.apply(Action::Select);
        assert_eq!(menu.page, Page::PluginParameterValue);
        menu.apply(Action::Next);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetPluginParameter {
                instance_id: "play.synth".into(),
                parameter_index: 0,
                value: 0.6,
            })
        );
        menu.complete_plugin_parameter_set(0, 0.6);
        menu.apply(Action::Select);
        assert_eq!(menu.page, Page::PluginParameters);
        assert_eq!(menu.render().line_2.trim(), "0.60");
    }

    #[test]
    fn little_preserves_enum_values_and_emits_trigger_gestures() {
        let mut menu = Menu::default();
        menu.set_play_plugins(
            vec![PlayPlugin::new("play.synth", "org.example.synth", "SYNTH")],
            Some("play.synth"),
        );
        menu.sync_active_plugin(
            "play.synth",
            "org.example.synth",
            "SYNTH",
            vec![PlaySound::new("factory.0", "Init", "Factory", "Init")],
            Some("factory.0"),
        );
        menu.sync_plugin_parameters(
            test_live_parameter_schema(),
            [(0, 0.5), (1, 1.0), (2, 10.0), (3, 0.0)],
        );
        menu.plugin_parameter_page_index = 1;
        menu.plugin_parameter_index = 1;
        menu.page = Page::PluginParameters;
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetPluginParameter {
                instance_id: "play.synth".into(),
                parameter_index: 2,
                value: 20.0,
            })
        );

        menu.page = Page::PluginParameters;
        menu.plugin_parameter_editor = None;
        menu.plugin_parameter_index = 2;
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::TriggerPluginParameter {
                instance_id: "play.synth".into(),
                parameter_index: 3,
            })
        );
    }

    fn plugin_menu() -> Menu {
        let mut menu = Menu::default();
        menu.set_active_plugin("org.rackforge.rf-dls", "RF-DLS");
        menu.set_play_sounds(
            vec![PlaySound::new(
                "dls.b00000000.p00000000",
                "PIANO 1",
                "dls",
                "B000 P000",
            )],
            Some("dls.b00000000.p00000000"),
        );
        menu
    }

    fn test_audio_state() -> AudioOutputState {
        let scarlett = AudioDeviceDescriptor {
            schema_version: AUDIO_DEVICE_SCHEMA_VERSION,
            id: AudioDeviceId::new("alsa.usb-scarlett.pcm-0").unwrap(),
            name: "Scarlett Solo USB".into(),
            backend: AudioBackend::Alsa,
            backend_address: "hw:3,0".into(),
            transport: AudioTransport::Usb,
            usb: Some(rackforge_audio_api::UsbAudioIdentity {
                vendor_id: 0x1235,
                product_id: 0x8211,
                serial: Some("TEST".into()),
            }),
            playback: Some(AudioStreamCapabilities {
                sample_formats: vec![AudioSampleFormat::S32Le],
                sample_rates_hz: vec![44_100, 48_000, 96_000],
                channels: AudioValueRange::new(2, 2).unwrap(),
                period_frames: AudioValueRange::new(8, 4096).unwrap(),
                buffer_frames: AudioValueRange::new(16, 8192).unwrap(),
            }),
            capture: None,
        };
        AudioOutputState {
            schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
            active_device: scarlett.clone(),
            active_profile: AudioOutputProfile {
                device: AudioDeviceSelector::Id {
                    id: scarlett.id.clone(),
                },
                fallback: AudioFallbackPolicy::None,
                sample_format: AudioSampleFormat::S32Le,
                sample_rate_hz: 48_000,
                channels: 2,
                period_frames: 128,
                buffer_frames: 384,
            },
            devices: vec![scarlett],
        }
    }

    fn test_performance_snapshot() -> PerformanceSnapshot {
        let rack_id = RackId::new("rack.stage-piano").unwrap();
        let song_id = SongId::new("song.opener").unwrap();
        let part_id = SongPartId::new("part.intro").unwrap();
        let rack = RackDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: rack_id.clone(),
            name: "Stage Piano".into(),
            enabled: true,
            keyboard_parts: None,
            graph: None,
            slots: vec![RackSlot {
                id: RackSlotId::new("instrument.main").unwrap(),
                name: "Main Instrument".into(),
                plugin_id: "org.rackforge.rf-dls".into(),
                state: None,
                legacy_program_id: Some("custom.user.stage-piano".into()),
                enabled: true,
                midi_input_channel: None,
                midi_note_low: 0,
                midi_note_high: 127,
                midi_transpose: 0,
                midi_output: MidiOutputRoute::None,
                audio_output_bus: "main".into(),
                level_per_mille: 1_000,
                pan_per_mille: 0,
            }],
        };
        let song = SongDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: song_id.clone(),
            name: "Opener".into(),
            enabled: true,
            parts: vec![SongPart {
                id: part_id.clone(),
                name: "Intro".into(),
                rack_id: rack_id.clone(),
                content: None,
                patterns: Vec::new(),
            }],
        };
        let setlist = SetlistDefinition {
            schema_version: PERFORMANCE_SCHEMA_VERSION,
            id: SetlistId::new("setlist.friday").unwrap(),
            name: "Friday".into(),
            enabled: true,
            entries: vec![SetlistEntry {
                id: SetlistEntryId::new("entry.opener").unwrap(),
                song_id,
            }],
        };
        let rack_location = LiveLocation::Rack {
            rack_id: rack_id.clone(),
        };
        PerformanceSnapshot {
            schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
            revision: LibraryRevision::new("0".repeat(64)).unwrap(),
            library: PerformanceLibrary {
                schema_version: PERFORMANCE_SCHEMA_VERSION,
                racks: vec![rack],
                songs: vec![song],
                setlists: vec![setlist],
                patterns: Vec::new(),
                sequencer_tabs: Vec::new(),
            },
            live: LivePerformanceState {
                mode: LiveBrowseMode::Rack,
                rack: Some(rack_location.clone()),
                song: None,
                setlist: None,
                active: Some(rack_location),
                active_rack_id: Some(rack_id),
            },
        }
    }

    /// The fixture holds one Rack, which cannot show a cursor moving. This
    /// one has somewhere to move to.
    fn snapshot_with_two_racks() -> PerformanceSnapshot {
        let mut snapshot = test_performance_snapshot();
        let mut organ = snapshot.library.racks[0].clone();
        organ.id = RackId::new("rack.organ").unwrap();
        organ.name = "Organ".into();
        snapshot.library.racks.push(organ);
        snapshot
    }

    /// Walks into LIVE and opens the Rack list, leaving the cursor on the
    /// Rack that is sounding.
    fn browse_live_racks(menu: &mut Menu) {
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        let _ = menu.take_command();
    }

    #[test]
    fn a_library_edit_leaves_the_browsing_cursor_where_the_player_put_it() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(snapshot_with_two_racks());
        browse_live_racks(&mut menu);
        assert_eq!(menu.render().line_1, "Stage Piano");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "Organ");

        // Someone saves a deck tab on the web surface: the library changes,
        // what is sounding does not. Every client is handed the snapshot.
        let mut edited = snapshot_with_two_racks();
        edited.revision = LibraryRevision::new("1".repeat(64)).unwrap();
        edited
            .library
            .sequencer_tabs
            .push(rackforge_performance_api::SequencerTabDefinition {
                lane: 0,
                view: Default::default(),
                slot_ids: Vec::new(),
                active_slot: 0,
            });
        menu.sync_performance_snapshot(edited);

        assert_eq!(
            menu.render().line_1,
            "Organ",
            "an unrelated edit pulled the cursor back to what is sounding"
        );
    }

    #[test]
    fn a_new_live_target_still_takes_the_screen_to_what_is_sounding() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(snapshot_with_two_racks());
        browse_live_racks(&mut menu);
        assert_eq!(menu.render().line_1, "Stage Piano");

        // Another surface activates the Organ. The screen belongs to the
        // machine's state, so the cursor follows it there.
        let organ_id = RackId::new("rack.organ").unwrap();
        let organ = LiveLocation::Rack {
            rack_id: organ_id.clone(),
        };
        let mut activated = snapshot_with_two_racks();
        activated.live.rack = Some(organ.clone());
        activated.live.active = Some(organ);
        activated.live.active_rack_id = Some(organ_id);
        menu.sync_performance_snapshot(activated);

        assert_eq!(menu.render().line_1, "Organ");
    }

    #[test]
    fn a_cursor_whose_rack_was_deleted_falls_back_to_what_is_sounding() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(snapshot_with_two_racks());
        browse_live_racks(&mut menu);
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "Organ");

        // The Organ is deleted elsewhere; the cursor has nothing to hold.
        let mut without_organ = snapshot_with_two_racks();
        without_organ
            .library
            .racks
            .retain(|rack| rack.name != "Organ");
        menu.sync_performance_snapshot(without_organ);

        assert_eq!(menu.render().line_1, "Stage Piano");
    }

    #[test]
    fn live_exposes_rack_song_and_setlist_without_forcing_a_hierarchy() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(test_performance_snapshot());
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "RACK");
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetLiveBrowseMode {
                mode: LiveBrowseMode::Rack
            })
        );
        assert_eq!(menu.render().line_1, "Stage Piano");
        assert_eq!(menu.render().line_2, "ACTIVE");
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::ActivateLiveTarget {
                location: LiveLocation::Rack {
                    rack_id: RackId::new("rack.stage-piano").unwrap()
                }
            })
        );

        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "Opener");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "Intro");

        menu.apply(Action::Back);
        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "Friday");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "Opener");
        assert_eq!(menu.render().line_2, "Intro");
    }

    #[test]
    fn keyboard_parts_shortcut_opens_and_edits_the_active_rack_zone() {
        let mut menu = plugin_menu();
        let mut snapshot = test_performance_snapshot();
        let mut right = snapshot.library.racks[0].slots[0].clone();
        right.id = RackSlotId::new("instrument.right").unwrap();
        right.name = "Bass".into();
        snapshot.library.racks[0].slots.push(right);
        menu.sync_performance_snapshot(snapshot);

        menu.apply_input(Input::KeyboardParts);
        let screen = menu.render();
        assert!(
            matches!(screen.header, Header::Visible(ref value) if value.starts_with("KEYBOARD"))
        );
        assert_eq!(screen.line_1, "PART 1");
        assert_eq!(screen.line_2, "FULL RANGE CH 1");

        menu.apply_input(Input::KeyboardParts);
        assert_eq!(menu.render().header, Header::Visible(HOME_HEADER.into()));
        menu.apply_input(Input::KeyboardParts);
        menu.apply_input(Input::EncoderPress);
        assert_eq!(menu.render().line_1, "MIDI CH");
        menu.apply_input(Input::Button3);
        assert_eq!(menu.render().line_1, "SPLIT KEY");
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);

        let Some(PerformanceDraft::Rack(rack)) = menu.performance_draft.as_ref() else {
            panic!("keyboard Part editor did not create a Rack draft");
        };
        assert_eq!(rack.keyboard_parts.unwrap().split_key, Some(61));
        assert_eq!(rack.slots[0].midi_note_low, 0);
        assert_eq!(rack.slots[0].midi_note_high, 127);
        assert_eq!(rack.slots[1].midi_note_low, 0);
        assert_eq!(rack.slots[1].midi_note_high, 127);
        assert!(menu.performance_dirty);
    }

    #[test]
    fn part_note_gesture_saves_one_shared_split_atomically() {
        let mut menu = plugin_menu();
        let mut snapshot = test_performance_snapshot();
        let mut right = snapshot.library.racks[0].slots[0].clone();
        right.id = RackSlotId::new("instrument.right").unwrap();
        snapshot.library.racks[0].slots.push(right);
        menu.sync_performance_snapshot(snapshot);

        menu.apply_input(Input::KeyboardSplitNote(64));
        let Some(MenuCommand::EditPerformance {
            edit: PerformanceEdit::PutRack { rack },
            ..
        }) = menu.take_command()
        else {
            panic!("split gesture did not request an atomic Rack save");
        };
        assert_eq!(rack.keyboard_parts.unwrap().split_key, Some(64));
        assert_eq!(rack.slots[0].midi_note_low, 0);
        assert_eq!(rack.slots[0].midi_note_high, 127);
        assert_eq!(rack.slots[1].midi_note_low, 0);
        assert_eq!(rack.slots[1].midi_note_high, 127);
    }

    #[test]
    fn rack_configuration_creates_a_transactional_draft() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(test_performance_snapshot());
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "NAME");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "SLOTS");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "+ ADD SLOT");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "SLOT 1");
        assert_eq!(menu.render().line_2, " ");
        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "SAVE");
        menu.apply(Action::Select);
        let Some(MenuCommand::EditPerformance {
            expected_revision,
            edit: PerformanceEdit::PutRack { rack },
        }) = menu.take_command()
        else {
            panic!("expected a typed Rack edit");
        };
        assert_eq!(expected_revision.as_str(), "0".repeat(64));
        assert_eq!(rack.id.as_str(), "rack.user.001");
        assert_eq!(rack.slots.len(), 1);
        assert_eq!(rack.slots[0].plugin_id, "org.rackforge.rf-dls");
        assert_eq!(
            rack.slots[0].legacy_program_id.as_deref(),
            Some("dls.b00000000.p00000000")
        );
        assert_eq!(menu.render().line_1.trim(), "SAVING");
    }

    #[test]
    fn rack_slot_enters_the_plugin_play_surface_without_exposing_programs_to_rack() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(test_performance_snapshot());
        menu.set_play_sounds(
            vec![
                PlaySound::new("dls.piano", "Piano", "dls", "B000 P000"),
                PlaySound::new("dls.strings", "Strings", "dls", "B000 P048"),
                PlaySound::new(
                    "custom.user.warm-piano",
                    "Warm Piano",
                    "custom",
                    "CUSTOM 001",
                )
                .editable(true),
            ],
            Some("dls.piano"),
        );
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "RF-DLS");
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "PLAY 1/1");
        assert_eq!(menu.render().line_1.trim(), "PROGRAMS");
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "BANK 1/2");
        assert_eq!(menu.render().line_1.trim(), "DLS");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1.trim(), "CUSTOM");
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("WARM PIANO"));
        menu.apply(Action::Select);
        let Some(MenuCommand::PreviewRack { rack }) = menu.take_command() else {
            panic!("expected the complete Rack draft to be previewed");
        };
        assert_eq!(rack.slots.len(), 1);
        assert_eq!(
            rack.slots[0].legacy_program_id.as_deref(),
            Some("custom.user.warm-piano")
        );
        assert_eq!(
            menu.focused_rack_slot().and_then(rack_slot_sound_hint),
            Some("custom.user.warm-piano")
        );
        menu.apply(Action::Back);
        assert_plugin_header(&menu.render(), "RF-DLS", "BANK 2/2");
        menu.apply(Action::Back);
        assert_eq!(menu.render().line_1.trim(), "PROGRAMS");
        menu.apply(Action::Back);
        assert_eq!(menu.render().line_1, "RF-DLS");
    }

    #[test]
    fn rack_slot_plugin_carousel_lists_every_little_plugin() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(test_performance_snapshot());
        menu.set_play_plugins(
            vec![
                PlayPlugin::new("live.main.instrument.1", "org.rackforge.rf-dls", "RF-DLS"),
                PlayPlugin::new(
                    "play.org.rackforge.rf-kr106",
                    "org.rackforge.rf-kr106",
                    "RF-KR106",
                ),
            ],
            Some("live.main.instrument.1"),
        );
        menu.page = Page::ConfigRacks;
        menu.performance_list_index = 1;
        menu.begin_performance_draft();
        menu.page = Page::ConfigRackSlotPlugin;

        assert_eq!(menu.render().line_1, "RF-DLS");
        assert!(matches!(
            menu.render().header,
            Header::Visible(ref value) if value.contains("1/2")
        ));
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "RF-KR106");

        // A background catalog refresh must not snap the slot carousel back
        // to the currently sounding plugin while the user is choosing.
        menu.set_active_plugin("org.rackforge.rf-dls", "RF-DLS");
        assert_eq!(menu.render().line_1, "RF-KR106");

        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SelectPlugin {
                instance_id: "play.org.rackforge.rf-kr106".into(),
            })
        );
        let Some(PerformanceDraft::Rack(rack)) = menu.performance_draft.as_ref() else {
            panic!("Rack draft disappeared while selecting its plugin");
        };
        assert_eq!(rack.slots[0].plugin_id, "org.rackforge.rf-kr106");
        assert_eq!(rack.slots[0].state, None);
        assert_eq!(rack.slots[0].legacy_program_id, None);
        assert!(menu.performance_dirty);
        assert_eq!(menu.page, Page::PluginLibrary);
    }

    #[test]
    fn song_parts_and_setlist_entries_are_ordered_editable_children() {
        let mut menu = plugin_menu();
        menu.sync_performance_snapshot(test_performance_snapshot());
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "+ ADD PART");
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("PART 2"));
        menu.apply(Action::Back);
        assert_eq!(menu.render().line_1, "PARTS");
        assert_eq!(menu.render().line_2, "2 PARTS");
    }

    #[test]
    fn audio_menu_uses_runtime_state_and_emits_typed_changes() {
        let mut menu = plugin_menu();
        menu.sync_audio_state(test_audio_state());
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "AUDIO");
        assert!(menu.render().line_2.contains("Scarlett"));
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "OUTPUT");
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("Scarlett"));
        menu.apply(Action::Select);
        let Some(MenuCommand::ApplyAudioOutput { profile }) = menu.take_command() else {
            panic!("expected typed audio output command");
        };
        assert_eq!(profile.sample_rate_hz, 48_000);
        assert_eq!(profile.period_frames, 128);
    }

    fn test_number(id: &str, label: &str, value: Option<i64>, decimals: u8) -> ProgramEditorField {
        ProgramEditorField {
            id: id.into(),
            label: label.into(),
            detail: format!("{label} value"),
            value: value.map_or(ProgramEditorValue::Inherited, ProgramEditorValue::Integer),
            kind: ProgramEditorFieldKind::Number {
                minimum: if id.contains("depth") { -10_000 } else { 0 },
                maximum: 10_000,
                step: 1,
                decimals,
                unit: (id.contains("attack")
                    || id.contains("decay")
                    || id.contains("release")
                    || id.contains("delay"))
                .then(|| "s".into()),
                allow_inherited: value.is_none(),
            },
            live_preview: true,
        }
    }

    fn test_page(
        id: &str,
        label: &str,
        detail: &str,
        fields: Vec<ProgramEditorField>,
    ) -> ProgramEditorPage {
        ProgramEditorPage {
            id: id.into(),
            label: label.into(),
            detail: detail.into(),
            enabled: true,
            pages: Vec::new(),
            fields,
        }
    }

    fn test_layer(id: &str, enabled: bool, optional: bool) -> ProgramEditorPage {
        let prefix = format!("layer.{id}");
        let mut fields = Vec::new();
        if optional {
            fields.push(ProgramEditorField {
                id: format!("{prefix}.enabled"),
                label: "ENABLED".into(),
                detail: "Enable layer B".into(),
                value: ProgramEditorValue::Boolean(enabled),
                kind: ProgramEditorFieldKind::Toggle,
                live_preview: false,
            });
        }
        ProgramEditorPage {
            id: format!("layer-{id}"),
            label: format!("LAYER {}", id.to_ascii_uppercase()),
            detail: if optional {
                "Optional layer".into()
            } else {
                "Required layer".into()
            },
            enabled: !optional || enabled,
            fields,
            pages: vec![
                test_page(
                    &format!("{prefix}.timbre"),
                    "TIMBRE",
                    "DLS source",
                    vec![ProgramEditorField {
                        id: format!("{prefix}.sound"),
                        label: "TIMBRE".into(),
                        detail: "DLS source".into(),
                        value: ProgramEditorValue::SoundId("dls.b00000000.p00000000".into()),
                        kind: ProgramEditorFieldKind::Sound {
                            bank: Some("dls".into()),
                        },
                        live_preview: true,
                    }],
                ),
                test_page(
                    &format!("{prefix}.amp-env"),
                    "AMP ENV",
                    "Amplitude ADSR",
                    vec![
                        test_number(&format!("{prefix}.amp.attack"), "ATTACK", None, 2),
                        test_number(&format!("{prefix}.amp.decay"), "DECAY", None, 2),
                        test_number(&format!("{prefix}.amp.sustain"), "SUSTAIN", None, 2),
                        test_number(&format!("{prefix}.amp.release"), "RELEASE", None, 2),
                    ],
                ),
                test_page(
                    &format!("{prefix}.pitch-env"),
                    "PITCH ENV",
                    "Pitch EG override",
                    vec![test_number(
                        &format!("{prefix}.pitch.attack"),
                        "ATTACK",
                        None,
                        2,
                    )],
                ),
                test_page(
                    &format!("{prefix}.lfo"),
                    "LFO",
                    "Rate delay depth",
                    vec![
                        ProgramEditorField {
                            id: format!("{prefix}.lfo.enabled"),
                            label: "MODE".into(),
                            detail: "LFO override".into(),
                            value: ProgramEditorValue::Choice("inherit".into()),
                            kind: ProgramEditorFieldKind::Choice {
                                options: vec![
                                    rackforge_program_api::ProgramEditorChoice {
                                        value: "inherit".into(),
                                        label: "INHERIT".into(),
                                        detail: None,
                                    },
                                    rackforge_program_api::ProgramEditorChoice {
                                        value: "on".into(),
                                        label: "ON".into(),
                                        detail: None,
                                    },
                                    rackforge_program_api::ProgramEditorChoice {
                                        value: "off".into(),
                                        label: "OFF".into(),
                                        detail: None,
                                    },
                                ],
                            },
                            live_preview: true,
                        },
                        test_number(&format!("{prefix}.lfo.frequency"), "RATE", None, 2),
                        test_number(&format!("{prefix}.lfo.delay"), "DELAY", None, 2),
                    ],
                ),
                test_page(
                    &format!("{prefix}.tuning"),
                    "TUNING",
                    "Pitch and expression",
                    vec![test_number(
                        &format!("{prefix}.transpose"),
                        "TRANSPOSE",
                        Some(0),
                        0,
                    )],
                ),
                test_page(
                    &format!("{prefix}.range"),
                    "RANGE",
                    "Key and velocity",
                    vec![test_number(
                        &format!("{prefix}.key-low"),
                        "KEY LOW",
                        Some(0),
                        0,
                    )],
                ),
                test_page(
                    &format!("{prefix}.volume"),
                    "VOLUME",
                    "Layer mix gain",
                    vec![test_number(
                        &format!("{prefix}.gain"),
                        "VOLUME",
                        Some(100),
                        2,
                    )],
                ),
            ],
        }
    }

    fn test_editor(layer_b_enabled: bool) -> rackforge_program_api::ProgramEditorView {
        rackforge_program_api::ProgramEditorView {
            schema_version: rackforge_program_api::PROGRAM_EDITOR_SCHEMA_VERSION,
            title: "RF-DLS".into(),
            pages: vec![
                test_layer("a", true, false),
                test_layer("b", layer_b_enabled, true),
                ProgramEditorPage {
                    id: "fx".into(),
                    label: "FX".into(),
                    detail: "Shared FX chain".into(),
                    enabled: false,
                    pages: Vec::new(),
                    fields: vec![ProgramEditorField {
                        id: "fx.status".into(),
                        label: "STATUS".into(),
                        detail: "Chain is empty".into(),
                        value: ProgramEditorValue::Choice("none".into()),
                        kind: ProgramEditorFieldKind::Choice {
                            options: vec![rackforge_program_api::ProgramEditorChoice {
                                value: "none".into(),
                                label: "NO FX".into(),
                                detail: None,
                            }],
                        },
                        live_preview: false,
                    }],
                },
                test_page(
                    "output",
                    "OUTPUT",
                    "Final program gain",
                    vec![test_number("program.gain", "GAIN", Some(100), 2)],
                ),
            ],
        }
    }

    fn draft(draft_id: u64, preview_sound_id: &str) -> ProgramDraftState {
        ProgramDraftState {
            draft_id,
            instance_id: InstanceId::new("live.main.instrument.1").unwrap(),
            original_program_id: None,
            name: "CUSTOM 001".into(),
            preview_sound_id: preview_sound_id.into(),
            storage_path: "custom/user.custom-001.rackforge-program.json".into(),
            artifacts: Vec::new(),
            document_json: r#"{
                "payload": {
                    "layers": [{
                        "id": "a",
                        "enabled": true,
                        "source": {"resource_id":"dls-bank","bank":0,"program":0},
                        "key_range": {"low":0,"high":127},
                        "velocity_range": {"low":0,"high":127},
                        "parameters": {
                            "gain":1.0,
                            "transpose_semitones":0,
                            "fine_tune_cents":0.0,
                            "pitch_bend_range_semitones":2.0,
                            "modulation_depth":1.0,
                            "amplitude_envelope": {
                                "attack_seconds":null,"decay_seconds":null,
                                "sustain_level":null,"release_seconds":null
                            },
                            "pitch_envelope": {
                                "attack_seconds":null,"decay_seconds":null,
                                "sustain_level":null,"release_seconds":null,
                                "depth_cents":null
                            },
                            "lfo": {
                                "enabled":null,"frequency_hz":null,"delay_seconds":null,
                                "pitch_depth_cents":null,
                                "mod_wheel_pitch_depth_cents":null,
                                "attenuation_depth_centibels":null,
                                "mod_wheel_attenuation_depth_centibels":null
                            }
                        }
                    }]
                }
            }"#
            .into(),
            editor: test_editor(false),
            dirty: false,
        }
    }

    fn draft_with_layer_b(draft_id: u64, enabled: bool) -> ProgramDraftState {
        let mut draft = draft(draft_id, "dls.b00000000.p00000000");
        let mut document = serde_json::from_str::<serde_json::Value>(&draft.document_json).unwrap();
        let layers = document
            .pointer_mut("/payload/layers")
            .and_then(serde_json::Value::as_array_mut)
            .unwrap();
        let mut layer_b = layers[0].clone();
        layer_b["id"] = serde_json::Value::from("b");
        layer_b["enabled"] = serde_json::Value::from(enabled);
        layers.push(layer_b);
        draft.document_json = serde_json::to_string(&document).unwrap();
        draft.editor = test_editor(enabled);
        draft
    }

    fn open_new_program_editor(menu: &mut Menu) {
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("ADD NEW"));
        menu.apply(Action::Select);
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::BeginProgramEdit { program_id: None })
        ));
        menu.sync_program_edit(Some(draft(17, "dls.b00000000.p00000000")), Some(7));
    }

    #[test]
    fn every_demo_frame_fits_the_stock_oled() {
        for screen in demo_frames() {
            assert!(screen.is_valid(), "{screen:?}");
        }
    }

    #[test]
    fn home_navigation_wraps_and_opens_the_selected_mode() {
        let mut menu = plugin_menu();
        menu.apply(Action::Previous);
        assert_eq!(menu.render().header, Header::Visible(HOME_HEADER.into()));
        assert_eq!(menu.render().line_2, "    [ CONFIG ]    ");

        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("CONFIG         1/6".into())
        );
    }

    #[test]
    fn back_preserves_each_pages_selection() {
        let mut menu = plugin_menu();
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("SONG"));
        assert_eq!(menu.render().line_2.trim(), "Songs and parts");
        assert!(!menu.render().line_1.contains(" v"));
        menu.apply(Action::Back);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("SONG"));
    }

    #[test]
    fn envelope_is_owned_by_the_active_plugin() {
        let mut menu = plugin_menu();
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLUGINS        1/1".into())
        );
        assert!(menu.render().line_1.contains("RF-DLS"));
        assert_eq!(menu.render().line_2.trim(), "Plugin settings");
        assert!(!menu.render().line_1.contains(" v"));

        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("ADD NEW"));
        menu.apply(Action::Select);
        let _ = menu.take_command();
        menu.sync_program_edit(Some(draft(17, "dls.b00000000.p00000000")), Some(7));
        assert!(menu.render().line_1.contains("NAME"));
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("AMP ENV"));
        assert_eq!(menu.render().line_2.trim(), "Amplitude ADSR");
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("AMP ENV        1/4".into())
        );
        assert!(menu.render().line_1.contains("ATTACK"));
        assert_eq!(menu.render().line_2.trim(), "INHERIT");

        menu.apply(Action::Back);
        assert!(menu.render().line_1.contains("AMP ENV"));
        menu.apply(Action::Back);
        assert!(menu.render().line_1.contains("LAYER A"));
        menu.apply(Action::Back);
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::CancelProgramEdit { draft_id: 17 })
        ));
        menu.sync_program_edit(None, None);
        assert_plugin_header(&menu.render(), "RF-DLS", "CUSTOM 1/1");
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLUGINS        1/1".into())
        );
    }

    #[test]
    fn exposes_exactly_seven_physical_inputs() {
        assert_eq!(Input::PHYSICAL.len(), 7);
        assert_eq!(Input::ALL.len(), 14);
        assert_eq!(Input::Button1.default_navigation(), Some(Action::Select));
        assert_eq!(Input::Button2.default_navigation(), Some(Action::Previous));
        assert_eq!(Input::Button3.default_navigation(), Some(Action::Next));
        assert_eq!(Input::Button4.default_navigation(), Some(Action::Back));
        assert_eq!(
            Input::EncoderLeft.default_navigation(),
            Some(Action::Previous)
        );
        assert_eq!(Input::EncoderRight.default_navigation(), Some(Action::Next));
        assert_eq!(
            Input::EncoderPress.default_navigation(),
            Some(Action::Select)
        );
        assert_eq!(Input::Button4Long.default_navigation(), None);
    }

    #[test]
    fn encoder_can_navigate_and_open_a_mode() {
        let mut menu = plugin_menu();
        menu.apply_input(Input::EncoderRight);
        assert_eq!(menu.render().line_1, "  LIVE    [ PLAY ]");
        menu.apply_input(Input::EncoderPress);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLAY           1/1".into())
        );
    }

    #[test]
    fn plugin_play_and_config_are_distinct_plugin_sections() {
        let mut menu = plugin_menu();
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLAY           1/1".into())
        );
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "PLAY 1/2");
        assert_eq!(menu.render().line_1.trim(), "PRESETS");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1.trim(), "PROGRAMS");
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "PROG 1/1");
        assert!(menu.render().line_1.contains("[PIANO 1]"));
        assert_eq!(menu.render().line_2.trim(), "B000 P000");
        menu.apply(Action::Back);
        assert_plugin_header(&menu.render(), "RF-DLS", "PLAY 2/2");
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLAY           1/1".into())
        );

        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "CUSTOM 1/1");
        assert!(menu.render().line_1.contains("ADD NEW"));
    }

    #[test]
    fn plugin_play_opens_host_presets_before_native_programs() {
        let mut menu = plugin_menu();
        menu.set_plugin_presets(
            vec![
                PlayPreset::new("preset.stage", "Stage Piano", "v1.0.0"),
                PlayPreset::new("preset.soft", "Soft Piano", "v1.0.0"),
            ],
            Some("preset.soft"),
        );
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "PRESETS");
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "PRESET 3/3");
        assert!(menu.render().line_1.to_ascii_uppercase().contains("SOFT"));

        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::LoadPluginPreset {
                preset_id: "preset.stage".into(),
            })
        );
        menu.complete_plugin_preset_load("preset.stage");
        assert!(menu.render().line_1.to_ascii_uppercase().contains("STAGE"));
    }

    #[test]
    fn plugin_presets_can_capture_the_current_state_even_when_the_catalog_is_empty() {
        let mut menu = plugin_menu();
        menu.page = Page::PluginPresets;

        let screen = menu.render();
        assert_plugin_header(&screen, "RF-DLS", "PRESET 1/1");
        assert_eq!(screen.line_1.trim(), "SAVE CURRENT");

        menu.apply(Action::Select);
        assert_eq!(menu.page, Page::PluginPresetName);
        assert!(menu.plugin_preset_name.is_editing());
        assert_eq!(menu.plugin_preset_name.value(), "PIANO 1");

        menu.apply(Action::Select);
        assert_eq!(menu.page, Page::PluginPresetBusy);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SavePluginPreset {
                name: "PIANO 1".into(),
            })
        );
    }

    #[test]
    fn completing_a_plugin_preset_save_focuses_the_canonical_saved_preset() {
        let mut menu = plugin_menu();
        menu.page = Page::PluginPresetBusy;
        let saved = PlayPreset::new("preset.new", "Warm Keys", "v1.2.3");
        menu.complete_plugin_preset_save(Ok((saved.clone(), vec![saved])));

        assert_eq!(menu.page, Page::PluginPresetResult);
        assert_eq!(menu.plugin_active_preset_id.as_deref(), Some("preset.new"));
        assert_eq!(menu.plugin_preset_index, 1);
        assert_eq!(menu.render().line_1.trim(), "SUCCESS");

        menu.apply(Action::Select);
        assert_eq!(menu.page, Page::PluginPresets);
        assert!(
            menu.render()
                .line_1
                .to_ascii_uppercase()
                .contains("[WARM KEYS]")
        );
    }

    #[test]
    fn cancelling_a_plugin_preset_name_does_not_emit_a_command() {
        let mut menu = plugin_menu();
        menu.page = Page::PluginPresets;
        menu.apply(Action::Select);
        menu.apply(Action::Back);

        assert_eq!(menu.page, Page::PluginPresets);
        assert_eq!(menu.take_command(), None);
    }

    #[test]
    fn plugin_play_uses_the_runtime_catalog_and_emits_selection() {
        let mut menu = plugin_menu();
        menu.set_play_sounds(
            vec![
                PlaySound::new("sound.piano", "Piano 1", "dls", "B000 P000"),
                PlaySound::new("sound.strings", "Stríngs 1", "dls", "B000 P048"),
            ],
            Some("sound.piano"),
        );
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("STR?NGS 1"));
        assert!(!menu.render().line_1.contains('['));
        assert_eq!(menu.render().line_2.trim(), "B000 P048");
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SelectSound {
                id: "sound.strings".into()
            })
        );
        assert_eq!(menu.take_command(), None);

        menu.set_play_sounds(
            vec![
                PlaySound::new("sound.piano", "Piano 1", "dls", "B000 P000"),
                PlaySound::new("sound.strings", "Strings 1", "dls", "B000 P048"),
            ],
            Some("sound.piano"),
        );
        assert!(menu.render().line_1.contains("STRINGS 1"));
        assert!(!menu.render().line_1.contains('['));

        menu.set_play_sounds(
            vec![
                PlaySound::new("sound.piano", "Piano 1", "dls", "B000 P000"),
                PlaySound::new("sound.strings", "Strings 1", "dls", "B000 P048"),
            ],
            Some("sound.strings"),
        );
        assert!(menu.render().line_1.contains("[STRINGS 1]"));
    }

    #[test]
    fn plugin_keeps_dls_and_custom_in_separate_collections() {
        let mut menu = plugin_menu();
        menu.set_play_sounds(
            vec![
                PlaySound::new("dls.piano", "Piano 1", "dls", "B000 P000"),
                PlaySound::new(
                    "custom.user.warm-piano",
                    "Warm Piano",
                    "custom",
                    "CUSTOM 001",
                )
                .editable(true),
            ],
            Some("dls.piano"),
        );
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("DLS"));
        assert_eq!(menu.render().line_2.trim(), "1 PROGRAMS");
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("CUSTOM"));
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "PROG 1/1");
        assert!(menu.render().line_1.contains("WARM PIANO"));
        assert_eq!(menu.render().line_2.trim(), "CUSTOM 001");
    }

    #[test]
    fn live_remains_a_rackforge_mode_above_plugins() {
        let mut menu = plugin_menu();
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("LIVE           1/3".into())
        );
        assert!(!menu.render().header.eq(&Header::Visible("RF-DLS".into())));
    }

    #[test]
    fn simple_carousel_shows_only_the_current_option_and_its_detail() {
        let mut menu = plugin_menu();
        menu.apply(Action::Select);
        let screen = menu.apply_input_and_render(Input::Button3);
        assert!(screen.line_1.contains("SONG"));
        assert!(!screen.line_1.contains("RACK"));
        assert!(!screen.line_1.contains('['));
        assert!(!screen.line_1.contains(']'));
        assert_eq!(screen.line_2.trim(), "Songs and parts");
    }

    #[test]
    fn envelope_value_editor_commits_or_cancels_before_back_exits() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);

        assert!(menu.render().line_1.contains("ATTACK"));
        menu.apply_input(Input::Button1);
        assert!(
            menu.editor_field
                .as_ref()
                .is_some_and(ValueCarousel::is_editing)
        );
        assert_eq!(menu.render().header, Header::Visible("ATTACK".into()));
        assert!(menu.render().line_1.starts_with('['));
        assert!(menu.render().line_2.trim().is_empty());
        menu.apply_input(Input::EncoderRight);
        assert_eq!(
            menu.render().line_1.trim_matches(&['[', ']', ' '][..]),
            "0.01 s"
        );
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.a.amp.attack".into(),
                value: ProgramEditorValue::Integer(1),
                preview: true,
            })
        );
        menu.apply_input(Input::Button4);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::RestoreProgramDraftPreview { draft_id: 17 })
        );
        assert!(menu.render().line_1.contains("ATTACK"));
        assert_eq!(menu.render().line_2.trim(), "INHERIT");
        assert!(menu.editor_field.is_none());

        menu.apply_input(Input::Button1);
        menu.apply_input(Input::EncoderRight);
        menu.apply_input(Input::Button1);
        assert!(menu.render().line_1.contains("ATTACK"));
        assert!(menu.editor_field.is_none());
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.a.amp.attack".into(),
                value: ProgramEditorValue::Integer(1),
                preview: false,
            })
        );

        menu.apply_input(Input::Button4);
        assert!(menu.render().line_1.contains("AMP ENV"));
    }

    #[test]
    fn timbre_change_targets_the_core_owned_program_draft() {
        let mut menu = plugin_menu();
        menu.set_play_sounds(
            vec![
                PlaySound::new("dls.b00000000.p00000000", "Piano 1", "dls", "B000 P000"),
                PlaySound::new("dls.b00000000.p00000030", "Strings 1", "dls", "B000 P048"),
            ],
            Some("dls.b00000000.p00000000"),
        );
        open_new_program_editor(&mut menu);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("STRINGS 1"));
        menu.apply(Action::Select);

        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.a.sound".into(),
                value: ProgramEditorValue::SoundId("dls.b00000000.p00000030".into()),
                preview: false,
            })
        );
    }

    #[test]
    fn layer_b_must_be_enabled_before_choosing_its_timbre() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("LAYER B"));
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("ENABLED"));
        assert_eq!(menu.render().line_2.trim(), "OFF");
        assert_eq!(menu.take_command(), None);

        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.b.enabled".into(),
                value: ProgramEditorValue::Boolean(true),
                preview: false,
            })
        );
        menu.sync_program_edit(Some(draft_with_layer_b(17, true)), Some(7));
        assert!(menu.render().line_1.contains("ENABLED"));
        assert_eq!(menu.render().line_2.trim(), "ON");

        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("TIMBRE"));
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("TIMBRE         1/1".into())
        );
    }

    #[test]
    fn layer_b_can_be_disabled_without_removing_its_configuration() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.sync_program_edit(Some(draft_with_layer_b(17, true)), Some(7));
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);

        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.b.enabled".into(),
                value: ProgramEditorValue::Boolean(false),
                preview: false,
            })
        );
    }

    #[test]
    fn layer_a_owns_all_synthesis_sections() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.apply(Action::Next);
        menu.apply(Action::Select);

        let expected = [
            "TIMBRE",
            "AMP ENV",
            "PITCH ENV",
            "LFO",
            "TUNING",
            "RANGE",
            "VOLUME",
        ];
        for (index, label) in expected.into_iter().enumerate() {
            assert!(menu.render().line_1.contains(label));
            if index + 1 < expected.len() {
                menu.apply(Action::Next);
            }
        }
    }

    #[test]
    fn layer_volume_is_independent_and_previewable() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        for _ in 0..6 {
            menu.apply(Action::Next);
        }
        assert!(menu.render().line_1.contains("VOLUME"));
        assert_eq!(menu.render().line_2.trim(), "Layer mix gain");

        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Visible("VOLUME".into()));
        assert_eq!(menu.render().line_1.trim(), "1.00");
        assert!(menu.render().line_2.trim().is_empty());
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button2);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.a.gain".into(),
                value: ProgramEditorValue::Integer(99),
                preview: true,
            })
        );
    }

    #[test]
    fn lfo_delay_override_targets_the_focused_layer() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        for _ in 0..3 {
            menu.apply(Action::Next);
        }
        assert!(menu.render().line_1.contains("LFO"));
        menu.apply(Action::Select);
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button3);
        assert!(menu.render().line_1.contains("DELAY"));
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "layer.a.lfo.delay".into(),
                value: ProgramEditorValue::Integer(1),
                preview: false,
            })
        );
    }

    #[test]
    fn save_is_an_explicit_program_section() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        for _ in 0..5 {
            menu.apply(Action::Next);
        }
        assert!(menu.render().line_1.contains("SAVE"));
        assert_eq!(menu.render().line_2.trim(), "Store program");
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SaveProgramDraft { draft_id: 17 })
        );
    }

    #[test]
    fn shared_fx_precedes_the_editable_program_output() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        for _ in 0..3 {
            menu.apply(Action::Next);
        }
        assert!(menu.render().line_1.contains("FX"));
        assert_eq!(menu.render().line_2.trim(), "Shared FX chain");
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("FX             1/1".into())
        );
        assert_eq!(menu.render().line_1.trim(), "STATUS");
        assert_eq!(menu.render().line_2.trim(), "NO FX");
        menu.apply(Action::Back);

        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("OUTPUT"));
        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Visible("GAIN".into()));
        assert_eq!(menu.render().line_1.trim(), "1.00");
        assert!(menu.render().line_2.trim().is_empty());
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button2);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "program.gain".into(),
                value: ProgramEditorValue::Integer(99),
                preview: true,
            })
        );
        menu.apply_input(Input::Button1);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::EditProgramDraftField {
                draft_id: 17,
                field_id: "program.gain".into(),
                value: ProgramEditorValue::Integer(99),
                preview: false,
            })
        );
    }

    #[test]
    fn dirty_program_requires_save_or_discard_before_leaving() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.program_draft.as_mut().unwrap().dirty = true;

        menu.apply(Action::Back);
        assert!(menu.render().line_1.contains("SAVE CHANGES?"));
        assert!(menu.render().line_2.starts_with('['));
        assert!(menu.render().line_2.contains("SAVE"));
        assert_eq!(menu.take_command(), None);

        menu.apply(Action::Back);
        assert!(menu.render().line_1.contains("NAME"));

        menu.apply(Action::Back);
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::ResolveProgramExit {
                draft_id: 17,
                decision: ProgramExitDecision::Save,
                destination: ProgramExitDestination::CustomPrograms,
            })
        );
    }

    #[test]
    fn dirty_program_can_be_explicitly_discarded() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.program_draft.as_mut().unwrap().dirty = true;

        menu.apply(Action::Back);
        menu.apply(Action::Next);
        assert!(menu.render().line_2.starts_with('['));
        assert!(menu.render().line_2.contains("DISCARD"));
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::ResolveProgramExit {
                draft_id: 17,
                decision: ProgramExitDecision::Discard,
                destination: ProgramExitDestination::CustomPrograms,
            })
        );
    }

    #[test]
    fn long_back_also_protects_a_dirty_program_before_returning_to_play() {
        let mut menu = Menu {
            active_mode: ActiveMode::Play,
            ..plugin_menu()
        };
        open_new_program_editor(&mut menu);
        menu.program_draft.as_mut().unwrap().dirty = true;

        menu.apply_input(Input::Button4Long);
        assert!(menu.render().line_1.contains("SAVE CHANGES?"));
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::ResolveProgramExit {
                draft_id: 17,
                decision: ProgramExitDecision::Discard,
                destination: ProgramExitDestination::ActiveMode {
                    mode: ActiveMode::Play,
                    selected_sound_id: Some("dls.b00000000.p00000000".into()),
                },
            })
        );
    }

    #[test]
    fn custom_program_name_is_edited_as_part_of_the_core_draft() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        assert!(menu.render().line_1.contains("NAME"));
        menu.apply(Action::Select);
        assert_plugin_header(&menu.render(), "RF-DLS", "NAME");
        menu.apply_input(Input::Button1);
        assert!(menu.program_name.is_editing());
        menu.apply_input(Input::EncoderRight);
        menu.apply_input(Input::Button1);

        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetProgramDraftName {
                draft_id: 17,
                name: "CUSTOM 002".into(),
            })
        );
    }

    #[test]
    fn long_back_returns_to_the_active_play_plugin_and_centers_selection() {
        let mut menu = plugin_menu();
        menu.set_play_sounds(
            vec![
                PlaySound::new("dls.piano", "Piano", "dls", "B000 P000"),
                PlaySound::new(
                    "custom.user.warm-piano",
                    "Warm Piano",
                    "custom",
                    "CUSTOM 001",
                )
                .editable(true),
            ],
            Some("custom.user.warm-piano"),
        );
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("RACKS"));

        menu.apply_input(Input::Button4Long);
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::ReturnToActiveMode {
                mode: ActiveMode::Play,
                ..
            })
        ));
        menu.complete_return_to_active_mode(ActiveMode::Play, Some("custom.user.warm-piano"));
        assert_plugin_header(&menu.render(), "RF-DLS", "PROG 1/1");
        assert!(menu.render().line_1.contains("[WARM PIANO]"));
    }

    #[test]
    fn restored_mode_controls_long_back_after_a_fresh_menu_start() {
        for mode in [ActiveMode::Live, ActiveMode::Play] {
            let mut menu = plugin_menu();
            menu.sync_active_mode(mode);

            menu.apply_input(Input::Button4Long);

            assert!(matches!(
                menu.take_command(),
                Some(MenuCommand::ReturnToActiveMode {
                    mode: returned_mode,
                    ..
                }) if returned_mode == mode
            ));
        }
    }

    #[test]
    fn choosing_play_publishes_the_new_active_mode() {
        let mut menu = plugin_menu();
        menu.apply(Action::Next);
        menu.apply(Action::Select);

        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetActiveMode {
                mode: ActiveMode::Play
            })
        );
    }

    #[test]
    fn audition_preview_never_replaces_the_play_navigation_anchor() {
        let mut menu = plugin_menu();
        let sounds = vec![
            PlaySound::new("dls.b00000000.p00000000", "Piano", "dls", "B000 P000"),
            PlaySound::new(
                "custom.user.warm-piano",
                "Warm Piano",
                "custom",
                "CUSTOM 001",
            )
            .editable(true),
        ];
        menu.set_play_sounds(sounds.clone(), Some("custom.user.warm-piano"));
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.sync_program_edit(Some(draft(17, "dls.b00000000.p00000000")), Some(7));
        menu.set_play_sounds(sounds, Some("dls.b00000000.p00000000"));
        menu.apply_input(Input::Button4Long);

        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::ReturnToActiveMode {
                mode: ActiveMode::Play,
                cancel_draft_id: Some(17),
                selected_sound_id: Some(id),
            }) if id == "custom.user.warm-piano"
        ));
    }

    #[test]
    fn home_chord_is_an_immediate_host_owned_escape() {
        let mut menu = plugin_menu();
        open_new_program_editor(&mut menu);
        menu.apply_input(Input::HomeChord);
        assert_eq!(menu.render().header, Header::Visible(HOME_HEADER.into()));
        assert!(matches!(menu.take_command(), Some(MenuCommand::ForceHome)));
        assert_eq!(menu.active_mode, ActiveMode::Idle);
        assert!(menu.program_draft.is_none());
        assert!(menu.audition_lease_id.is_none());
    }

    #[test]
    fn native_footer_labels_are_present_on_every_page() {
        let mut menu = plugin_menu();
        for action in [
            None,
            Some(Action::Select),
            Some(Action::Back),
            Some(Action::Next),
            Some(Action::Select),
            Some(Action::Back),
            Some(Action::Next),
            Some(Action::Select),
        ] {
            if let Some(action) = action {
                menu.apply(action);
            }
            assert_eq!(menu.render().footer, standard_footer(None));
        }
    }

    #[test]
    fn contextual_button_is_highlighted_only_while_pressed() {
        let mut menu = plugin_menu();
        assert!(menu.set_button_pressed(Input::Button1, true));
        assert_eq!(menu.render().footer[0].state, VisualState::Pressed);
        assert_eq!(menu.render().footer[1].state, VisualState::Normal);

        assert!(menu.set_button_pressed(Input::Button1, false));
        assert!(
            menu.render()
                .footer
                .iter()
                .all(|button| button.state == VisualState::Normal)
        );
        assert!(!menu.set_button_pressed(Input::EncoderPress, true));
    }

    #[test]
    fn system_web_settings_are_navigable_and_reflect_host_state() {
        let mut menu = plugin_menu();
        menu.sync_web_settings(WebSystemSettings {
            enabled: true,
            access: WebAccess::Local,
            port: 8787,
            lan_ip: Some([192, 168, 1, 17]),
            service_online: true,
            pin_set: false,
        });

        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "RACKS");
        menu.apply(Action::Previous);
        assert_eq!(menu.render().line_1.trim(), "SYSTEM");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "WEB INTERFACE");
        menu.apply(Action::Select);

        let enabled = menu.render();
        assert!(matches!(
            enabled.header,
            Header::Visible(header)
                if header.starts_with("WEB") && header.ends_with("1/6")
        ));
        assert_eq!(enabled.line_1, "ENABLED");
        assert_eq!(enabled.line_2, "ON");

        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        assert_eq!(menu.render().line_2, "[OFF]");
        menu.apply_input(Input::Button4);
        assert_eq!(menu.render().line_2, "ON");

        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "ACCESS");
        assert_eq!(menu.render().line_2, "LOCAL ONLY");
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        assert_eq!(menu.render().line_2, "[LAN]");
        menu.apply_input(Input::Button1);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetWebAccess {
                access: WebAccess::Lan
            })
        );
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "ADDRESS");
        assert_eq!(menu.render().line_2, "192.168.1.17:8787");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "PORT");
        assert_eq!(menu.render().line_2, "8787");
        menu.apply(Action::Next);
        // The controller reports whether the device has been claimed and
        // offers no way to change it. A PIN shown on this display would put a
        // screen back in the path of getting in, which is what taking pairing
        // off it was for.
        assert_eq!(menu.render().line_1, "ACCESS PIN");
        assert_eq!(menu.render().line_2, "NOT SET");

        menu.sync_web_settings(WebSystemSettings {
            enabled: true,
            access: WebAccess::Lan,
            port: 8787,
            lan_ip: Some([192, 168, 1, 17]),
            service_online: true,
            pin_set: true,
        });
        assert_eq!(menu.render().line_2, "SET");
        menu.apply_input(Input::Button1);
        assert_eq!(menu.take_command(), None, "the display cannot set a PIN");

        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "STATUS");
        assert_eq!(menu.render().line_2, "ONLINE");

        menu.apply(Action::Back);
        assert_eq!(menu.render().line_1.trim(), "WEB INTERFACE");
    }

    #[test]
    fn wifi_menu_groups_known_networks_and_exposes_profile_actions() {
        let mut menu = plugin_menu();
        menu.sync_wifi_settings(WifiSystemSettings {
            available: true,
            enabled: true,
            connected: true,
            ssid: Some("PHONE HOTSPOT".into()),
            signal_percent: Some(82),
            saved_networks: vec![
                SavedWifiNetwork {
                    id: "wifi-phone".into(),
                    name: "Phone".into(),
                    ssid: Some("PHONE HOTSPOT".into()),
                    active: true,
                },
                SavedWifiNetwork {
                    id: "wifi-home".into(),
                    name: "Home".into(),
                    ssid: Some("HOME".into()),
                    active: false,
                },
            ],
        });

        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "WEB INTERFACE");
        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "WI-FI");
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1, "STATUS");
        assert_eq!(menu.render().line_2, "PHONE HOTSPOT 82%");

        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_1, "KNOWN");
        assert_eq!(menu.render().line_2, "2 SAVED");
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_1, "PHONE HOTSPOT");
        assert_eq!(menu.render().line_2, "CONNECTED");
        menu.apply_input(Input::Button3);
        assert_eq!(menu.render().line_1, "HOME");
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_1, "CONNECT");
        menu.apply_input(Input::Button1);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::ActivateSavedWifi {
                connection_id: "wifi-home".into()
            })
        );

        menu.apply_input(Input::Button4);
        menu.apply_input(Input::Button4);
        menu.apply_input(Input::Button4);
        menu.apply_input(Input::Button3);
        assert_eq!(menu.render().line_1, "RADIO");
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        assert_eq!(menu.render().line_2, "[OFF]");
        menu.apply_input(Input::Button1);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetWifiEnabled { enabled: false })
        );
    }

    #[test]
    fn discovered_secured_network_requests_a_redacted_password() {
        let mut menu = plugin_menu();
        menu.sync_wifi_settings(WifiSystemSettings {
            available: true,
            enabled: true,
            connected: false,
            ssid: None,
            signal_percent: None,
            saved_networks: vec![SavedWifiNetwork {
                id: "known".into(),
                name: "Known".into(),
                ssid: Some("KNOWN".into()),
                active: false,
            }],
        });

        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);
        assert_eq!(menu.take_command(), Some(MenuCommand::ScanWifi));

        menu.sync_discovered_wifi(vec![
            DiscoveredWifiNetwork {
                ssid: "KNOWN".into(),
                signal_percent: 99,
                secured: true,
            },
            DiscoveredWifiNetwork {
                ssid: "PHONE NEW".into(),
                signal_percent: 75,
                secured: true,
            },
        ]);
        assert_eq!(menu.render().line_1, "PHONE NEW");
        assert_eq!(menu.render().line_2, "75% SECURED");
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_1, "CONNECT");
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_1.trim(), "PASSWORD");
        assert_eq!(menu.render().line_2.trim(), "PRESS OK");

        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3Long);
        assert!(menu.render().line_2.contains("*[A]"));
        menu.apply_input(Input::Button1);
        let Some(MenuCommand::ConnectDiscoveredWifi {
            ssid,
            passphrase: Some(passphrase),
        }) = menu.take_command()
        else {
            panic!("expected discovered Wi-Fi connection command");
        };
        assert_eq!(ssid, "PHONE NEW");
        assert_eq!(passphrase.expose(), "AA");
        assert!(!format!("{passphrase:?}").contains("AA"));
    }

    #[test]
    fn compositor_emits_one_full_snapshot_then_only_changed_regions() {
        let mut compositor = ScreenCompositor::default();
        let first = Menu::default().render();
        assert!(compositor.publish(first.clone(), SurfaceUpdatePriority::Interactive));
        let initial = compositor.take().unwrap();
        assert_eq!(initial.revision, 1);
        assert_eq!(initial.changed, ScreenRegions::full());
        assert!(!compositor.publish(first.clone(), SurfaceUpdatePriority::Background));
        assert!(compositor.take().is_none());

        let mut header = first;
        header.header = Header::Visible("MASTER 75%".into());
        assert!(compositor.publish(header.clone(), SurfaceUpdatePriority::Immediate));
        let update = compositor.take().unwrap();
        assert_eq!(update.screen, header);
        assert_eq!(update.changed, ScreenRegions::header());
        assert_eq!(update.priority, SurfaceUpdatePriority::Immediate);
    }

    #[test]
    fn compositor_coalesces_to_latest_screen_and_preserves_highest_priority() {
        let mut compositor = ScreenCompositor::default();
        let first = Menu::default().render();
        compositor.publish(first.clone(), SurfaceUpdatePriority::Interactive);
        compositor.take().unwrap();

        let mut intermediate = first.clone();
        intermediate.line_1 = "INTERMEDIATE".into();
        compositor.publish(intermediate, SurfaceUpdatePriority::Immediate);
        let mut latest = first;
        latest.line_1 = "LATEST".into();
        compositor.publish(latest.clone(), SurfaceUpdatePriority::Background);

        let update = compositor.take().unwrap();
        assert_eq!(update.revision, 3);
        assert_eq!(update.screen, latest);
        assert!(update.changed.body);
        assert_eq!(update.priority, SurfaceUpdatePriority::Immediate);
        assert!(compositor.take().is_none());
    }

    #[test]
    fn compositor_promotes_a_duplicate_pending_screen_without_new_revision() {
        let mut compositor = ScreenCompositor::default();
        let screen = Menu::default().render();
        assert!(compositor.publish(screen.clone(), SurfaceUpdatePriority::Background));
        assert!(!compositor.publish(screen, SurfaceUpdatePriority::Immediate));

        let update = compositor.take().unwrap();
        assert_eq!(update.revision, 1);
        assert_eq!(update.priority, SurfaceUpdatePriority::Immediate);
    }

    #[test]
    fn compositor_reconnect_resends_one_authoritative_full_snapshot() {
        let mut compositor = ScreenCompositor::default();
        let screen = Menu::default().render();
        compositor.publish(screen.clone(), SurfaceUpdatePriority::Interactive);
        compositor.take().unwrap();

        compositor.invalidate_delivery();
        let restored = compositor.take().unwrap();
        assert_eq!(restored.screen, screen);
        assert_eq!(restored.changed, ScreenRegions::full());
        assert_eq!(restored.priority, SurfaceUpdatePriority::Immediate);
        assert!(compositor.take().is_none());
    }
}
