#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Color {
    Black,
    White,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StyleRole {
    Normal,
    Focused,
    Pressed,
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Style {
    pub foreground: Color,
    pub background: Color,
    pub role: StyleRole,
}

impl Style {
    pub const NORMAL: Self = Self {
        foreground: Color::Black,
        background: Color::White,
        role: StyleRole::Normal,
    };
    pub const FOCUSED: Self = Self {
        foreground: Color::White,
        background: Color::Black,
        role: StyleRole::Focused,
    };
    pub const PRESSED: Self = Self {
        foreground: Color::White,
        background: Color::Black,
        role: StyleRole::Pressed,
    };
    pub const DISABLED: Self = Self {
        foreground: Color::Black,
        background: Color::White,
        role: StyleRole::Disabled,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub character: char,
    pub style: Style,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        assert!(width > 0 && height > 0);
        Self {
            width,
            height,
            cells: vec![
                Cell {
                    character: ' ',
                    style: Style::NORMAL,
                };
                width * height
            ],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn cell(&self, x: usize, y: usize) -> Option<Cell> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells
            .get(y.checked_mul(self.width)?.checked_add(x)?)
            .copied()
    }

    pub fn fill(&mut self, area: Rect, character: char, style: Style) {
        for y in area.y..area.y.saturating_add(area.height).min(self.height) {
            for x in area.x..area.x.saturating_add(area.width).min(self.width) {
                self.cells[y * self.width + x] = Cell { character, style };
            }
        }
    }

    pub fn write(&mut self, x: usize, y: usize, text: &str, style: Style) {
        if y >= self.height {
            return;
        }
        for (offset, character) in text.chars().enumerate() {
            let x = x + offset;
            if x >= self.width {
                break;
            }
            self.cells[y * self.width + x] = Cell { character, style };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_palette_is_white_on_black() {
        assert_eq!(Style::FOCUSED.foreground, Color::White);
        assert_eq!(Style::FOCUSED.background, Color::Black);
        assert_eq!(Style::FOCUSED.role, StyleRole::Focused);
    }

    #[test]
    fn drawing_is_clipped_to_the_frame() {
        let mut frame = Frame::new(4, 2);
        frame.fill(Rect::new(3, 1, 8, 8), '#', Style::FOCUSED);
        frame.write(2, 0, "LONG", Style::NORMAL);
        assert_eq!(frame.cell(2, 0).unwrap().character, 'L');
        assert_eq!(frame.cell(3, 0).unwrap().character, 'O');
        assert_eq!(frame.cell(3, 1).unwrap().character, '#');
        assert_eq!(frame.cell(4, 0), None);
        assert_eq!(frame.cell(0, 2), None);
    }
}
