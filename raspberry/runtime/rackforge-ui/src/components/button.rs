use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button {
    id: ComponentId,
    label: String,
    state: VisualState,
}

impl Button {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: ComponentId::new(id),
            label: label.into(),
            state: VisualState::Normal,
        }
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.state = if disabled {
            VisualState::Disabled
        } else {
            VisualState::Normal
        };
    }

    fn style(&self) -> Style {
        match self.state {
            VisualState::Normal => Style::NORMAL,
            VisualState::Focused => Style::FOCUSED,
            VisualState::Pressed => Style::PRESSED,
            VisualState::Disabled => Style::DISABLED,
        }
    }
}

impl Component for Button {
    fn id(&self) -> &ComponentId {
        &self.id
    }

    fn state(&self) -> VisualState {
        self.state
    }

    fn set_focused(&mut self, focused: bool) {
        if self.state != VisualState::Disabled {
            self.state = if focused {
                VisualState::Focused
            } else {
                VisualState::Normal
            };
        }
    }

    fn handle(&mut self, input: Input) -> ComponentEvent {
        if !matches!(self.state, VisualState::Focused | VisualState::Pressed) {
            return ComponentEvent::Ignored;
        }
        if matches!(input, Input::Button1 | Input::EncoderPress) {
            return ComponentEvent::Activated(self.id.clone());
        }
        ComponentEvent::Ignored
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let style = self.style();
        frame.fill(area, ' ', style);
        if area.width == 0 || area.height == 0 {
            return;
        }
        let label: String = self.label.chars().take(area.width).collect();
        let x = area.x + area.width.saturating_sub(label.chars().count()) / 2;
        let y = area.y + area.height / 2;
        frame.write(x, y, &label, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_button_uses_the_inverted_palette_and_activates() {
        let mut button = Button::new("ok", "OK");
        button.set_focused(true);
        let mut frame = Frame::new(4, 1);
        button.render(&mut frame, Rect::new(0, 0, 4, 1));
        assert_eq!(frame.cell(0, 0).unwrap().style, Style::FOCUSED);
        assert_eq!(
            button.handle(Input::EncoderPress),
            ComponentEvent::Activated(ComponentId::new("ok"))
        );
    }

    #[test]
    fn disabled_button_never_activates() {
        let mut button = Button::new("delete", "DEL");
        button.set_disabled(true);
        assert_eq!(button.handle(Input::Button1), ComponentEvent::Ignored);
    }

    #[test]
    fn unfocused_button_never_activates() {
        let mut button = Button::new("save", "SAVE");
        assert_eq!(button.handle(Input::EncoderPress), ComponentEvent::Ignored);
    }
}
