use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};

use super::render_centered;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmationDialog {
    id: ComponentId,
    prompt: String,
    choices: Vec<String>,
    selected: usize,
    state: VisualState,
}

impl ConfirmationDialog {
    pub fn new(
        id: impl Into<String>,
        prompt: impl Into<String>,
        choices: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let prompt = prompt.into();
        let choices = choices.into_iter().map(Into::into).collect::<Vec<_>>();
        assert!(!prompt.trim().is_empty());
        assert!(!choices.is_empty());
        assert!(choices.iter().all(|choice| !choice.trim().is_empty()));
        Self {
            id: ComponentId::new(id),
            prompt,
            choices,
            selected: 0,
            state: VisualState::Normal,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn set_selected(&mut self, selected: usize) {
        assert!(selected < self.choices.len());
        self.selected = selected;
    }

    fn move_selection(&mut self, delta: isize) -> ComponentEvent {
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.choices.len() as isize) as usize;
        ComponentEvent::Changed(self.id.clone())
    }
}

impl Component for ConfirmationDialog {
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
            Input::Button2 | Input::EncoderLeft => self.move_selection(-1),
            Input::Button3 | Input::EncoderRight => self.move_selection(1),
            Input::Button1 | Input::EncoderPress => ComponentEvent::Activated(self.id.clone()),
            Input::Button4 => ComponentEvent::ExitRequested(self.id.clone()),
            _ => ComponentEvent::Ignored,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        render_centered(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            &self.prompt,
            Style::NORMAL,
        );
        if area.height >= 2 {
            render_centered(
                frame,
                Rect::new(area.x, area.y + 1, area.width, 1),
                &self.choices[self.selected],
                if self.state == VisualState::Focused {
                    Style::FOCUSED
                } else {
                    Style::NORMAL
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog() -> ConfirmationDialog {
        let mut dialog = ConfirmationDialog::new("unsaved", "SAVE CHANGES?", ["SAVE", "DISCARD"]);
        dialog.set_focused(true);
        dialog
    }

    #[test]
    fn arrows_choose_and_ok_activates() {
        let mut dialog = dialog();
        assert_eq!(dialog.selected(), 0);
        assert!(matches!(
            dialog.handle(Input::Button3),
            ComponentEvent::Changed(_)
        ));
        assert_eq!(dialog.selected(), 1);
        assert!(matches!(
            dialog.handle(Input::Button1),
            ComponentEvent::Activated(_)
        ));
    }

    #[test]
    fn back_cancels_the_dialog_without_choosing() {
        let mut dialog = dialog();
        assert!(matches!(
            dialog.handle(Input::Button4),
            ComponentEvent::ExitRequested(_)
        ));
    }
}
