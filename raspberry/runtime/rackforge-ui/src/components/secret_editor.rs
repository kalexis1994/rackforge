use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};
use std::fmt;
use zeroize::Zeroize;

use super::render_centered;

const FIRST_GLYPH: u8 = b' ';
const LAST_GLYPH: u8 = b'~';

#[derive(Clone, Eq, PartialEq)]
pub struct SecretValue(Vec<u8>);

impl SecretValue {
    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).expect("secret editor stores printable ASCII")
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct SecretEditor {
    id: ComponentId,
    label: String,
    value: Vec<u8>,
    original: Option<Vec<u8>>,
    cursor: usize,
    maximum_length: usize,
    state: VisualState,
}

impl fmt::Debug for SecretEditor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecretEditor")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("value", &"[REDACTED]")
            .field("cursor", &self.cursor)
            .field("maximum_length", &self.maximum_length)
            .field("state", &self.state)
            .finish()
    }
}

impl SecretEditor {
    pub fn new(id: impl Into<String>, label: impl Into<String>, maximum_length: usize) -> Self {
        let label = label.into();
        assert!(!label.trim().is_empty());
        assert!((1..=128).contains(&maximum_length));
        Self {
            id: ComponentId::new(id),
            label,
            value: vec![b'A'],
            original: None,
            cursor: 0,
            maximum_length,
            state: VisualState::Normal,
        }
    }

    pub fn is_editing(&self) -> bool {
        self.original.is_some()
    }

    pub fn clear(&mut self) {
        self.value.zeroize();
        self.value.clear();
        self.value.push(b'A');
        if let Some(mut original) = self.original.take() {
            original.zeroize();
        }
        self.cursor = 0;
    }

    pub fn take_secret(&mut self) -> SecretValue {
        debug_assert!(!self.is_editing());
        let value = std::mem::replace(&mut self.value, vec![b'A']);
        self.cursor = 0;
        SecretValue(value)
    }

    fn event(&self, event: fn(ComponentId) -> ComponentEvent) -> ComponentEvent {
        event(self.id.clone())
    }

    fn begin(&mut self) -> ComponentEvent {
        if self.is_editing() {
            return ComponentEvent::Ignored;
        }
        self.original = Some(self.value.clone());
        self.event(ComponentEvent::EditBegan)
    }

    fn commit(&mut self) -> ComponentEvent {
        let Some(mut original) = self.original.take() else {
            return ComponentEvent::Ignored;
        };
        original.zeroize();
        self.event(ComponentEvent::EditCommitted)
    }

    fn cancel(&mut self) -> ComponentEvent {
        let Some(original) = self.original.take() else {
            return self.event(ComponentEvent::ExitRequested);
        };
        self.value.zeroize();
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
            self.value.push(b'A');
            self.cursor += 1;
        }
        self.event(ComponentEvent::Changed)
    }

    fn cycle_glyph(&mut self, delta: isize) -> ComponentEvent {
        let span = i16::from(LAST_GLYPH - FIRST_GLYPH + 1);
        let current = i16::from(self.value[self.cursor] - FIRST_GLYPH);
        let next = (current + delta as i16).rem_euclid(span) as u8;
        self.value[self.cursor] = FIRST_GLYPH + next;
        self.event(ComponentEvent::Changed)
    }

    fn delete(&mut self) -> ComponentEvent {
        if self.value.len() == 1 {
            self.value[0].zeroize();
            self.value[0] = b'A';
        } else {
            let mut removed = self.value.remove(self.cursor);
            removed.zeroize();
            self.cursor = self.cursor.min(self.value.len() - 1);
        }
        self.event(ComponentEvent::Changed)
    }

    fn masked_value(&self, width: usize) -> String {
        let available = width.saturating_sub(2).max(1);
        let half = available / 2;
        let start = self
            .cursor
            .saturating_sub(half)
            .min(self.value.len().saturating_sub(available));
        let end = (start + available).min(self.value.len());
        let mut rendered = String::new();
        for (index, byte) in self.value[start..end].iter().enumerate() {
            if start + index == self.cursor {
                rendered.push('[');
                rendered.push(*byte as char);
                rendered.push(']');
            } else {
                rendered.push('*');
            }
        }
        rendered
    }
}

impl Drop for SecretEditor {
    fn drop(&mut self) {
        self.value.zeroize();
        if let Some(original) = self.original.as_mut() {
            original.zeroize();
        }
    }
}

impl Component for SecretEditor {
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
            Input::Button4Long | Input::HomeChord => ComponentEvent::Ignored,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let counter = self
            .is_editing()
            .then(|| format!(" {}/{}", self.cursor + 1, self.maximum_length))
            .unwrap_or_default();
        let label_width = area.width.saturating_sub(counter.len());
        frame.fill(Rect::new(area.x, area.y, area.width, 1), ' ', Style::NORMAL);
        frame.write(
            area.x,
            area.y,
            &self.label.chars().take(label_width).collect::<String>(),
            Style::NORMAL,
        );
        frame.write(
            area.x + area.width.saturating_sub(counter.len()),
            area.y,
            &counter,
            Style::NORMAL,
        );
        if area.height >= 2 {
            let text = if self.is_editing() {
                self.masked_value(area.width)
            } else {
                "PRESS OK".to_owned()
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
    fn masks_every_character_except_the_cursor_and_redacts_debug() {
        let mut editor = SecretEditor::new("wifi-password", "PASSWORD", 64);
        editor.set_focused(true);
        editor.handle(Input::Button1);
        editor.handle(Input::Button3Long);
        let mut frame = Frame::new(18, 2);
        editor.render(&mut frame, Rect::new(0, 0, 18, 2));
        let rendered = TextFallback::default().row(&frame, 1);
        assert!(rendered.contains("*[A]"));
        assert!(!format!("{editor:?}").contains("AA"));
    }

    #[test]
    fn committed_secret_is_taken_and_editor_is_reset() {
        let mut editor = SecretEditor::new("wifi-password", "PASSWORD", 64);
        editor.set_focused(true);
        editor.handle(Input::Button1);
        editor.handle(Input::Button3Long);
        editor.handle(Input::Button1);
        let secret = editor.take_secret();
        assert_eq!(secret.expose(), "AA");
        assert_eq!(format!("{secret:?}"), "SecretValue([REDACTED])");
        assert!(!editor.is_editing());
    }
}
