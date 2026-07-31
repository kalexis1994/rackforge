//! RackForge-owned LITTLE surface state and plugin navigation.
//!
//! This crate has no MIDI, SysEx, USB or controller-model knowledge.

use rackforge_audio_api::{
    AudioDeviceDescriptor, AudioDeviceSelector, AudioFallbackPolicy, AudioOutputProfile,
    AudioOutputState, AudioSampleFormat,
};
use rackforge_controller_api::LITTLE_TEXT_COLUMNS;
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

pub const DISPLAY_COLUMNS: usize = LITTLE_TEXT_COLUMNS;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Header {
    Visible(String),
    Hidden,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaySound {
    pub id: String,
    pub name: String,
    pub bank: String,
    pub detail: String,
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
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveMode {
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
    pub pairing_available: bool,
}

impl Default for WebSystemSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            access: WebAccess::Local,
            port: 8787,
            lan_ip: None,
            service_online: false,
            pairing_available: false,
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
    SelectSound {
        id: String,
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
    ForceHome {
        cancel_draft_id: Option<u64>,
    },
    SetWebEnabled {
        enabled: bool,
    },
    SetWebAccess {
        access: WebAccess,
    },
    SetWebPort {
        port: u16,
    },
    BeginWebPairing,
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

    fn fullscreen(line_1: impl Into<String>, line_2: impl Into<String>) -> Self {
        let screen = Self {
            header: Header::Hidden,
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
    Play,
    Config,
    Plugins,
    System,
    Audio,
    AudioOutput,
    AudioRate,
    AudioLatency,
    AudioBusy,
    AudioResult,
    SystemWeb,
    SystemWebPairing,
    SystemWifi,
    SystemWifiNetworks,
    SystemWifiKnown,
    SystemWifiKnownActions,
    SystemWifiDiscovered,
    SystemWifiDiscoveredActions,
    SystemWifiPassword,
    SystemWifiBusy,
    SystemWifiResult,
    RfDlsLibrary,
    RfDlsPlay,
    RfDlsCustomPrograms,
    RfDlsProgramSections,
    RfDlsName,
    RfDlsLayerMenu,
    RfDlsTimbre,
    RfDlsEnvelope,
    RfDlsPitchEnvelope,
    RfDlsRange,
    RfDlsLfo,
    RfDlsTuning,
    RfDlsLayerLevel,
    RfDlsSharedFx,
    RfDlsProgramOutput,
    RfDlsUnsavedChanges,
    ProgramEditorRoot,
    ProgramEditorPage,
    ProgramEditorField,
    ProgramEditorSound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingProgramExit {
    return_page: Page,
    destination: ProgramExitDestination,
}

#[derive(Debug)]
pub struct Menu {
    page: Page,
    active_mode: ActiveMode,
    home_index: usize,
    live_index: usize,
    play_index: usize,
    config_index: usize,
    plugin_index: usize,
    system_index: usize,
    system_web_index: usize,
    web_settings: WebSystemSettings,
    web_edit_candidate: WebSystemSettings,
    system_web_editing: bool,
    pairing_code: Option<String>,
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
    rf_dls_library_index: usize,
    rf_dls_play_index: usize,
    rf_dls_custom_index: usize,
    rf_dls_section_index: usize,
    rf_dls_layer_index: usize,
    rf_dls_layer_option_index: usize,
    rf_dls_timbre_index: usize,
    rf_dls_sounds: Vec<PlaySound>,
    rf_dls_active_sound_id: Option<String>,
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
const HOME_HEADER: &str = "RACK FORGE";
const LIVE_ITEMS: [&str; 4] = ["PIANO 1", "WARM PAD", "DLS STRINGS", "M1 HOUSE"];
const LIVE_DETAILS: [&str; 4] = ["DLS piano", "Layered pad", "RF-DLS bank", "Korg M1"];
const PLAY_ITEMS: [&str; 1] = ["RF-DLS"];
const PLAY_DETAILS: [&str; 1] = ["DLS banks"];
const CONFIG_ITEMS: [&str; 4] = ["PLUGINS", "SETLISTS", "AUDIO", "SYSTEM"];
const CONFIG_DETAILS: [&str; 4] = [
    "Plugin settings",
    "Performance order",
    "Output settings",
    "RackForge settings",
];
const PLUGIN_ITEMS: [&str; 1] = ["RF-DLS"];
const PLUGIN_DETAILS: [&str; 1] = [" "];
const SYSTEM_WEB_ITEM: (&str, &str) = ("WEB INTERFACE", "Browser & pairing");
const SYSTEM_WIFI_ITEM: (&str, &str) = ("WI-FI", "Wireless network");
const SYSTEM_WEB_ITEMS: [&str; 6] = [
    "ENABLED",
    "ACCESS",
    "ADDRESS",
    "PORT",
    "PAIR DEVICE",
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
const RF_DLS_LIBRARIES: [&str; 2] = ["DLS", "CUSTOM"];
const RF_DLS_PROGRAM_SECTIONS: [&str; 6] = ["NAME", "LAYER A", "LAYER B", "FX", "OUTPUT", "SAVE"];
const RF_DLS_SECTION_DETAILS: [&str; 6] = [
    "Program name",
    "Required layer",
    "Optional layer",
    "Shared FX chain",
    "Final program gain",
    "Store program",
];
const RF_DLS_LAYER_SECTIONS: [&str; 7] = [
    "TIMBRE",
    "AMP ENV",
    "PITCH ENV",
    "LFO",
    "TUNING",
    "RANGE",
    "VOLUME",
];
const RF_DLS_LAYER_DETAILS: [&str; 7] = [
    "DLS source",
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
            play_index: 0,
            config_index: 0,
            plugin_index: 0,
            system_index: 0,
            system_web_index: 0,
            web_settings: WebSystemSettings::default(),
            web_edit_candidate: WebSystemSettings::default(),
            system_web_editing: false,
            pairing_code: None,
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
            rf_dls_library_index: 0,
            rf_dls_play_index: 0,
            rf_dls_custom_index: 0,
            rf_dls_section_index: 0,
            rf_dls_layer_index: 0,
            rf_dls_layer_option_index: 0,
            rf_dls_timbre_index: 0,
            rf_dls_sounds: vec![PlaySound::new(
                "dls.b00000000.p00000000",
                "PIANO 1",
                "dls",
                "B000 P000",
            )],
            rf_dls_active_sound_id: Some("dls.b00000000.p00000000".into()),
            play_anchor_sound_id: Some("dls.b00000000.p00000000".into()),
            audition_lease_id: None,
            program_draft: None,
            program_name: program_name_editor("CUSTOM 001"),
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
    pub fn sync_active_mode(&mut self, mode: ActiveMode) {
        self.active_mode = mode;
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

    pub fn show_pairing_code(&mut self, code: impl Into<String>) {
        let code = code.into();
        if code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit()) {
            self.pairing_code = Some(code);
            self.page = Page::SystemWebPairing;
        }
    }

    pub fn set_play_sounds(&mut self, sounds: Vec<PlaySound>, selected_sound_id: Option<&str>) {
        let browsed_sound_id = self
            .filtered_sounds()
            .get(self.rf_dls_play_index)
            .map(|sound| sound.id.clone());
        self.rf_dls_sounds = sounds;
        self.rf_dls_active_sound_id = selected_sound_id.map(str::to_owned);
        if self.program_draft.is_none() {
            self.play_anchor_sound_id = selected_sound_id.map(str::to_owned);
        }
        let focus_id = browsed_sound_id.as_deref().or(selected_sound_id);
        if let Some(sound) =
            focus_id.and_then(|id| self.rf_dls_sounds.iter().find(|sound| sound.id == id))
        {
            self.rf_dls_library_index = library_index(&sound.bank).unwrap_or(0);
        }
        self.rf_dls_play_index = focus_id
            .and_then(|id| {
                self.filtered_sounds()
                    .iter()
                    .position(|sound| sound.id == id)
            })
            .unwrap_or(0);
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
        if self.rf_dls_layer_index >= self.program_layer_count()
            && self.page != Page::RfDlsLayerMenu
        {
            self.rf_dls_layer_index = self.program_layer_count().saturating_sub(1);
        }
        self.rf_dls_layer_option_index = self
            .rf_dls_layer_option_index
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
            self.page = Page::RfDlsCustomPrograms;
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

    pub fn apply_input(&mut self, input: Input) {
        if input == Input::HomeChord {
            let cancel_draft_id = self.program_draft.as_ref().map(|draft| draft.draft_id);
            self.audition_lease_id = None;
            self.program_draft = None;
            self.page = Page::Home;
            self.pending_command = Some(MenuCommand::ForceHome { cancel_draft_id });
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
        if self.page == Page::ProgramEditorField {
            self.apply_generic_editor_field_input(input);
        } else if self.page == Page::ProgramEditorSound {
            if let Some(action) = input.default_navigation() {
                self.apply_generic_editor_action(action);
            }
        } else if matches!(self.page, Page::ProgramEditorRoot | Page::ProgramEditorPage) {
            if let Some(action) = input.default_navigation() {
                self.apply_generic_editor_action(action);
            }
        } else if self.page == Page::RfDlsUnsavedChanges {
            self.apply_unsaved_changes_input(input);
        } else if self.page == Page::RfDlsName {
            self.apply_program_name_input(input);
        } else if self.page == Page::RfDlsProgramOutput {
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
        } else if self.page == Page::SystemWebPairing {
            if matches!(input, Input::Button4 | Input::EncoderPress) {
                self.page = Page::SystemWeb;
            }
        } else if self.is_layer_parameter_page() {
            self.apply_layer_parameter_input(input);
        } else if let Some(action) = input.default_navigation() {
            self.apply(action);
        }
    }

    fn begin_program_edit(&mut self) {
        let custom_sounds = self.custom_sounds();
        let program_id = if self.rf_dls_custom_index == 0 {
            None
        } else {
            let Some(program) = custom_sounds.get(self.rf_dls_custom_index - 1) else {
                return;
            };
            Some(program.id.clone())
        };
        self.pending_command = Some(MenuCommand::BeginProgramEdit { program_id });
    }

    pub fn apply_input_and_render(&mut self, input: Input) -> Screen {
        self.apply_input(input);
        self.render()
    }

    pub fn apply(&mut self, action: Action) {
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
            Page::ProgramEditorRoot | Page::ProgramEditorPage | Page::ProgramEditorSound
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
        if self.page == Page::RfDlsUnsavedChanges {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_unsaved_changes_input(input);
            return;
        }
        if self.page == Page::RfDlsName {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_program_name_input(input);
            return;
        }
        if self.page == Page::RfDlsProgramOutput {
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
                    Page::RfDlsPlay => Page::RfDlsLibrary,
                    Page::RfDlsLibrary => Page::Play,
                    Page::RfDlsCustomPrograms => Page::Plugins,
                    Page::RfDlsName | Page::RfDlsSharedFx | Page::RfDlsProgramOutput => {
                        Page::RfDlsProgramSections
                    }
                    Page::RfDlsLayerMenu => Page::RfDlsProgramSections,
                    Page::RfDlsTimbre
                    | Page::RfDlsEnvelope
                    | Page::RfDlsPitchEnvelope
                    | Page::RfDlsRange
                    | Page::RfDlsLfo
                    | Page::RfDlsTuning
                    | Page::RfDlsLayerLevel => Page::RfDlsLayerMenu,
                    Page::RfDlsProgramSections => {
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
                        Page::RfDlsCustomPrograms
                    }
                    Page::Plugins => Page::Config,
                    Page::Audio => Page::Config,
                    Page::AudioOutput | Page::AudioRate | Page::AudioLatency => Page::Audio,
                    Page::AudioBusy | Page::AudioResult => Page::Audio,
                    Page::SystemWeb => Page::System,
                    Page::SystemWebPairing => Page::SystemWeb,
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
                if self.page == Page::RfDlsPlay {
                    if let Some(sound) = self.filtered_sounds().get(self.rf_dls_play_index) {
                        self.pending_command = Some(MenuCommand::SelectSound {
                            id: sound.id.clone(),
                        });
                    }
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
                    Page::Play if self.play_index == 0 => Page::RfDlsLibrary,
                    Page::RfDlsLibrary => {
                        self.rf_dls_play_index = 0;
                        Page::RfDlsPlay
                    }
                    Page::Config if self.config_index == 0 => Page::Plugins,
                    Page::Config if self.config_index == 2 => Page::Audio,
                    Page::Config if self.config_index == 3 => Page::System,
                    Page::Plugins if self.plugin_index == 0 => Page::RfDlsCustomPrograms,
                    Page::System if self.system_index == 0 => Page::SystemWeb,
                    Page::System if self.system_index == 1 && self.wifi_settings.available => {
                        Page::SystemWifi
                    }
                    Page::RfDlsCustomPrograms => {
                        self.begin_program_edit();
                        Page::RfDlsCustomPrograms
                    }
                    Page::RfDlsProgramSections => match self.rf_dls_section_index {
                        0 => Page::RfDlsName,
                        1 | 2 => {
                            self.rf_dls_layer_index = self.rf_dls_section_index - 1;
                            self.rf_dls_layer_option_index = 0;
                            self.sync_layer_editors();
                            Page::RfDlsLayerMenu
                        }
                        3 => Page::RfDlsSharedFx,
                        4 => Page::RfDlsProgramOutput,
                        _ => {
                            if let Some(draft_id) =
                                self.program_draft.as_ref().map(|draft| draft.draft_id)
                            {
                                self.pending_command =
                                    Some(MenuCommand::SaveProgramDraft { draft_id });
                            }
                            Page::RfDlsProgramSections
                        }
                    },
                    Page::RfDlsLayerMenu => {
                        if self.rf_dls_layer_index == 1 && self.rf_dls_layer_option_index == 0 {
                            self.toggle_layer_b();
                            Page::RfDlsLayerMenu
                        } else {
                            let section_index = self.rf_dls_layer_option_index
                                - usize::from(self.rf_dls_layer_index == 1);
                            match section_index {
                                0 => {
                                    if let Some(source_id) =
                                        self.program_layer_source_id(self.rf_dls_layer_index)
                                        && let Some(index) = self
                                            .dls_sounds()
                                            .iter()
                                            .position(|sound| sound.id == source_id)
                                    {
                                        self.rf_dls_timbre_index = index;
                                    }
                                    Page::RfDlsTimbre
                                }
                                1 => Page::RfDlsEnvelope,
                                2 => Page::RfDlsPitchEnvelope,
                                3 => Page::RfDlsLfo,
                                4 => Page::RfDlsTuning,
                                5 => Page::RfDlsRange,
                                _ => Page::RfDlsLayerLevel,
                            }
                        }
                    }
                    Page::RfDlsTimbre => {
                        let sound_id = self
                            .dls_sounds()
                            .get(self.rf_dls_timbre_index)
                            .map(|sound| sound.id.clone());
                        if let Some(sound_id) = sound_id
                            && let Some(draft_id) =
                                self.program_draft.as_ref().map(|draft| draft.draft_id)
                        {
                            self.pending_command = Some(MenuCommand::EditProgramDraftField {
                                draft_id,
                                field_id: format!(
                                    "layer.{}.sound",
                                    if self.rf_dls_layer_index == 0 {
                                        "a"
                                    } else {
                                        "b"
                                    }
                                ),
                                value: ProgramEditorValue::SoundId(sound_id),
                                preview: false,
                            });
                        }
                        Page::RfDlsTimbre
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
            ActiveMode::Live => {
                self.page = Page::Live;
            }
            ActiveMode::Play => {
                let focus_sound_id = focus_sound_id.or(self.play_anchor_sound_id.as_deref());
                if let Some(sound) = focus_sound_id
                    .and_then(|id| self.rf_dls_sounds.iter().find(|sound| sound.id == id))
                {
                    self.rf_dls_library_index = library_index(&sound.bank).unwrap_or(0);
                    self.rf_dls_play_index = self
                        .filtered_sounds()
                        .iter()
                        .position(|candidate| candidate.id == sound.id)
                        .unwrap_or(0);
                }
                self.page = Page::RfDlsPlay;
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
                indexed_title("LIVE SET", self.live_index, LIVE_ITEMS.len()),
                &LIVE_ITEMS,
                &LIVE_DETAILS,
                self.live_index,
            ),
            Page::Play => simple_screen(
                indexed_title("PLAY", self.play_index, PLAY_ITEMS.len()),
                &PLAY_ITEMS,
                &PLAY_DETAILS,
                self.play_index,
            ),
            Page::Config => {
                let detail = if self.config_index == 2 {
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
            Page::Plugins => simple_screen(
                indexed_title("PLUGINS", self.plugin_index, PLUGIN_ITEMS.len()),
                &PLUGIN_ITEMS,
                &PLUGIN_DETAILS,
                self.plugin_index,
            ),
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
            Page::SystemWebPairing => self.render_pairing_code(),
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
            Page::RfDlsLibrary => self.render_rf_dls_library(),
            Page::RfDlsPlay => self.render_rf_dls_play(),
            Page::RfDlsCustomPrograms => self.render_custom_programs(),
            Page::RfDlsProgramSections => simple_screen(
                indexed_title(
                    self.program_draft
                        .as_ref()
                        .map_or("PROGRAM", |draft| draft.name.as_str()),
                    self.rf_dls_section_index,
                    RF_DLS_PROGRAM_SECTIONS.len(),
                ),
                &RF_DLS_PROGRAM_SECTIONS,
                &RF_DLS_SECTION_DETAILS,
                self.rf_dls_section_index,
            ),
            Page::RfDlsName => {
                let [line_1, line_2] = component_lines(&self.program_name, false);
                Screen::with_header("RF-DLS", line_1, line_2)
            }
            Page::RfDlsLayerMenu => self.render_layer_menu(),
            Page::RfDlsTimbre => self.render_timbre(),
            Page::RfDlsEnvelope => {
                let [line_1, line_2] = component_lines(&self.envelope, true);
                Screen::fullscreen(line_1, line_2)
            }
            Page::RfDlsPitchEnvelope => {
                let [line_1, line_2] = component_lines(&self.pitch_envelope, true);
                Screen::with_header(self.layer_header("PITCH ENV"), line_1, line_2)
            }
            Page::RfDlsLfo => {
                let [line_1, line_2] = component_lines(&self.lfo, true);
                Screen::with_header(self.layer_header("LFO"), line_1, line_2)
            }
            Page::RfDlsTuning => {
                let [line_1, line_2] = component_lines(&self.tuning, true);
                Screen::with_header(self.layer_header("TUNING"), line_1, line_2)
            }
            Page::RfDlsRange => {
                let [line_1, line_2] = component_lines(&self.range, true);
                Screen::with_header(self.layer_header("RANGE"), line_1, line_2)
            }
            Page::RfDlsLayerLevel => {
                let [line_1, line_2] = component_lines(&self.layer_level, true);
                Screen::with_header(self.layer_header("VOLUME"), line_1, line_2)
            }
            Page::RfDlsSharedFx => Screen::with_header("SHARED FX", "NO FX", "Chain is empty"),
            Page::RfDlsProgramOutput => {
                let [line_1, line_2] = component_lines(&self.program_output, true);
                Screen::with_header("OUTPUT", line_1, line_2)
            }
            Page::RfDlsUnsavedChanges => {
                let [line_1, line_2] = component_lines(&self.unsaved_changes, true);
                Screen::with_header("RF-DLS", line_1, line_2)
            }
            Page::ProgramEditorRoot => self.render_generic_editor_root(),
            Page::ProgramEditorPage => self.render_generic_editor_page(),
            Page::ProgramEditorField => self.render_generic_editor_field(),
            Page::ProgramEditorSound => self.render_generic_editor_sound(),
        };
        screen.footer = standard_footer(self.pressed_button);
        screen
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
                if settings.pairing_available {
                    "READY".to_owned()
                } else {
                    "LOCKED".to_owned()
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

    fn render_pairing_code(&self) -> Screen {
        let line_1 = self.pairing_code.as_ref().map_or_else(
            || "NO ACTIVE CODE".to_owned(),
            |code| format!("CODE {code}"),
        );
        Screen::with_header("PAIR DEVICE", line_1, "VALID FOR 2 MIN")
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
                4 if self.web_settings.pairing_available => {
                    self.pending_command = Some(MenuCommand::BeginWebPairing);
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
            Page::Play => (&mut self.play_index, PLAY_ITEMS.len()),
            Page::Config => (&mut self.config_index, CONFIG_ITEMS.len()),
            Page::Plugins => (&mut self.plugin_index, PLUGIN_ITEMS.len()),
            Page::System => {
                let len = self.system_item_count();
                (&mut self.system_index, len)
            }
            Page::SystemWeb => (&mut self.system_web_index, SYSTEM_WEB_ITEMS.len()),
            Page::Audio => (&mut self.audio_index, AUDIO_ITEMS.len()),
            Page::RfDlsLibrary => (&mut self.rf_dls_library_index, RF_DLS_LIBRARIES.len()),
            Page::RfDlsPlay if self.filtered_sounds().is_empty() => return,
            Page::RfDlsPlay => {
                let len = self.filtered_sounds().len();
                (&mut self.rf_dls_play_index, len)
            }
            Page::RfDlsCustomPrograms => {
                let len = self.custom_sounds().len() + 1;
                (&mut self.rf_dls_custom_index, len)
            }
            Page::RfDlsProgramSections => (
                &mut self.rf_dls_section_index,
                RF_DLS_PROGRAM_SECTIONS.len(),
            ),
            Page::RfDlsLayerMenu => {
                let len = self.layer_menu_len();
                (&mut self.rf_dls_layer_option_index, len)
            }
            Page::RfDlsTimbre if self.dls_sounds().is_empty() => return,
            Page::RfDlsTimbre => {
                let len = self.dls_sounds().len();
                (&mut self.rf_dls_timbre_index, len)
            }
            Page::RfDlsName => return,
            Page::RfDlsTuning
            | Page::RfDlsPitchEnvelope
            | Page::RfDlsLayerLevel
            | Page::RfDlsSharedFx
            | Page::RfDlsProgramOutput
            | Page::RfDlsRange
            | Page::RfDlsLfo
            | Page::RfDlsEnvelope
            | Page::RfDlsUnsavedChanges
            | Page::ProgramEditorRoot
            | Page::ProgramEditorPage
            | Page::ProgramEditorField
            | Page::ProgramEditorSound
            | Page::SystemWebPairing
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
                Action::Back => self.page = Page::ProgramEditorPage,
                Action::Select => self.select_editor_sound(),
            },
            Page::ProgramEditorRoot | Page::ProgramEditorPage => match action {
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
                self.page = Page::RfDlsName;
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
                self.page = Page::ProgramEditorPage;
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
            self.page = Page::ProgramEditorPage;
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
                    .rf_dls_sounds
                    .iter()
                    .filter(|sound| bank.as_ref().is_none_or(|bank| sound.bank == *bank))
                    .collect::<Vec<_>>();
                self.rf_dls_timbre_index = matching
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
            self.page = Page::ProgramEditorPage;
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
        self.page = Page::ProgramEditorPage;
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
            Page::ProgramEditorPage
        };
    }

    fn editor_sound_choices(&self) -> Vec<&PlaySound> {
        let bank = self
            .active_editor_field()
            .and_then(|field| match &field.kind {
                ProgramEditorFieldKind::Sound { bank } => bank.as_deref(),
                _ => None,
            });
        self.rf_dls_sounds
            .iter()
            .filter(|sound| bank.is_none_or(|bank| sound.bank == bank))
            .collect()
    }

    fn move_editor_sound(&mut self, delta: isize) {
        let len = self.editor_sound_choices().len();
        if len > 0 {
            self.rf_dls_timbre_index =
                ((self.rf_dls_timbre_index as isize + delta).rem_euclid(len as isize)) as usize;
        }
    }

    fn select_editor_sound(&mut self) {
        let sound_id = self
            .editor_sound_choices()
            .get(self.rf_dls_timbre_index)
            .map(|sound| sound.id.clone());
        if let (Some(field_id), Some(sound_id)) = (self.editor_field_id.clone(), sound_id) {
            self.emit_generic_editor_field(field_id, ProgramEditorValue::SoundId(sound_id), false);
        }
    }

    fn apply_layer_parameter_input(&mut self, input: Input) {
        let event = match self.page {
            Page::RfDlsEnvelope => self.envelope.handle(input),
            Page::RfDlsPitchEnvelope => self.pitch_envelope.handle(input),
            Page::RfDlsLfo => self.lfo.handle(input),
            Page::RfDlsTuning => self.tuning.handle(input),
            Page::RfDlsRange => self.range.handle(input),
            Page::RfDlsLayerLevel => self.layer_level.handle(input),
            _ => ComponentEvent::Ignored,
        };
        match event {
            ComponentEvent::Changed(_) if self.layer_editor_is_editing() => {
                self.emit_layer_parameter(true);
            }
            ComponentEvent::EditCommitted(_) => self.emit_layer_parameter(false),
            ComponentEvent::EditCancelled(_) => self.restore_program_preview(),
            ComponentEvent::ExitRequested(_) => self.page = Page::RfDlsLayerMenu,
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
            Page::RfDlsEnvelope => self.envelope.selected_item(),
            Page::RfDlsPitchEnvelope => self.pitch_envelope.selected_item(),
            Page::RfDlsLfo => self.lfo.selected_item(),
            Page::RfDlsTuning => self.tuning.selected_item(),
            Page::RfDlsRange => self.range.selected_item(),
            Page::RfDlsLayerLevel => self.layer_level.selected_item(),
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
                field_id: editor_layer_field_id(self.rf_dls_layer_index, item.id()),
                value,
                preview,
            });
        }
    }

    fn layer_editor_is_editing(&self) -> bool {
        match self.page {
            Page::RfDlsEnvelope => self.envelope.is_editing(),
            Page::RfDlsPitchEnvelope => self.pitch_envelope.is_editing(),
            Page::RfDlsLfo => self.lfo.is_editing(),
            Page::RfDlsTuning => self.tuning.is_editing(),
            Page::RfDlsRange => self.range.is_editing(),
            Page::RfDlsLayerLevel => self.layer_level.is_editing(),
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
            Page::RfDlsEnvelope
                | Page::RfDlsPitchEnvelope
                | Page::RfDlsLfo
                | Page::RfDlsTuning
                | Page::RfDlsRange
                | Page::RfDlsLayerLevel
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
            ComponentEvent::ExitRequested(_) => self.page = Page::RfDlsProgramSections,
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
        let return_page = if self.page == Page::RfDlsUnsavedChanges {
            self.pending_program_exit
                .as_ref()
                .map_or(Page::RfDlsProgramSections, |pending| pending.return_page)
        } else {
            self.page
        };
        self.unsaved_changes.set_selected(0);
        self.pending_program_exit = Some(PendingProgramExit {
            return_page,
            destination,
        });
        self.page = Page::RfDlsUnsavedChanges;
    }

    fn apply_unsaved_changes_input(&mut self, input: Input) {
        match self.unsaved_changes.handle(input) {
            ComponentEvent::Activated(_) => {
                let Some(pending) = self.pending_program_exit.as_ref() else {
                    self.page = Page::RfDlsProgramSections;
                    return;
                };
                let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) else {
                    self.pending_program_exit = None;
                    self.page = Page::RfDlsCustomPrograms;
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
                    .map_or(Page::RfDlsProgramSections, |pending| pending.return_page);
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
        carousel.set_selected(self.rf_dls_timbre_index.min(sounds.len() - 1));
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        Screen::with_header(
            indexed_title(
                &field.label,
                self.rf_dls_timbre_index.min(sounds.len() - 1),
                sounds.len(),
            ),
            line_1,
            line_2,
        )
    }

    fn render_rf_dls_play(&self) -> Screen {
        let sounds = self.filtered_sounds();
        let library = RF_DLS_LIBRARIES[self.rf_dls_library_index];
        if sounds.is_empty() {
            return Screen::with_header(library, "NO PROGRAMS", " ");
        }
        let mut carousel = SimpleCarousel::new(
            "rf-dls-sounds",
            sounds.iter().map(|sound| {
                let name = if self.rf_dls_active_sound_id.as_deref() == Some(sound.id.as_str()) {
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
        carousel.set_selected(self.rf_dls_play_index);
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        Screen::with_header(
            indexed_title(library, self.rf_dls_play_index, sounds.len()),
            line_1,
            line_2,
        )
    }

    fn render_rf_dls_library(&self) -> Screen {
        let counts = RF_DLS_LIBRARIES.map(|library| {
            let bank = library.to_ascii_lowercase();
            format!(
                "{} PROGRAMS",
                self.rf_dls_sounds
                    .iter()
                    .filter(|sound| sound.bank == bank)
                    .count()
            )
        });
        let details = counts.iter().map(String::as_str).collect::<Vec<_>>();
        simple_screen(
            indexed_title(
                "RF-DLS PLAY",
                self.rf_dls_library_index,
                RF_DLS_LIBRARIES.len(),
            ),
            &RF_DLS_LIBRARIES,
            &details,
            self.rf_dls_library_index,
        )
    }

    fn render_custom_programs(&self) -> Screen {
        let custom = self.custom_sounds();
        let mut names = vec!["ADD NEW".to_owned()];
        let mut details = vec!["Create program".to_owned()];
        for program in custom {
            names.push(program.name.clone());
            details.push(program.detail.clone());
        }
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let detail_refs = details.iter().map(String::as_str).collect::<Vec<_>>();
        simple_screen(
            indexed_title("CUSTOM PROGRAMS", self.rf_dls_custom_index, names.len()),
            &name_refs,
            &detail_refs,
            self.rf_dls_custom_index,
        )
    }

    fn render_layer_menu(&self) -> Screen {
        if self.rf_dls_layer_index == 0 {
            return simple_screen(
                self.layer_header(""),
                &RF_DLS_LAYER_SECTIONS,
                &RF_DLS_LAYER_DETAILS,
                self.rf_dls_layer_option_index,
            );
        }
        if self.program_layer_enabled(1) {
            let mut sections = vec!["ENABLED"];
            sections.extend(RF_DLS_LAYER_SECTIONS);
            let mut details = vec!["ON"];
            details.extend(RF_DLS_LAYER_DETAILS);
            simple_screen(
                self.layer_header(""),
                &sections,
                &details,
                self.rf_dls_layer_option_index,
            )
        } else {
            simple_screen(
                self.layer_header(""),
                &["ENABLED"],
                &["OFF"],
                self.rf_dls_layer_option_index,
            )
        }
    }

    fn render_timbre(&self) -> Screen {
        let sounds = self.dls_sounds();
        if sounds.is_empty() {
            return Screen::with_header(self.layer_header("TIMBRE"), "NO DLS SOUNDS", " ");
        }
        let selected_id = self.program_layer_source_id(self.rf_dls_layer_index);
        let mut carousel = SimpleCarousel::new(
            "rf-dls-timbre",
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
        carousel.set_selected(self.rf_dls_timbre_index);
        carousel.set_focused(true);
        let [line_1, line_2] = component_lines(&carousel, false);
        Screen::with_header(
            indexed_title(
                &self.layer_header("TIMBRE"),
                self.rf_dls_timbre_index,
                sounds.len(),
            ),
            line_1,
            line_2,
        )
    }

    fn filtered_sounds(&self) -> Vec<&PlaySound> {
        let bank = RF_DLS_LIBRARIES[self.rf_dls_library_index].to_ascii_lowercase();
        self.rf_dls_sounds
            .iter()
            .filter(|sound| sound.bank == bank)
            .collect()
    }

    fn dls_sounds(&self) -> Vec<&PlaySound> {
        self.rf_dls_sounds
            .iter()
            .filter(|sound| sound.bank == "dls")
            .collect()
    }

    fn custom_sounds(&self) -> Vec<&PlaySound> {
        self.rf_dls_sounds
            .iter()
            .filter(|sound| sound.bank == "custom")
            .collect()
    }

    fn sync_layer_editors(&mut self) {
        let layer = self.program_layer_value(self.rf_dls_layer_index);
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
        if self.rf_dls_layer_index == 0 {
            RF_DLS_LAYER_SECTIONS.len()
        } else if !self.program_layer_enabled(1) {
            1
        } else {
            RF_DLS_LAYER_SECTIONS.len() + 1
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
        let layer = if self.rf_dls_layer_index == 0 {
            "A"
        } else {
            "B"
        };
        format!("LAYER {layer} {section}").trim_end().to_owned()
    }

    fn is_program_edit_page(&self) -> bool {
        matches!(
            self.page,
            Page::RfDlsProgramSections
                | Page::RfDlsName
                | Page::RfDlsLayerMenu
                | Page::RfDlsTimbre
                | Page::RfDlsEnvelope
                | Page::RfDlsRange
                | Page::RfDlsLfo
                | Page::RfDlsTuning
                | Page::RfDlsPitchEnvelope
                | Page::RfDlsLayerLevel
                | Page::RfDlsSharedFx
                | Page::RfDlsProgramOutput
                | Page::RfDlsUnsavedChanges
                | Page::ProgramEditorRoot
                | Page::ProgramEditorPage
                | Page::ProgramEditorField
                | Page::ProgramEditorSound
        )
    }
}

fn library_index(bank: &str) -> Option<usize> {
    RF_DLS_LIBRARIES
        .iter()
        .position(|library| library.eq_ignore_ascii_case(bank))
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
    let mut normalized = value
        .chars()
        .filter(|character| !character.is_control())
        .map(|character| if character.is_ascii() { character } else { '?' })
        .take(DISPLAY_COLUMNS)
        .collect::<String>();
    if normalized.trim().is_empty() {
        normalized = fallback.into();
    }
    normalized
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

fn wifi_password_editor() -> SecretEditor {
    let mut editor = SecretEditor::new("system-wifi-password", "PASSWORD", 64);
    editor.set_focused(true);
    editor
}

fn unsaved_changes_dialog() -> ConfirmationDialog {
    let mut dialog =
        ConfirmationDialog::new("rf-dls-unsaved", "SAVE CHANGES?", ["SAVE", "DISCARD"]);
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
    use rackforge_session_api::InstanceId;

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

    #[test]
    fn audio_menu_uses_runtime_state_and_emits_typed_changes() {
        let mut menu = Menu::default();
        menu.sync_audio_state(test_audio_state());
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
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
        let mut menu = Menu::default();
        menu.apply(Action::Previous);
        assert_eq!(menu.render().header, Header::Visible(HOME_HEADER.into()));
        assert_eq!(menu.render().line_2, "    [ CONFIG ]    ");

        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("CONFIG         1/4".into())
        );
    }

    #[test]
    fn back_preserves_each_pages_selection() {
        let mut menu = Menu::default();
        menu.apply(Action::Select);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("WARM PAD"));
        assert_eq!(menu.render().line_2.trim(), "Layered pad");
        assert!(!menu.render().line_1.contains(" v"));
        menu.apply(Action::Back);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("WARM PAD"));
    }

    #[test]
    fn envelope_is_owned_by_the_rf_dls_plugin() {
        let mut menu = Menu::default();
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLUGINS        1/1".into())
        );
        assert!(menu.render().line_1.contains("RF-DLS"));
        assert!(menu.render().line_2.trim().is_empty());
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
        assert_eq!(
            menu.render().header,
            Header::Visible("CUSTOM PROGRAM 1/1".into())
        );
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLUGINS        1/1".into())
        );
    }

    #[test]
    fn exposes_exactly_seven_physical_inputs() {
        assert_eq!(Input::PHYSICAL.len(), 7);
        assert_eq!(Input::ALL.len(), 12);
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
        let mut menu = Menu::default();
        menu.apply_input(Input::EncoderRight);
        assert_eq!(menu.render().line_1, "  LIVE    [ PLAY ]");
        menu.apply_input(Input::EncoderPress);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLAY           1/1".into())
        );
    }

    #[test]
    fn rf_dls_play_and_config_are_distinct_plugin_sections() {
        let mut menu = Menu::default();
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLAY           1/1".into())
        );
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("RF-DLS PLAY    1/2".into())
        );
        assert!(menu.render().line_1.contains("DLS"));
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("DLS            1/1".into())
        );
        assert!(menu.render().line_1.contains("[PIANO 1]"));
        assert_eq!(menu.render().line_2.trim(), "B000 P000");
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("RF-DLS PLAY    1/2".into())
        );
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("PLAY           1/1".into())
        );

        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("CUSTOM PROGRAM 1/1".into())
        );
        assert!(menu.render().line_1.contains("ADD NEW"));
    }

    #[test]
    fn rf_dls_play_uses_the_runtime_catalog_and_emits_selection() {
        let mut menu = Menu::default();
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
    fn rf_dls_keeps_dls_and_custom_in_separate_collections() {
        let mut menu = Menu::default();
        menu.set_play_sounds(
            vec![
                PlaySound::new("dls.piano", "Piano 1", "dls", "B000 P000"),
                PlaySound::new(
                    "custom.user.warm-piano",
                    "Warm Piano",
                    "custom",
                    "CUSTOM 001",
                ),
            ],
            Some("dls.piano"),
        );
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("DLS"));
        assert_eq!(menu.render().line_2.trim(), "1 PROGRAMS");
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("CUSTOM"));
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("CUSTOM         1/1".into())
        );
        assert!(menu.render().line_1.contains("WARM PIANO"));
        assert_eq!(menu.render().line_2.trim(), "CUSTOM 001");
    }

    #[test]
    fn live_remains_a_rackforge_mode_above_plugins() {
        let mut menu = Menu::default();
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("LIVE SET       1/4".into())
        );
        assert!(!menu.render().header.eq(&Header::Visible("RF-DLS".into())));
    }

    #[test]
    fn simple_carousel_shows_only_the_current_option_and_its_detail() {
        let mut menu = Menu::default();
        menu.apply(Action::Select);
        let screen = menu.apply_input_and_render(Input::Button3);
        assert!(screen.line_1.contains("WARM PAD"));
        assert!(!screen.line_1.contains("PIANO"));
        assert!(!screen.line_1.contains('['));
        assert!(!screen.line_1.contains(']'));
        assert_eq!(screen.line_2.trim(), "Layered pad");
    }

    #[test]
    fn envelope_value_editor_commits_or_cancels_before_back_exits() {
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
            ..Menu::default()
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
        let mut menu = Menu::default();
        open_new_program_editor(&mut menu);
        assert!(menu.render().line_1.contains("NAME"));
        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Visible("RF-DLS".into()));
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
        let mut menu = Menu::default();
        menu.set_play_sounds(
            vec![
                PlaySound::new("dls.piano", "Piano", "dls", "B000 P000"),
                PlaySound::new(
                    "custom.user.warm-piano",
                    "Warm Piano",
                    "custom",
                    "CUSTOM 001",
                ),
            ],
            Some("custom.user.warm-piano"),
        );
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        menu.apply(Action::Back);
        menu.apply(Action::Next);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("PLUGINS"));

        menu.apply_input(Input::Button4Long);
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::ReturnToActiveMode {
                mode: ActiveMode::Play,
                ..
            })
        ));
        menu.complete_return_to_active_mode(ActiveMode::Play, Some("custom.user.warm-piano"));
        assert_eq!(
            menu.render().header,
            Header::Visible("CUSTOM         1/1".into())
        );
        assert!(menu.render().line_1.contains("[WARM PIANO]"));
    }

    #[test]
    fn restored_mode_controls_long_back_after_a_fresh_menu_start() {
        for mode in [ActiveMode::Live, ActiveMode::Play] {
            let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
        let sounds = vec![
            PlaySound::new("dls.b00000000.p00000000", "Piano", "dls", "B000 P000"),
            PlaySound::new(
                "custom.user.warm-piano",
                "Warm Piano",
                "custom",
                "CUSTOM 001",
            ),
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
        let mut menu = Menu::default();
        open_new_program_editor(&mut menu);
        menu.apply_input(Input::HomeChord);
        assert_eq!(menu.render().header, Header::Visible(HOME_HEADER.into()));
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::ForceHome {
                cancel_draft_id: Some(17)
            })
        ));
        assert!(menu.program_draft.is_none());
        assert!(menu.audition_lease_id.is_none());
    }

    #[test]
    fn native_footer_labels_are_present_on_every_page() {
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
        menu.sync_web_settings(WebSystemSettings {
            enabled: true,
            access: WebAccess::Local,
            port: 8787,
            lan_ip: Some([192, 168, 1, 17]),
            service_online: true,
            pairing_available: false,
        });

        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        assert_eq!(menu.render().line_1.trim(), "PLUGINS");
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
        assert_eq!(menu.render().line_1, "PAIR DEVICE");
        assert_eq!(menu.render().line_2, "LOCKED");

        menu.sync_web_settings(WebSystemSettings {
            enabled: true,
            access: WebAccess::Lan,
            port: 8787,
            lan_ip: Some([192, 168, 1, 17]),
            service_online: true,
            pairing_available: true,
        });
        menu.apply_input(Input::Button1);
        assert_eq!(menu.take_command(), Some(MenuCommand::BeginWebPairing));
        menu.show_pairing_code("123456");
        assert_eq!(menu.render().header, Header::Visible("PAIR DEVICE".into()));
        assert_eq!(menu.render().line_1, "CODE 123456");
        menu.apply_input(Input::Button4);
        assert_eq!(menu.render().line_1, "PAIR DEVICE");

        menu.apply(Action::Next);
        assert_eq!(menu.render().line_1, "STATUS");
        assert_eq!(menu.render().line_2, "ONLINE");

        menu.apply(Action::Back);
        assert_eq!(menu.render().line_1.trim(), "WEB INTERFACE");
    }

    #[test]
    fn wifi_menu_groups_known_networks_and_exposes_profile_actions() {
        let mut menu = Menu::default();
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
        let mut menu = Menu::default();
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
}
