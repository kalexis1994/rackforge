use crate::{
    Component, ComponentEvent, ComponentId, EditorCommand, EditorMode, EditorState, Frame, Input,
    Rect, Style, VisualState,
};

use super::render_centered;

#[derive(Clone, Debug, PartialEq)]
pub enum EditableValue {
    Number {
        value: f64,
        minimum: f64,
        maximum: f64,
        step: f64,
        /// `maximum / minimum` when a turn should multiply rather than add.
        ///
        /// A room runs forty-five cubic metres to forty-five thousand. Stepping
        /// that by its declared cubic metre would take forty-five thousand
        /// presses to cross, and stepping it by a hundredth of the range would
        /// jump from a broom cupboard to a hall on the first one. Equal ratios
        /// give the same number of turns everywhere, which is how the quantity
        /// is heard.
        ratio: Option<f64>,
        decimals: usize,
        unit: String,
    },
    OptionalNumber {
        value: Option<f64>,
        initial: f64,
        minimum: f64,
        maximum: f64,
        step: f64,
        decimals: usize,
        unit: String,
    },
    Integer {
        value: i64,
        minimum: i64,
        maximum: i64,
        step: i64,
        unit: String,
    },
    Choice {
        selected: usize,
        choices: Vec<String>,
    },
    Toggle {
        value: bool,
        off_label: String,
        on_label: String,
    },
}

impl EditableValue {
    /// A number whose turns are equal ratios rather than equal steps.
    ///
    /// `step` is read as the fraction of the whole span one turn covers, so a
    /// logarithmic control has the same number of detents as a linear one and
    /// spends them where the quantity actually changes.
    pub fn number_logarithmic(
        value: f64,
        minimum: f64,
        maximum: f64,
        turn_fraction: f64,
        decimals: usize,
        unit: impl Into<String>,
    ) -> Self {
        assert!(minimum > 0.0 && minimum < maximum && turn_fraction > 0.0);
        Self::Number {
            value: value.clamp(minimum, maximum),
            minimum,
            maximum,
            step: turn_fraction,
            ratio: Some(maximum / minimum),
            decimals,
            unit: unit.into(),
        }
    }

    pub fn number(
        value: f64,
        minimum: f64,
        maximum: f64,
        step: f64,
        decimals: usize,
        unit: impl Into<String>,
    ) -> Self {
        assert!(
            value.is_finite()
                && minimum.is_finite()
                && maximum.is_finite()
                && step.is_finite()
                && minimum < maximum
                && (minimum..=maximum).contains(&value)
                && step > 0.0
                && decimals <= 6
        );
        Self::Number {
            value,
            minimum,
            maximum,
            step,
            ratio: None,
            decimals,
            unit: unit.into(),
        }
    }

    pub fn integer(
        value: i64,
        minimum: i64,
        maximum: i64,
        step: i64,
        unit: impl Into<String>,
    ) -> Self {
        assert!(minimum < maximum && (minimum..=maximum).contains(&value) && step > 0);
        Self::Integer {
            value,
            minimum,
            maximum,
            step,
            unit: unit.into(),
        }
    }

    pub fn optional_number(
        value: Option<f64>,
        initial: f64,
        minimum: f64,
        maximum: f64,
        step: f64,
        decimals: usize,
        unit: impl Into<String>,
    ) -> Self {
        assert!(
            value.is_none_or(f64::is_finite)
                && initial.is_finite()
                && minimum.is_finite()
                && maximum.is_finite()
                && step.is_finite()
                && minimum < maximum
                && value.is_none_or(|value| (minimum..=maximum).contains(&value))
                && (minimum..=maximum).contains(&initial)
                && step > 0.0
                && decimals <= 6
        );
        Self::OptionalNumber {
            value,
            initial,
            minimum,
            maximum,
            step,
            decimals,
            unit: unit.into(),
        }
    }

    pub fn choice(selected: usize, choices: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let choices: Vec<String> = choices.into_iter().map(Into::into).collect();
        assert!(!choices.is_empty() && selected < choices.len());
        assert!(choices.iter().all(|choice| !choice.is_empty()));
        Self::Choice { selected, choices }
    }

    pub fn toggle(value: bool, off_label: impl Into<String>, on_label: impl Into<String>) -> Self {
        let off_label = off_label.into();
        let on_label = on_label.into();
        assert!(!off_label.is_empty() && !on_label.is_empty());
        Self::Toggle {
            value,
            off_label,
            on_label,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Self::Number {
                value,
                decimals,
                unit,
                ..
            } => with_unit(format!("{value:.decimals$}"), unit),
            Self::OptionalNumber {
                value,
                decimals,
                unit,
                ..
            } => value.map_or_else(
                || "INHERIT".into(),
                |value| with_unit(format!("{value:.decimals$}"), unit),
            ),
            Self::Integer { value, unit, .. } => with_unit(value.to_string(), unit),
            Self::Choice { selected, choices } => choices[*selected].clone(),
            Self::Toggle {
                value,
                off_label,
                on_label,
            } => {
                if *value {
                    on_label.clone()
                } else {
                    off_label.clone()
                }
            }
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number { value, .. } => Some(*value),
            Self::OptionalNumber { value, .. } => *value,
            Self::Integer { value, .. } => Some(*value as f64),
            Self::Choice { selected, .. } => Some(*selected as f64),
            Self::Toggle { value, .. } => Some(u8::from(*value) as f64),
        }
    }

    pub fn as_optional_f64(&self) -> Option<Option<f64>> {
        match self {
            Self::OptionalNumber { value, .. } => Some(*value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Integer { value, .. } => Some(*value),
            _ => None,
        }
    }

    pub fn choice_index(&self) -> Option<usize> {
        match self {
            Self::Choice { selected, .. } => Some(*selected),
            _ => None,
        }
    }

    fn adjust(&mut self, delta: isize) -> bool {
        if delta == 0 {
            return false;
        }
        match self {
            Self::Number {
                value,
                minimum,
                maximum,
                step,
                ratio,
                ..
            } => {
                let next = if let Some(ratio) = *ratio {
                    // `step` is the fraction of the whole span one turn
                    // covers, so the detents are the same count as a linear
                    // control's and each is the same ratio wide.
                    let turns = (1.0 / *step).max(1.0);
                    let position = if *value > 0.0 && *minimum > 0.0 {
                        (*value / *minimum).ln() / ratio.ln()
                    } else {
                        0.0
                    };
                    let index = (position * turns).round() + delta as f64;
                    (*minimum * ratio.powf(index / turns)).clamp(*minimum, *maximum)
                } else {
                    let index = ((*value - *minimum) / *step).round() + delta as f64;
                    (*minimum + index * *step).clamp(*minimum, *maximum)
                };
                if (next - *value).abs() <= f64::EPSILON {
                    return false;
                }
                *value = next;
            }
            Self::OptionalNumber {
                value,
                initial,
                minimum,
                maximum,
                step,
                ..
            } => {
                let Some(current) = *value else {
                    *value = Some(*initial);
                    return true;
                };
                let index = ((current - *minimum) / *step).round() + delta as f64;
                if index < 0.0 {
                    *value = None;
                    return true;
                }
                let next = (*minimum + index * *step).clamp(*minimum, *maximum);
                if (next - current).abs() <= f64::EPSILON {
                    return false;
                }
                *value = Some(next);
            }
            Self::Integer {
                value,
                minimum,
                maximum,
                step,
                ..
            } => {
                let offset = (*value - *minimum) / *step;
                let next_offset = offset.saturating_add(delta as i64);
                let next = minimum
                    .saturating_add(next_offset.saturating_mul(*step))
                    .clamp(*minimum, *maximum);
                if next == *value {
                    return false;
                }
                *value = next;
            }
            Self::Choice { selected, choices } => {
                let next = (*selected as isize + delta)
                    .clamp(0, choices.len().saturating_sub(1) as isize)
                    as usize;
                if next == *selected {
                    return false;
                }
                *selected = next;
            }
            Self::Toggle { value, .. } => {
                let next = delta > 0;
                if next == *value {
                    return false;
                }
                *value = next;
            }
        }
        true
    }
}

fn with_unit(value: String, unit: &str) -> String {
    if unit.is_empty() {
        value
    } else {
        format!("{value} {unit}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueItem {
    id: String,
    label: String,
    value: EditableValue,
}

impl ValueItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>, value: EditableValue) -> Self {
        let id = id.into();
        let label = label.into();
        assert!(!id.is_empty() && !label.is_empty());
        Self { id, label, value }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn value(&self) -> &EditableValue {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueCarousel {
    id: ComponentId,
    items: Vec<ValueItem>,
    selected: usize,
    state: VisualState,
    editor: EditorState,
    original_value: Option<EditableValue>,
}

impl ValueCarousel {
    pub fn new(id: impl Into<String>, items: impl IntoIterator<Item = ValueItem>) -> Self {
        let items: Vec<ValueItem> = items.into_iter().collect();
        assert!(!items.is_empty());
        Self {
            id: ComponentId::new(id),
            items,
            selected: 0,
            state: VisualState::Normal,
            editor: EditorState::default(),
            original_value: None,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_item(&self) -> &ValueItem {
        &self.items[self.selected]
    }

    pub fn is_editing(&self) -> bool {
        self.editor.mode() == EditorMode::Edit
    }

    pub fn set_selected(&mut self, selected: usize) {
        assert!(!self.is_editing());
        assert!(selected < self.items.len());
        self.selected = selected;
    }

    fn move_selection(&mut self, delta: isize) {
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.items.len() as isize) as usize;
    }

    fn event(&self, event: fn(ComponentId) -> ComponentEvent) -> ComponentEvent {
        event(self.id.clone())
    }
}

impl Component for ValueCarousel {
    fn id(&self) -> &ComponentId {
        &self.id
    }

    fn state(&self) -> VisualState {
        self.state
    }

    fn set_focused(&mut self, focused: bool) {
        self.state = if focused {
            VisualState::Focused
        } else {
            VisualState::Normal
        };
    }

    fn handle(&mut self, input: Input) -> ComponentEvent {
        if self.state != VisualState::Focused {
            return ComponentEvent::Ignored;
        }

        match self.editor.handle(input) {
            EditorCommand::BeginEdit => {
                self.original_value = Some(self.items[self.selected].value.clone());
                return self.event(ComponentEvent::EditBegan);
            }
            EditorCommand::CommitEdit => {
                self.original_value = None;
                return self.event(ComponentEvent::EditCommitted);
            }
            EditorCommand::CancelEdit => {
                if let Some(original) = self.original_value.take() {
                    self.items[self.selected].value = original;
                }
                return self.event(ComponentEvent::EditCancelled);
            }
            EditorCommand::ExitEditor => return self.event(ComponentEvent::ExitRequested),
            EditorCommand::Ignored => {}
        }

        let delta = match input {
            Input::Button2 | Input::EncoderLeft => -1,
            Input::Button3 | Input::EncoderRight => 1,
            _ => return ComponentEvent::Ignored,
        };
        if self.is_editing() {
            if self.items[self.selected].value.adjust(delta) {
                self.event(ComponentEvent::Changed)
            } else {
                ComponentEvent::Ignored
            }
        } else {
            self.move_selection(delta);
            self.event(ComponentEvent::Changed)
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let item = self.selected_item();
        let navigation_focus = self.state == VisualState::Focused && !self.is_editing();
        let edit_focus = self.state == VisualState::Focused && self.is_editing();
        render_centered(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            item.label(),
            if navigation_focus {
                Style::FOCUSED
            } else {
                Style::NORMAL
            },
        );
        if area.height >= 2 {
            render_centered(
                frame,
                Rect::new(area.x, area.y + 1, area.width, 1),
                &item.value().display(),
                if edit_focus {
                    Style::FOCUSED
                } else {
                    Style::SECONDARY
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A control spanning three orders of magnitude is turned by ratio.
    ///
    /// A room runs 45 to 45000 cubic metres. Stepping that by its declared
    /// cubic metre would take forty-five thousand presses to cross; stepping
    /// it by a hundredth of the span would jump from a cupboard to a hall on
    /// the first one. Equal ratios cross it in a hundred turns and give the
    /// same resolution at both ends, which is how the size is heard.
    #[test]
    fn a_logarithmic_carousel_turns_by_ratio_not_by_step() {
        let mut room = EditableValue::number_logarithmic(45.0, 45.0, 45_000.0, 0.01, 0, "m3");

        // One turn off the bottom is a few cubic metres, not four hundred.
        assert!(room.adjust(1));
        let first = room.as_f64().unwrap();
        assert!(
            (3.0..5.0).contains(&(first - 45.0)),
            "first turn to {first}"
        );

        // The same turn near the top moves by hundreds, and by the same ratio.
        let mut hall = EditableValue::number_logarithmic(45_000.0, 45.0, 45_000.0, 0.01, 0, "m3");
        assert!(hall.adjust(-1));
        let down = hall.as_f64().unwrap();
        assert!(
            ((first / 45.0) - (45_000.0 / down)).abs() < 1e-9,
            "both ends must move by one ratio: {first} and {down}"
        );

        // A hundred turns crosses the whole span, and it stops at the ends.
        let mut sweep = EditableValue::number_logarithmic(45.0, 45.0, 45_000.0, 0.01, 0, "m3");
        for _ in 0..100 {
            sweep.adjust(1);
        }
        assert!((sweep.as_f64().unwrap() - 45_000.0).abs() < 1.0);
        assert!(!sweep.adjust(1), "the top is the top");
    }

    /// A linear control is untouched by any of that.
    #[test]
    fn a_linear_carousel_still_adds_its_step() {
        let mut decibels = EditableValue::number(4.5, 0.0, 30.0, 0.1, 1, "dB");
        assert!(decibels.adjust(1));
        assert!((decibels.as_f64().unwrap() - 4.6).abs() < 1e-9);
        assert!(decibels.adjust(-2));
        assert!((decibels.as_f64().unwrap() - 4.4).abs() < 1e-9);
    }
    use crate::TextFallback;

    fn carousel() -> ValueCarousel {
        let mut carousel = ValueCarousel::new(
            "envelope",
            [
                ValueItem::new(
                    "attack",
                    "ATTACK",
                    EditableValue::number(0.0, 0.0, 5.0, 0.01, 2, "s"),
                ),
                ValueItem::new(
                    "sustain",
                    "SUSTAIN",
                    EditableValue::number(1.0, 0.0, 1.0, 0.01, 2, "x"),
                ),
            ],
        );
        carousel.set_focused(true);
        carousel
    }

    fn rows(carousel: &ValueCarousel) -> [String; 2] {
        let mut frame = Frame::new(18, 2);
        carousel.render(&mut frame, Rect::new(0, 0, 18, 2));
        [
            TextFallback::default().row(&frame, 0),
            TextFallback::default().row(&frame, 1),
        ]
    }

    #[test]
    fn focus_moves_from_name_to_value_during_editing() {
        let mut carousel = carousel();
        assert_eq!(rows(&carousel)[0], "[     ATTACK     ]");
        assert_eq!(rows(&carousel)[1], "      0.00 s      ");

        assert!(matches!(
            carousel.handle(Input::Button1),
            ComponentEvent::EditBegan(_)
        ));
        assert_eq!(rows(&carousel)[0], "      ATTACK      ");
        assert_eq!(rows(&carousel)[1], "[     0.00 s     ]");
    }

    #[test]
    fn ok_commits_and_back_restores_the_original_value() {
        let mut carousel = carousel();
        carousel.handle(Input::Button1);
        carousel.handle(Input::EncoderRight);
        assert_eq!(carousel.selected_item().value().as_f64(), Some(0.01));
        assert!(matches!(
            carousel.handle(Input::Button4),
            ComponentEvent::EditCancelled(_)
        ));
        assert_eq!(carousel.selected_item().value().as_f64(), Some(0.0));
        assert!(!carousel.is_editing());

        carousel.handle(Input::Button1);
        carousel.handle(Input::Button3);
        assert!(matches!(
            carousel.handle(Input::Button1),
            ComponentEvent::EditCommitted(_)
        ));
        assert_eq!(carousel.selected_item().value().as_f64(), Some(0.01));
    }

    #[test]
    fn back_exits_only_when_not_editing() {
        let mut carousel = carousel();
        assert!(matches!(
            carousel.handle(Input::Button4),
            ComponentEvent::ExitRequested(_)
        ));
    }

    #[test]
    fn navigation_changes_parameter_instead_of_value() {
        let mut carousel = carousel();
        carousel.handle(Input::EncoderRight);
        assert_eq!(carousel.selected(), 1);
        assert_eq!(carousel.selected_item().id(), "sustain");
        assert_eq!(carousel.selected_item().value().as_f64(), Some(1.0));
    }

    #[test]
    fn optional_number_can_inherit_override_and_return_to_inherit() {
        let mut value = EditableValue::optional_number(None, 5.0, 0.0, 10.0, 1.0, 1, "Hz");
        assert_eq!(value.display(), "INHERIT");
        assert!(value.adjust(1));
        assert_eq!(value.as_optional_f64(), Some(Some(5.0)));
        for _ in 0..6 {
            value.adjust(-1);
        }
        assert_eq!(value.as_optional_f64(), Some(None));
    }
}
