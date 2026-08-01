use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};

use super::render_centered;

const GLYPHS: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.&+";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEditor {
    id: ComponentId,
    label: String,
    value: Vec<u8>,
    original: Option<Vec<u8>>,
    cursor: usize,
    maximum_length: usize,
    state: VisualState,
}

impl TextEditor {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        value: impl AsRef<str>,
        maximum_length: usize,
    ) -> Self {
        let label = label.into();
        assert!(!label.trim().is_empty());
        assert!((1..=64).contains(&maximum_length));
        let value = normalize(value.as_ref(), maximum_length);
        assert!(!value.is_empty());
        Self {
            id: ComponentId::new(id),
            label,
            cursor: value.len().saturating_sub(1),
            value,
            original: None,
            maximum_length,
            state: VisualState::Normal,
        }
    }

    pub fn value(&self) -> &str {
        std::str::from_utf8(&self.value).expect("text editor stores only ASCII glyphs")
    }

    pub fn set_value(&mut self, value: impl AsRef<str>) {
        assert!(!self.is_editing());
        let value = normalize(value.as_ref(), self.maximum_length);
        assert!(!value.is_empty());
        self.value = value;
        self.cursor = self.cursor.min(self.value.len().saturating_sub(1));
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_editing(&self) -> bool {
        self.original.is_some()
    }

    fn event(&self, event: fn(ComponentId) -> ComponentEvent) -> ComponentEvent {
        event(self.id.clone())
    }

    fn begin(&mut self) -> ComponentEvent {
        if self.is_editing() {
            return ComponentEvent::Ignored;
        }
        self.original = Some(self.value.clone());
        self.cursor = self.cursor.min(self.value.len().saturating_sub(1));
        self.event(ComponentEvent::EditBegan)
    }

    fn commit(&mut self) -> ComponentEvent {
        if !self.is_editing() {
            return ComponentEvent::Ignored;
        }
        while self.value.last() == Some(&b' ') {
            self.value.pop();
        }
        if self.value.is_empty() {
            self.value.push(b'A');
        }
        self.cursor = self.cursor.min(self.value.len().saturating_sub(1));
        self.original = None;
        self.event(ComponentEvent::EditCommitted)
    }

    fn cancel(&mut self) -> ComponentEvent {
        let Some(original) = self.original.take() else {
            return self.event(ComponentEvent::ExitRequested);
        };
        self.value = original;
        self.cursor = self.cursor.min(self.value.len().saturating_sub(1));
        self.event(ComponentEvent::EditCancelled)
    }

    fn move_cursor(&mut self, delta: isize) -> ComponentEvent {
        if delta.is_negative() {
            self.cursor = self.cursor.saturating_sub(1);
        } else if self.cursor + 1 < self.value.len() {
            self.cursor += 1;
        } else if self.value.len() < self.maximum_length {
            self.value.push(b' ');
            self.cursor += 1;
        }
        self.event(ComponentEvent::Changed)
    }

    fn cycle_glyph(&mut self, delta: isize) -> ComponentEvent {
        let current = self.value[self.cursor];
        let index = GLYPHS
            .iter()
            .position(|glyph| *glyph == current)
            .unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(GLYPHS.len() as isize) as usize;
        self.value[self.cursor] = GLYPHS[next];
        self.event(ComponentEvent::Changed)
    }

    fn delete(&mut self) -> ComponentEvent {
        if self.value.len() == 1 {
            self.value[0] = b' ';
            return self.event(ComponentEvent::Changed);
        }
        self.value.remove(self.cursor);
        self.cursor = self.cursor.min(self.value.len() - 1);
        self.event(ComponentEvent::Changed)
    }

    fn marked_value(&self, width: usize) -> String {
        let available = width.saturating_sub(2).max(1);
        let half = available / 2;
        let start = self
            .cursor
            .saturating_sub(half)
            .min(self.value.len().saturating_sub(available));
        let end = (start + available).min(self.value.len());
        let mut rendered = String::new();
        for (index, byte) in self.value[start..end].iter().enumerate() {
            let absolute = start + index;
            if absolute == self.cursor {
                rendered.push('[');
                rendered.push(*byte as char);
                rendered.push(']');
            } else {
                rendered.push(*byte as char);
            }
        }
        rendered
    }
}

impl Component for TextEditor {
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
        if !self.is_editing() {
            return match input {
                Input::Button1 | Input::EncoderPress => self.begin(),
                Input::Button4 => self.cancel(),
                _ => ComponentEvent::Ignored,
            };
        }
        match input {
            Input::Button1 | Input::EncoderPress => self.commit(),
            Input::Button2 | Input::EncoderLeft => self.cycle_glyph(-1),
            Input::Button3 | Input::EncoderRight => self.cycle_glyph(1),
            Input::Button4 => self.cancel(),
            Input::Button1Long => self.delete(),
            Input::Button2Long => self.move_cursor(-1),
            Input::Button3Long => self.move_cursor(1),
            Input::Button4Long
            | Input::HomeChord
            | Input::KeyboardParts
            | Input::KeyboardPartsLong
            | Input::KeyboardSplitNote(_) => ComponentEvent::Ignored,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let counter = if self.is_editing() {
            format!(" {}/{}", self.cursor + 1, self.maximum_length)
        } else {
            String::new()
        };
        let label_width = area.width.saturating_sub(counter.len());
        let label: String = self.label.chars().take(label_width).collect();
        frame.fill(Rect::new(area.x, area.y, area.width, 1), ' ', Style::NORMAL);
        frame.write(area.x, area.y, &label, Style::NORMAL);
        frame.write(
            area.x + area.width.saturating_sub(counter.len()),
            area.y,
            &counter,
            Style::NORMAL,
        );
        if area.height >= 2 {
            if self.is_editing() {
                render_centered(
                    frame,
                    Rect::new(area.x, area.y + 1, area.width, 1),
                    &self.marked_value(area.width),
                    Style::NORMAL,
                );
            } else {
                render_centered(
                    frame,
                    Rect::new(area.x, area.y + 1, area.width, 1),
                    self.value(),
                    if self.state == VisualState::Focused {
                        Style::FOCUSED
                    } else {
                        Style::NORMAL
                    },
                );
            }
        }
    }
}

fn normalize(value: &str, maximum_length: usize) -> Vec<u8> {
    value
        .bytes()
        .map(|byte| byte.to_ascii_uppercase())
        .map(|byte| if GLYPHS.contains(&byte) { byte } else { b' ' })
        .take(maximum_length)
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextFallback;

    fn editor() -> TextEditor {
        let mut editor = TextEditor::new("name", "NAME", "CUSTOM 002", 16);
        editor.set_focused(true);
        editor
    }

    #[test]
    fn short_arrows_change_the_character_and_short_ok_commits() {
        let mut editor = editor();
        assert!(matches!(
            editor.handle(Input::Button1),
            ComponentEvent::EditBegan(_)
        ));
        editor.cursor = 0;
        editor.handle(Input::Button3);
        assert_eq!(editor.value(), "DUSTOM 002");
        editor.handle(Input::Button1);
        assert!(!editor.is_editing());
        assert_eq!(editor.value(), "DUSTOM 002");
    }

    #[test]
    fn back_restores_the_whole_original_value() {
        let mut editor = editor();
        editor.handle(Input::Button1);
        editor.handle(Input::EncoderRight);
        editor.handle(Input::Button1Long);
        assert_ne!(editor.value(), "CUSTOM 002");
        assert!(matches!(
            editor.handle(Input::Button4),
            ComponentEvent::EditCancelled(_)
        ));
        assert_eq!(editor.value(), "CUSTOM 002");
    }

    #[test]
    fn long_arrows_move_and_long_ok_deletes() {
        let mut editor = editor();
        editor.handle(Input::Button1);
        assert_eq!(editor.cursor(), "CUSTOM 002".len() - 1);
        editor.handle(Input::Button2Long);
        assert_eq!(editor.cursor(), "CUSTOM 002".len() - 2);
        editor.handle(Input::Button3Long);
        assert_eq!(editor.cursor(), "CUSTOM 002".len() - 1);
        let length = editor.value().len();
        editor.handle(Input::Button1Long);
        assert_eq!(editor.value().len(), length - 1);
    }

    #[test]
    fn long_next_at_the_end_opens_a_new_character_slot() {
        let mut editor = editor();
        editor.handle(Input::Button1);
        let length = editor.value().len();
        editor.handle(Input::Button3Long);
        assert_eq!(editor.value().len(), length + 1);
        assert_eq!(editor.cursor(), length);
    }

    #[test]
    fn renders_an_explicit_cursor_on_text_only_surfaces() {
        let mut editor = editor();
        editor.handle(Input::Button1);
        let mut frame = Frame::new(18, 2);
        editor.render(&mut frame, Rect::new(0, 0, 18, 2));
        assert!(TextFallback::default().row(&frame, 1).contains("[2]"));
    }
}
