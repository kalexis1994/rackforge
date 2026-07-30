use rackforge_controller_api::LITTLE_TEXT_COLUMNS;
pub use rackforge_ui::Input;
use rackforge_ui::{
    Component, ComponentEvent, Frame, NavigationAction as Action, Rect, TextFallback, VisualState,
    components::{Button, CarouselItem, EditableValue, SimpleCarousel, ValueCarousel, ValueItem},
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuCommand {
    SelectSound {
        id: String,
    },
    BeginAudition {
        preview_sound_id: String,
        draft_name: String,
        program_id: Option<String>,
    },
    EndAudition {
        lease_id: u64,
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
    Addons,
    RfDlsLibrary,
    RfDlsPlay,
    RfDlsCustomPrograms,
    RfDlsProgramSections,
    RfDlsTimbre,
    RfDlsExpression,
    RfDlsEnvelope,
    RfDlsTuning,
    RfDlsFx,
    RfDlsOutput,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProgramDraft {
    program_id: Option<String>,
    name: String,
    base_sound_id: String,
}

#[derive(Debug)]
pub struct Menu {
    page: Page,
    home_index: usize,
    live_index: usize,
    play_index: usize,
    config_index: usize,
    addon_index: usize,
    rf_dls_library_index: usize,
    rf_dls_play_index: usize,
    rf_dls_custom_index: usize,
    rf_dls_section_index: usize,
    rf_dls_timbre_index: usize,
    rf_dls_sounds: Vec<PlaySound>,
    rf_dls_active_sound_id: Option<String>,
    audition_lease_id: Option<u64>,
    pending_draft: Option<ProgramDraft>,
    program_draft: Option<ProgramDraft>,
    envelope: ValueCarousel,
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
const RF_DLS_PROGRAM_SECTIONS: [&str; 6] =
    ["TIMBRE", "EXPRESSION", "ENVELOPE", "TUNING", "FX", "OUTPUT"];
const RF_DLS_SECTION_DETAILS: [&str; 6] = [
    "Base DLS sound",
    "Wheel and pedals",
    "ADSR override",
    "Pitch and octave",
    "Chorus / reverb",
    "Level and pan",
];

impl Default for Menu {
    fn default() -> Self {
        Self {
            page: Page::Home,
            home_index: 0,
            live_index: 0,
            play_index: 0,
            config_index: 0,
            addon_index: 0,
            rf_dls_library_index: 0,
            rf_dls_play_index: 0,
            rf_dls_custom_index: 0,
            rf_dls_section_index: 0,
            rf_dls_timbre_index: 0,
            rf_dls_sounds: vec![PlaySound::new(
                "dls.b00000000.p00000000",
                "PIANO 1",
                "dls",
                "B000 P000",
            )],
            rf_dls_active_sound_id: Some("dls.b00000000.p00000000".into()),
            audition_lease_id: None,
            pending_draft: None,
            program_draft: None,
            envelope: envelope_carousel(),
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

    pub fn audition_started(&mut self, lease_id: u64) -> bool {
        let Some(draft) = self.pending_draft.take() else {
            return false;
        };
        if draft.name.trim().is_empty() || draft.base_sound_id.is_empty() {
            return false;
        }
        self.audition_lease_id = Some(lease_id);
        self.program_draft = Some(draft);
        self.rf_dls_section_index = 0;
        self.page = Page::RfDlsProgramSections;
        true
    }

    pub fn audition_ended(&mut self) {
        self.audition_lease_id = None;
        self.pending_draft = None;
        self.program_draft = None;
        self.page = Page::RfDlsCustomPrograms;
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
        if self.page == Page::RfDlsEnvelope {
            self.apply_envelope_input(input);
        } else {
            self.apply(input.default_navigation());
        }
    }

    fn begin_program_edit(&mut self) {
        let dls_sounds = self.dls_sounds();
        let Some(first_dls) = dls_sounds.first() else {
            return;
        };
        let custom_sounds = self.custom_sounds();
        let draft = if self.rf_dls_custom_index == 0 {
            ProgramDraft {
                program_id: None,
                name: format!("CUSTOM {:03}", custom_sounds.len() + 1),
                base_sound_id: first_dls.id.clone(),
            }
        } else {
            let Some(program) = custom_sounds.get(self.rf_dls_custom_index - 1) else {
                return;
            };
            ProgramDraft {
                program_id: Some(program.id.clone()),
                name: program.name.clone(),
                base_sound_id: program.id.clone(),
            }
        };
        self.pending_command = Some(MenuCommand::BeginAudition {
            preview_sound_id: draft.base_sound_id.clone(),
            draft_name: draft.name.clone(),
            program_id: draft.program_id.clone(),
        });
        self.pending_draft = Some(draft);
    }

    pub fn apply_input_and_render(&mut self, input: Input) -> Screen {
        self.apply_input(input);
        self.render()
    }

    pub fn apply(&mut self, action: Action) {
        if self.page == Page::RfDlsEnvelope {
            let input = match action {
                Action::Previous => Input::Button2,
                Action::Next => Input::Button3,
                Action::Back => Input::Button4,
                Action::Select => Input::Button1,
            };
            self.apply_envelope_input(input);
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
                    Page::RfDlsTimbre
                    | Page::RfDlsExpression
                    | Page::RfDlsEnvelope
                    | Page::RfDlsTuning
                    | Page::RfDlsFx
                    | Page::RfDlsOutput => Page::RfDlsProgramSections,
                    Page::RfDlsProgramSections => {
                        if let Some(lease_id) = self.audition_lease_id {
                            self.pending_command = Some(MenuCommand::EndAudition { lease_id });
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
                        0 => Page::Live,
                        1 => Page::Play,
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
                        0 => Page::RfDlsTimbre,
                        1 => Page::RfDlsExpression,
                        2 => Page::RfDlsEnvelope,
                        3 => Page::RfDlsTuning,
                        4 => Page::RfDlsFx,
                        _ => Page::RfDlsOutput,
                    },
                    Page::RfDlsTimbre => {
                        let sound_id = self
                            .dls_sounds()
                            .get(self.rf_dls_timbre_index)
                            .map(|sound| sound.id.clone());
                        if let Some(sound_id) = sound_id {
                            if let Some(draft) = self.program_draft.as_mut() {
                                draft.base_sound_id = sound_id.clone();
                            }
                            self.pending_command = Some(MenuCommand::SelectSound { id: sound_id });
                        }
                        Page::RfDlsTimbre
                    }
                    page => page,
                };
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
            Page::RfDlsTimbre => self.render_timbre(),
            Page::RfDlsExpression => Screen::with_header("EXPRESSION", "MOD WHEEL", "DEPTH 100%"),
            Page::RfDlsEnvelope => {
                let [line_1, line_2] = component_lines(&self.envelope, true);
                Screen::fullscreen(line_1, line_2)
            }
            Page::RfDlsTuning => Screen::with_header("TUNING", "TRANSPOSE", "0 st"),
            Page::RfDlsFx => Screen::with_header("FX", "REVERB", "0%"),
            Page::RfDlsOutput => Screen::with_header("OUTPUT", "LEVEL", "100%"),
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
            Page::RfDlsTimbre if self.dls_sounds().is_empty() => return,
            Page::RfDlsTimbre => {
                let len = self.dls_sounds().len();
                (&mut self.rf_dls_timbre_index, len)
            }
            Page::RfDlsExpression | Page::RfDlsTuning | Page::RfDlsFx | Page::RfDlsOutput => return,
            Page::RfDlsEnvelope => return,
        };
        *selection = ((*selection as isize + delta).rem_euclid(len as isize)) as usize;
    }

    fn apply_envelope_input(&mut self, input: Input) {
        if matches!(
            self.envelope.handle(input),
            ComponentEvent::ExitRequested(_)
        ) {
            self.page = Page::RfDlsProgramSections;
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

    fn render_timbre(&self) -> Screen {
        let sounds = self.dls_sounds();
        if sounds.is_empty() {
            return Screen::with_header("TIMBRE", "NO DLS SOUNDS", " ");
        }
        let selected_id = self
            .program_draft
            .as_ref()
            .map(|draft| draft.base_sound_id.as_str());
        let mut carousel = SimpleCarousel::new(
            "rf-dls-timbre",
            sounds.iter().map(|sound| {
                let name = if selected_id == Some(sound.id.as_str()) {
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
            indexed_title("TIMBRE", self.rf_dls_timbre_index, sounds.len()),
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

fn envelope_carousel() -> ValueCarousel {
    let mut carousel = ValueCarousel::new(
        "rf-dls-envelope",
        [
            ValueItem::new(
                "envelope.attack",
                "ATTACK",
                EditableValue::number(0.0, 0.0, 5.0, 0.01, 2, "s"),
            ),
            ValueItem::new(
                "envelope.decay",
                "DECAY",
                EditableValue::number(0.0, 0.0, 5.0, 0.01, 2, "s"),
            ),
            ValueItem::new(
                "envelope.sustain",
                "SUSTAIN",
                EditableValue::number(1.0, 0.0, 1.0, 0.01, 2, "x"),
            ),
            ValueItem::new(
                "envelope.release",
                "RELEASE",
                EditableValue::number(0.05, 0.005, 10.0, 0.005, 3, "s"),
            ),
        ],
    );
    carousel.set_focused(true);
    carousel
}

pub fn demo_frames() -> Vec<Screen> {
    let mut menu = Menu::default();
    let mut screens = vec![menu.render()];
    for input in Input::ALL {
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

    fn open_new_program_editor(menu: &mut Menu) {
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert!(menu.render().line_1.contains("ADD NEW"));
        menu.apply(Action::Select);
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::BeginAudition {
                program_id: None,
                ..
            })
        ));
        assert!(menu.audition_started(7));
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
        assert!(menu.audition_started(7));
        assert!(menu.render().line_1.contains("TIMBRE"));
        menu.apply(Action::Next);
        menu.apply(Action::Next);
        assert!(menu.render().line_1.contains("ENVELOPE"));
        assert_eq!(menu.render().line_2.trim(), "ADSR override");
        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Hidden);
        assert!(menu.render().line_1.contains("ATTACK"));
        assert_eq!(menu.render().line_2.trim(), "0.00 s");

        menu.apply(Action::Back);
        assert!(menu.render().line_1.contains("ENVELOPE"));
        menu.apply(Action::Back);
        assert!(matches!(
            menu.take_command(),
            Some(MenuCommand::EndAudition { lease_id: 7 })
        ));
        menu.audition_ended();
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
        assert_eq!(Input::ALL.len(), 7);
        assert_eq!(Input::Button1.default_navigation(), Action::Select);
        assert_eq!(Input::Button2.default_navigation(), Action::Previous);
        assert_eq!(Input::Button3.default_navigation(), Action::Next);
        assert_eq!(Input::Button4.default_navigation(), Action::Back);
        assert_eq!(Input::EncoderLeft.default_navigation(), Action::Previous);
        assert_eq!(Input::EncoderRight.default_navigation(), Action::Next);
        assert_eq!(Input::EncoderPress.default_navigation(), Action::Select);
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
        menu.apply_input(Input::Button4);
        assert_eq!(menu.render().line_2.trim(), "0.00 s");
        assert!(!menu.envelope.is_editing());
        assert!(menu.render().line_1.starts_with('['));

        menu.apply_input(Input::Button1);
        menu.apply_input(Input::Button3);
        menu.apply_input(Input::Button1);
        assert_eq!(menu.render().line_2.trim(), "0.01 s");
        assert!(!menu.envelope.is_editing());
        assert!(menu.render().line_1.starts_with('['));

        menu.apply_input(Input::Button4);
        assert!(menu.render().line_1.contains("ENVELOPE"));
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
