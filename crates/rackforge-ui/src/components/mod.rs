mod button;
mod carousel;
mod confirmation_dialog;
mod secret_editor;
mod select;
mod spinner;
mod text_editor;
mod value_carousel;

pub use button::Button;
pub use carousel::{CarouselItem, SimpleCarousel};
pub use confirmation_dialog::ConfirmationDialog;
pub use secret_editor::{SecretEditor, SecretValue};
pub use select::Select;
pub use spinner::Spinner;
pub use text_editor::TextEditor;
pub use value_carousel::{EditableValue, ValueCarousel, ValueItem};

use crate::{Frame, Rect, Style};

/// Cuts `text` to `columns`, marking the cut when there is one.
///
/// The display is eighteen columns wide and rejects anything outside ASCII --
/// a single `Ñ` is refused outright -- so the mark is three full stops and not
/// an ellipsis character. A hard cut in the middle of a word reads as a name
/// that happens to end there; the stops say the name goes on.
pub fn truncated(text: &str, columns: usize) -> String {
    if text.chars().count() <= columns {
        return text.to_string();
    }
    if columns <= ELLIPSIS.len() {
        return text.chars().take(columns).collect();
    }
    let kept: String = text.chars().take(columns - ELLIPSIS.len()).collect();
    format!("{}{ELLIPSIS}", kept.trim_end())
}

pub const ELLIPSIS: &str = "...";

/// How many columns a centred component really has for its text.
///
/// One column is kept clear on each side, which is where a focused carousel
/// puts the marks that say it is the one the encoder is turning. Anything
/// that decorates a label before handing it over -- the brackets on the
/// program you are playing -- has to size itself against this and not against
/// the panel, or its own last character falls off the edge.
pub fn text_columns(width: usize) -> usize {
    let padding = usize::from(width >= 3);
    width.saturating_sub(padding * 2).max(1)
}

fn render_centered(frame: &mut Frame, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.fill(area, ' ', style);
    let text = truncated(text, text_columns(area.width));
    let x = area.x + area.width.saturating_sub(text.chars().count()) / 2;
    frame.write(x, area.y + area.height / 2, &text, style);
}
