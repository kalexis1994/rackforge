pub use rackforge_ui::Input;
use rackforge_ui::{
    Component, ComponentEvent, Frame, NavigationAction as Action, Rect, TextFallback, VisualState,
    components::{Button, CarouselItem, EditableValue, SimpleCarousel, ValueCarousel, ValueItem},
};

pub const DISPLAY_COLUMNS: usize = 18;

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
    pub detail: String,
}

impl PlaySound {
    pub fn new(id: impl Into<String>, name: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: normalized_display_text(&name.into(), "UNNAMED").to_ascii_uppercase(),
            detail: normalized_display_text(&detail.into(), " "),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MenuCommand {
    SelectSound { id: String },
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
    RfDlsPlay,
    RfDlsConfig,
    RfDlsEnvelope,
}

#[derive(Debug)]
pub struct Menu {
    page: Page,
    home_index: usize,
    live_index: usize,
    play_index: usize,
    config_index: usize,
    addon_index: usize,
    rf_dls_play_index: usize,
    rf_dls_config_index: usize,
    rf_dls_sounds: Vec<PlaySound>,
    rf_dls_active_sound_id: Option<String>,
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
const RF_DLS_CONFIG_ITEMS: [&str; 4] = ["ENVELOPE", "VOLUME", "BANK", "EFFECTS"];
const RF_DLS_CONFIG_DETAILS: [&str; 4] = [
    "DLS voice shape",
    "Master output",
    "gm.dls resource",
    "Chorus / reverb",
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
            rf_dls_play_index: 0,
            rf_dls_config_index: 0,
            rf_dls_sounds: vec![PlaySound::new(
                "dls.b00000000.p00000000",
                "PIANO 1",
                "B000 P000",
            )],
            rf_dls_active_sound_id: Some("dls.b00000000.p00000000".into()),
            envelope: envelope_carousel(),
            pressed_button: None,
            pending_command: None,
        }
    }
}

impl Menu {
    pub fn set_play_sounds(&mut self, sounds: Vec<PlaySound>, selected_sound_id: Option<&str>) {
        let browsed_sound_id = self
            .rf_dls_sounds
            .get(self.rf_dls_play_index)
            .map(|sound| sound.id.clone());
        self.rf_dls_sounds = sounds;
        self.rf_dls_active_sound_id = selected_sound_id.map(str::to_owned);
        self.rf_dls_play_index = browsed_sound_id
            .as_deref()
            .and_then(|browsed| {
                self.rf_dls_sounds
                    .iter()
                    .position(|sound| sound.id == browsed)
            })
            .or_else(|| {
                selected_sound_id.and_then(|selected| {
                    self.rf_dls_sounds
                        .iter()
                        .position(|sound| sound.id == selected)
                })
            })
            .unwrap_or(0);
    }

    pub fn take_command(&mut self) -> Option<MenuCommand> {
        self.pending_command.take()
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
                    Page::RfDlsPlay => Page::Play,
                    Page::RfDlsConfig => Page::Addons,
                    Page::Addons => Page::Config,
                    Page::Home => Page::Home,
                    _ => Page::Home,
                };
            }
            Action::Select => {
                if self.page == Page::RfDlsPlay {
                    if let Some(sound) = self.rf_dls_sounds.get(self.rf_dls_play_index) {
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
                    Page::Play if self.play_index == 0 => Page::RfDlsPlay,
                    Page::Config if self.config_index == 0 => Page::Addons,
                    Page::Addons if self.addon_index == 0 => Page::RfDlsConfig,
                    Page::RfDlsConfig if self.rf_dls_config_index == 0 => Page::RfDlsEnvelope,
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
            Page::RfDlsPlay => self.render_rf_dls_play(),
            Page::RfDlsConfig => simple_screen(
                indexed_title(
                    "RF-DLS CONFIG",
                    self.rf_dls_config_index,
                    RF_DLS_CONFIG_ITEMS.len(),
                ),
                &RF_DLS_CONFIG_ITEMS,
                &RF_DLS_CONFIG_DETAILS,
                self.rf_dls_config_index,
            ),
            Page::RfDlsEnvelope => {
                let [line_1, line_2] = component_lines(&self.envelope, true);
                Screen::fullscreen(line_1, line_2)
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
            Page::RfDlsPlay if self.rf_dls_sounds.is_empty() => return,
            Page::RfDlsPlay => (&mut self.rf_dls_play_index, self.rf_dls_sounds.len()),
            Page::RfDlsConfig => (&mut self.rf_dls_config_index, RF_DLS_CONFIG_ITEMS.len()),
            Page::RfDlsEnvelope => return,
        };
        *selection = ((*selection as isize + delta).rem_euclid(len as isize)) as usize;
    }

    fn apply_envelope_input(&mut self, input: Input) {
        if matches!(
            self.envelope.handle(input),
            ComponentEvent::ExitRequested(_)
        ) {
            self.page = Page::RfDlsConfig;
        }
    }

    fn render_rf_dls_play(&self) -> Screen {
        if self.rf_dls_sounds.is_empty() {
            return Screen::with_header("RF-DLS PLAY", "NO SOUNDS", " ");
        }
        let mut carousel = SimpleCarousel::new(
            "rf-dls-sounds",
            self.rf_dls_sounds.iter().map(|sound| {
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
            indexed_title(
                "RF-DLS PLAY",
                self.rf_dls_play_index,
                self.rf_dls_sounds.len(),
            ),
            line_1,
            line_2,
        )
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
        assert_eq!(
            menu.render().header,
            Header::Visible("RF-DLS CONFIG  1/4".into())
        );
        assert!(menu.render().line_1.contains("ENVELOPE"));
        assert_eq!(menu.render().line_2.trim(), "DLS voice shape");

        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Hidden);
        assert!(menu.render().line_1.contains("ATTACK"));
        assert_eq!(menu.render().line_2.trim(), "0.00 s");

        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("RF-DLS CONFIG  1/4".into())
        );
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("ADDONS         1/1".into())
        );
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("CONFIG         1/4".into())
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
            Header::Visible("RF-DLS PLAY    1/1".into())
        );
        assert!(menu.render().line_1.contains("[PIANO 1]"));
        assert_eq!(menu.render().line_2.trim(), "B000 P000");
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
            Header::Visible("RF-DLS CONFIG  1/4".into())
        );
    }

    #[test]
    fn rf_dls_play_uses_the_runtime_catalog_and_emits_selection() {
        let mut menu = Menu::default();
        menu.set_play_sounds(
            vec![
                PlaySound::new("sound.piano", "Piano 1", "B000 P000"),
                PlaySound::new("sound.strings", "Stríngs 1", "B000 P048"),
            ],
            Some("sound.piano"),
        );
        menu.apply(Action::Next);
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
                PlaySound::new("sound.piano", "Piano 1", "B000 P000"),
                PlaySound::new("sound.strings", "Strings 1", "B000 P048"),
            ],
            Some("sound.piano"),
        );
        assert!(menu.render().line_1.contains("STRINGS 1"));
        assert!(!menu.render().line_1.contains('['));

        menu.set_play_sounds(
            vec![
                PlaySound::new("sound.piano", "Piano 1", "B000 P000"),
                PlaySound::new("sound.strings", "Strings 1", "B000 P048"),
            ],
            Some("sound.strings"),
        );
        assert!(menu.render().line_1.contains("[STRINGS 1]"));
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
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
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
        assert_eq!(
            menu.render().header,
            Header::Visible("RF-DLS CONFIG  1/4".into())
        );
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
