mod button;
mod carousel;
mod select;
mod value_carousel;

pub use button::Button;
pub use carousel::{CarouselItem, SimpleCarousel};
pub use select::Select;
pub use value_carousel::{EditableValue, ValueCarousel, ValueItem};

use crate::{Frame, Rect, Style};

fn render_centered(frame: &mut Frame, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.fill(area, ' ', style);
    let padding = usize::from(area.width >= 3);
    let available = area.width.saturating_sub(padding * 2).max(1);
    let text: String = text.chars().take(available).collect();
    let x = area.x + area.width.saturating_sub(text.chars().count()) / 2;
    frame.write(x, area.y + area.height / 2, &text, style);
}
