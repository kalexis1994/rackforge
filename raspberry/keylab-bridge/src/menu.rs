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
    Instruments,
    Roland,
    RolandEnvelope,
}

#[derive(Debug)]
pub struct Menu {
    page: Page,
    home_index: usize,
    live_index: usize,
    play_index: usize,
    config_index: usize,
    instrument_index: usize,
    roland_index: usize,
    envelope: ValueCarousel,
    pressed_button: Option<usize>,
}

const HOME_ITEMS: [&str; 3] = ["LIVE", "PLAY", "CONFIG"];
const HOME_HEADER: &str = "RACK FORGE";
const LIVE_ITEMS: [&str; 4] = ["PIANO 1", "WARM PAD", "SC-55", "M1 HOUSE"];
const LIVE_DETAILS: [&str; 4] = ["SCVA piano", "Layered pad", "Sound Canvas", "Korg M1"];
const PLAY_ITEMS: [&str; 2] = ["ROLAND SCVA", "KORG M1"];
const PLAY_DETAILS: [&str; 2] = ["Sound Canvas", "Legacy synth"];
const CONFIG_ITEMS: [&str; 4] = ["INSTRUMENTS", "SETLISTS", "AUDIO", "SYSTEM"];
const CONFIG_DETAILS: [&str; 4] = [
    "Plugin racks",
    "Performance order",
    "Scarlett Solo",
    "RackForge settings",
];
const INSTRUMENT_ITEMS: [&str; 2] = ["ROLAND SCVA", "KORG M1"];
const INSTRUMENT_DETAILS: [&str; 2] = ["Current plugin", "Not installed"];
const ROLAND_ITEMS: [&str; 3] = ["ENVELOPE", "VOLUME", "EFFECTS"];
const ROLAND_DETAILS: [&str; 3] = ["Shape each voice", "Master output", "Chorus and reverb"];

impl Default for Menu {
    fn default() -> Self {
        Self {
            page: Page::Home,
            home_index: 0,
            live_index: 0,
            play_index: 0,
            config_index: 0,
            instrument_index: 0,
            roland_index: 0,
            envelope: envelope_carousel(),
            pressed_button: None,
        }
    }
}

impl Menu {
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
        if self.page == Page::RolandEnvelope {
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
        if self.page == Page::RolandEnvelope {
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
                    Page::Roland => Page::Instruments,
                    Page::Instruments => Page::Config,
                    Page::Home => Page::Home,
                    _ => Page::Home,
                };
            }
            Action::Select => {
                self.page = match self.page {
                    Page::Home => match self.home_index {
                        0 => Page::Live,
                        1 => Page::Play,
                        _ => Page::Config,
                    },
                    Page::Config if self.config_index == 0 => Page::Instruments,
                    Page::Instruments if self.instrument_index == 0 => Page::Roland,
                    Page::Roland if self.roland_index == 0 => Page::RolandEnvelope,
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
            Page::Instruments => simple_screen(
                indexed_title("INSTRUMENTS", self.instrument_index, INSTRUMENT_ITEMS.len()),
                &INSTRUMENT_ITEMS,
                &INSTRUMENT_DETAILS,
                self.instrument_index,
            ),
            Page::Roland => simple_screen(
                "ROLAND SCVA",
                &ROLAND_ITEMS,
                &ROLAND_DETAILS,
                self.roland_index,
            ),
            Page::RolandEnvelope => {
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
            Page::Instruments => (&mut self.instrument_index, INSTRUMENT_ITEMS.len()),
            Page::Roland => (&mut self.roland_index, ROLAND_ITEMS.len()),
            Page::RolandEnvelope => return,
        };
        *selection = ((*selection as isize + delta).rem_euclid(len as isize)) as usize;
    }

    fn apply_envelope_input(&mut self, input: Input) {
        if matches!(
            self.envelope.handle(input),
            ComponentEvent::ExitRequested(_)
        ) {
            self.page = Page::Roland;
        }
    }
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
        "roland-envelope",
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
    fn envelope_is_owned_by_the_roland_plugin() {
        let mut menu = Menu::default();
        menu.apply(Action::Previous);
        menu.apply(Action::Select);
        menu.apply(Action::Select);
        assert_eq!(
            menu.render().header,
            Header::Visible("INSTRUMENTS    1/2".into())
        );
        assert!(menu.render().line_1.contains("ROLAND SCVA"));
        assert!(!menu.render().line_1.contains(" v"));

        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Visible("ROLAND SCVA".into()));
        assert!(menu.render().line_1.contains("ENVELOPE"));
        assert_eq!(menu.render().line_2.trim(), "Shape each voice");

        menu.apply(Action::Select);
        assert_eq!(menu.render().header, Header::Hidden);
        assert!(menu.render().line_1.contains("ATTACK"));
        assert_eq!(menu.render().line_2.trim(), "0.00 s");

        menu.apply(Action::Back);
        assert_eq!(menu.render().header, Header::Visible("ROLAND SCVA".into()));
        menu.apply(Action::Back);
        assert_eq!(
            menu.render().header,
            Header::Visible("INSTRUMENTS    1/2".into())
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
            Header::Visible("PLAY           1/2".into())
        );
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
        assert_eq!(menu.render().header, Header::Visible("ROLAND SCVA".into()));
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
