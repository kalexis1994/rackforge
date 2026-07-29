use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Select {
    id: ComponentId,
    options: Vec<String>,
    selected: usize,
    open: bool,
    state: VisualState,
}

impl Select {
    pub fn new(
        id: impl Into<String>,
        options: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        assert!(!options.is_empty());
        Self {
            id: ComponentId::new(id),
            options,
            selected: 0,
            open: false,
            state: VisualState::Normal,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn set_selected(&mut self, selected: usize) {
        assert!(selected < self.options.len());
        self.selected = selected;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn move_selection(&mut self, delta: isize) {
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.options.len() as isize) as usize;
    }
}

impl Component for Select {
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
        match input {
            Input::Button2 | Input::EncoderLeft => {
                self.move_selection(-1);
                ComponentEvent::Changed(self.id.clone())
            }
            Input::Button3 | Input::EncoderRight => {
                self.move_selection(1);
                ComponentEvent::Changed(self.id.clone())
            }
            Input::Button1 | Input::EncoderPress => {
                self.open = !self.open;
                ComponentEvent::Activated(self.id.clone())
            }
            Input::Button4 if self.open => {
                self.open = false;
                ComponentEvent::Changed(self.id.clone())
            }
            _ => ComponentEvent::Ignored,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let style = if self.state == VisualState::Focused {
            Style::FOCUSED
        } else {
            Style::NORMAL
        };
        frame.fill(area, ' ', style);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let suffix = if self.open { " ^" } else { " v" };
        let available = area.width.saturating_sub(suffix.len());
        let value: String = self.options[self.selected]
            .chars()
            .take(available)
            .collect();
        let label = format!("{value}{suffix}");
        let x = area.x + area.width.saturating_sub(label.chars().count()) / 2;
        let y = area.y + area.height / 2;
        frame.write(x, y, &label, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_wraps_and_open_state_is_explicit() {
        let mut select = Select::new("instrument", ["PIANO", "PAD", "BASS"]);
        select.set_focused(true);
        assert!(matches!(
            select.handle(Input::EncoderLeft),
            ComponentEvent::Changed(_)
        ));
        assert_eq!(select.selected(), 2);
        assert!(matches!(
            select.handle(Input::EncoderPress),
            ComponentEvent::Activated(_)
        ));
        assert!(select.is_open());
        select.handle(Input::Button4);
        assert!(!select.is_open());
    }

    #[test]
    fn unfocused_select_ignores_navigation() {
        let mut select = Select::new("instrument", ["PIANO", "PAD"]);
        assert_eq!(select.handle(Input::EncoderRight), ComponentEvent::Ignored);
        assert_eq!(select.selected(), 0);
    }
}
