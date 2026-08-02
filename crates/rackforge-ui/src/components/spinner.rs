use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};

use super::render_centered;

const ASCII_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spinner {
    id: ComponentId,
    label: String,
    detail: String,
    frame: usize,
}

impl Spinner {
    pub fn ascii(
        id: impl Into<String>,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let label = label.into();
        assert!(!label.trim().is_empty());
        Self {
            id: ComponentId::new(id),
            label,
            detail: detail.into(),
            frame: 0,
        }
    }

    pub fn reset(&mut self, label: impl Into<String>) {
        let label = label.into();
        assert!(!label.trim().is_empty());
        self.label = label;
        self.frame = 0;
    }

    pub fn set_detail(&mut self, detail: impl Into<String>) {
        self.detail = detail.into();
    }

    pub fn advance(&mut self) {
        self.frame = (self.frame + 1) % ASCII_FRAMES.len();
    }

    pub fn frame(&self) -> &'static str {
        ASCII_FRAMES[self.frame]
    }
}

impl Component for Spinner {
    fn id(&self) -> &ComponentId {
        &self.id
    }

    fn state(&self) -> VisualState {
        VisualState::Normal
    }

    fn set_focused(&mut self, _focused: bool) {}

    fn handle(&mut self, _input: Input) -> ComponentEvent {
        ComponentEvent::Ignored
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        render_centered(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            &self.label,
            Style::NORMAL,
        );
        if area.height >= 2 {
            let text = if self.detail.trim().is_empty() {
                self.frame().to_owned()
            } else {
                format!("{} {}", self.detail, self.frame())
            };
            render_centered(
                frame,
                Rect::new(area.x, area.y + 1, area.width, 1),
                &text,
                Style::NORMAL,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextFallback;

    #[test]
    fn ascii_spinner_cycles_without_special_glyphs() {
        let mut spinner = Spinner::ascii("loader", "SCANNING", "PLEASE WAIT");
        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(spinner.frame());
            spinner.advance();
        }
        assert_eq!(seen, ["|", "/", "-", "\\"]);

        let mut frame = Frame::new(18, 2);
        spinner.render(&mut frame, Rect::new(0, 0, 18, 2));
        assert!(
            TextFallback::default()
                .row(&frame, 1)
                .contains("PLEASE WAIT |")
        );
    }
}
