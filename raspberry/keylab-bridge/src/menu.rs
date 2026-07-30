use rackforge_controller_api::LITTLE_TEXT_COLUMNS;
use rackforge_session_api::ProgramDraftState;
pub use rackforge_ui::Input;
use rackforge_ui::{
    Component, ComponentEvent, Frame, NavigationAction as Action, Rect, TextFallback, VisualState,
    components::{
        Button, CarouselItem, ConfirmationDialog, EditableValue, SimpleCarousel, TextEditor,
        ValueCarousel, ValueItem,
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
    SelectSound {
        id: String,
    },
    BeginProgramEdit {
        program_id: Option<String>,
    },
    SetProgramDraftSound {
        draft_id: u64,
        layer_index: usize,
        sound_id: String,
    },
    AddProgramDraftLayer {
        draft_id: u64,
    },
    SetProgramDraftLayerParameter {
        draft_id: u64,
        layer_index: usize,
        parameter: String,
        value: ProgramParameterValue,
    },
    PreviewProgramDraftLayerParameter {
        draft_id: u64,
        layer_index: usize,
        parameter: String,
        value: ProgramParameterValue,
    },
    SetProgramDraftParameter {
        draft_id: u64,
        parameter: String,
        value: ProgramParameterValue,
    },
    PreviewProgramDraftParameter {
        draft_id: u64,
        parameter: String,
        value: ProgramParameterValue,
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
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProgramParameterValue {
    Number(Option<f64>),
    Integer(i64),
    Boolean(Option<bool>),
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
    Addons,
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
    addon_index: usize,
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
    pressed_button: Option<usize>,
    pending_command: Option<MenuCommand>,
}

const HOME_ITEMS: [&str; 3] = ["LIVE", "PLAY", "CONFIG"];
const HOME_HEADER: &str = "RACK FORGE";
const LIVE_ITEMS: [&str; 4] = ["PIANO 1", "WARM PAD", "DLS STRINGS", "M1 HOUSE"];
const LIVE_DETAILS: [&str; 4] = ["DLS piano", "Layered pad", "RF-DLS bank", "Korg M1"];
const PLAY_ITEMS: [&str; 1] = ["RF-DLS"];
const PLAY_DETAILS: [&str; 1] = ["DLS banks"];
const CONFIG_ITEMS: [&str; 4] = ["ADDONS", "SETLISTS", "AUDIO", "SYSTEM"];
const CONFIG_DETAILS: [&str; 4] = [
    "Addon settings",
    "Performance order",
    "Scarlett Solo",
    "RackForge settings",
];
const ADDON_ITEMS: [&str; 1] = ["RF-DLS"];
const ADDON_DETAILS: [&str; 1] = [" "];
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
            addon_index: 0,
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
            pressed_button: None,
            pending_command: None,
        }
    }
}

impl Menu {
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
                self.rf_dls_section_index = 0;
                self.page = Page::RfDlsProgramSections;
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
        if self.page == Page::RfDlsUnsavedChanges {
            self.apply_unsaved_changes_input(input);
        } else if self.page == Page::RfDlsName {
            self.apply_program_name_input(input);
        } else if self.page == Page::RfDlsProgramOutput {
            self.apply_program_output_input(input);
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
                    Page::RfDlsCustomPrograms => Page::Addons,
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
                    Page::Addons => Page::Config,
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
                            Page::Live
                        }
                        1 => {
                            self.active_mode = ActiveMode::Play;
                            Page::Play
                        }
                        _ => Page::Config,
                    },
                    Page::Play if self.play_index == 0 => Page::RfDlsLibrary,
                    Page::RfDlsLibrary => {
                        self.rf_dls_play_index = 0;
                        Page::RfDlsPlay
                    }
                    Page::Config if self.config_index == 0 => Page::Addons,
                    Page::Addons if self.addon_index == 0 => Page::RfDlsCustomPrograms,
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
                            self.pending_command = Some(MenuCommand::SetProgramDraftSound {
                                draft_id,
                                layer_index: self.rf_dls_layer_index,
                                sound_id,
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
            Page::Config => simple_screen(
                indexed_title("CONFIG", self.config_index, CONFIG_ITEMS.len()),
                &CONFIG_ITEMS,
                &CONFIG_DETAILS,
                self.config_index,
            ),
            Page::Addons => simple_screen(
                indexed_title("ADDONS", self.addon_index, ADDON_ITEMS.len()),
                &ADDON_ITEMS,
                &ADDON_DETAILS,
                self.addon_index,
            ),
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
        };
        screen.footer = standard_footer(self.pressed_button);
        screen
    }

    fn move_selection(&mut self, delta: isize) {
        let (selection, len) = match self.page {
            Page::Home => (&mut self.home_index, HOME_ITEMS.len()),
            Page::Live => (&mut self.live_index, LIVE_ITEMS.len()),
            Page::Play => (&mut self.play_index, PLAY_ITEMS.len()),
            Page::Config => (&mut self.config_index, CONFIG_ITEMS.len()),
            Page::Addons => (&mut self.addon_index, ADDON_ITEMS.len()),
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
            | Page::RfDlsUnsavedChanges => return,
        };
        *selection = ((*selection as isize + delta).rem_euclid(len as isize)) as usize;
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
        if self.program_layer_count() < 2 {
            self.pending_command = Some(MenuCommand::AddProgramDraftLayer { draft_id });
        } else {
            self.pending_command = Some(MenuCommand::SetProgramDraftLayerParameter {
                draft_id,
                layer_index: 1,
                parameter: "enabled".into(),
                value: ProgramParameterValue::Boolean(Some(!enabled)),
            });
        }
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
            ProgramParameterValue::Boolean(match item.value().choice_index() {
                Some(0) => None,
                Some(1) => Some(true),
                Some(2) => Some(false),
                _ => return,
            })
        } else if let Some(value) = item.value().as_optional_f64() {
            ProgramParameterValue::Number(value)
        } else if let Some(value) = item.value().as_i64() {
            ProgramParameterValue::Integer(value)
        } else if let Some(value) = item.value().as_f64() {
            ProgramParameterValue::Number(Some(value))
        } else {
            return;
        };
        if let Some(draft_id) = self.program_draft.as_ref().map(|draft| draft.draft_id) {
            self.pending_command = Some(if preview {
                MenuCommand::PreviewProgramDraftLayerParameter {
                    draft_id,
                    layer_index: self.rf_dls_layer_index,
                    parameter: item.id().to_owned(),
                    value,
                }
            } else {
                MenuCommand::SetProgramDraftLayerParameter {
                    draft_id,
                    layer_index: self.rf_dls_layer_index,
                    parameter: item.id().to_owned(),
                    value,
                }
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
            self.pending_command = Some(if preview {
                MenuCommand::PreviewProgramDraftParameter {
                    draft_id,
                    parameter: "gain".into(),
                    value: ProgramParameterValue::Number(Some(value)),
                }
            } else {
                MenuCommand::SetProgramDraftParameter {
                    draft_id,
                    parameter: "gain".into(),
                    value: ProgramParameterValue::Number(Some(value)),
                }
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
                self.page = Page::RfDlsProgramSections;
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
        )
    }
}

fn library_index(bank: &str) -> Option<usize> {
    RF_DLS_LIBRARIES
        .iter()
        .position(|library| library.eq_ignore_ascii_case(bank))
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
    use rackforge_session_api::InstanceId;

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
    fn envelope_is_owned_by_the_rf_dls_addon() {
        let mut menu = Menu::default();
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("ADDONS         1/1".into())
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
        assert_eq!(menu.render().header, Header::Hidden);
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
            Header::Visible("ADDONS         1/1".into())
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
    fn rf_dls_play_and_config_are_distinct_addon_sections() {
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
    fn live_remains_a_rackforge_mode_above_addons() {
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
        assert!(menu.envelope.is_editing());
        assert!(menu.render().line_2.starts_with('['));
        menu.apply_input(Input::EncoderRight);
        assert_eq!(
            menu.render().line_2.trim_matches(&['[', ']', ' '][..]),
            "0.01 s"
        );
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::PreviewProgramDraftLayerParameter {
                draft_id: 17,
                layer_index: 0,
                parameter: "amplitude_envelope.attack_seconds".into(),
                value: ProgramParameterValue::Number(Some(0.01)),
            })
        );
        menu.apply_input(Input::Button4);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::RestoreProgramDraftPreview { draft_id: 17 })
        );
        assert_eq!(menu.render().line_2.trim(), "INHERIT");
        assert!(!menu.envelope.is_editing());
        assert!(menu.render().line_1.starts_with('['));

        menu.apply_input(Input::Button1);
        menu.apply_input(Input::EncoderRight);
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_2.trim(), "0.01 s");
        assert!(!menu.envelope.is_editing());
        assert!(menu.render().line_1.starts_with('['));
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetProgramDraftLayerParameter {
                draft_id: 17,
                layer_index: 0,
                parameter: "amplitude_envelope.attack_seconds".into(),
                value: ProgramParameterValue::Number(Some(0.01)),
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
            Some(MenuCommand::SetProgramDraftSound {
                draft_id: 17,
                layer_index: 0,
                sound_id: "dls.b00000000.p00000030".into(),
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
            Some(MenuCommand::AddProgramDraftLayer { draft_id: 17 })
        );
        menu.sync_program_edit(Some(draft_with_layer_b(17, true)), Some(7));
        assert!(menu.render().line_1.contains("ENABLED"));
        assert_eq!(menu.render().line_2.trim(), "ON");

        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("TIMBRE"));
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("LAYER B TIMBRE 1/1".into())
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
            Some(MenuCommand::SetProgramDraftLayerParameter {
                draft_id: 17,
                layer_index: 1,
                parameter: "enabled".into(),
                value: ProgramParameterValue::Boolean(Some(false)),
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
        assert_eq!(
            menu.render().header,
            Header::Visible("LAYER A VOLUME".into())
        );
        assert!(menu.render().line_1.contains("VOLUME"));
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button2);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::PreviewProgramDraftLayerParameter {
                draft_id: 17,
                layer_index: 0,
                parameter: "gain".into(),
                value: ProgramParameterValue::Number(Some(0.99)),
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
            Some(MenuCommand::SetProgramDraftLayerParameter {
                draft_id: 17,
                layer_index: 0,
                parameter: "lfo.delay_seconds".into(),
                value: ProgramParameterValue::Number(Some(0.0)),
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
        assert_eq!(menu.render().header, Header::Visible("SHARED FX".into()));
        assert_eq!(menu.render().line_1.trim(), "NO FX");
        menu.apply(Action::Back);

        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("OUTPUT"));
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("GAIN"));
        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button2);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::PreviewProgramDraftParameter {
                draft_id: 17,
                parameter: "gain".into(),
                value: ProgramParameterValue::Number(Some(0.99)),
            })
        );
        menu.apply_input(Input::Button1);
        assert_eq!(
            menu.take_command(),
            Some(MenuCommand::SetProgramDraftParameter {
                draft_id: 17,
                parameter: "gain".into(),
                value: ProgramParameterValue::Number(Some(0.99)),
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
    fn long_back_returns_to_the_active_play_addon_and_centers_selection() {
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
        assert!(menu.render().line_1.contains("ADDONS"));

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
}
