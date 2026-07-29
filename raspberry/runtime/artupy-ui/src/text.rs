use crate::{Frame, StyleRole};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextFallback {
    mark_focus: bool,
}

impl Default for TextFallback {
    fn default() -> Self {
        Self { mark_focus: true }
    }
}

impl TextFallback {
    pub fn new(mark_focus: bool) -> Self {
        Self { mark_focus }
    }

    pub fn row(&self, frame: &Frame, y: usize) -> String {
        if y >= frame.height() {
            return String::new();
        }
        let mut characters: Vec<char> = (0..frame.width())
            .map(|x| {
                frame
                    .cell(x, y)
                    .expect("coordinates are in range")
                    .character
            })
            .collect();
        if self.mark_focus {
            let mut x = 0;
            while x < frame.width() {
                let focused = frame
                    .cell(x, y)
                    .is_some_and(|cell| cell.style.role == StyleRole::Focused);
                if !focused {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < frame.width()
                    && frame
                        .cell(x, y)
                        .is_some_and(|cell| cell.style.role == StyleRole::Focused)
                {
                    x += 1;
                }
                if x - start >= 2 {
                    characters[start] = '[';
                    characters[x - 1] = ']';
                }
            }
        }
        characters.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Component, Rect, components::Button};

    #[test]
    fn focused_button_degrades_to_brackets() {
        let mut frame = Frame::new(8, 1);
        let mut button = Button::new("play", "PLAY");
        button.set_focused(true);
        button.render(&mut frame, Rect::new(0, 0, 8, 1));
        assert_eq!(TextFallback::default().row(&frame, 0), "[ PLAY ]");
    }
}
