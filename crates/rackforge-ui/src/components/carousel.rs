use crate::{Component, ComponentEvent, ComponentId, Frame, Input, Rect, Style, VisualState};

use super::render_centered;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CarouselItem {
    label: String,
    detail: String,
}

impl CarouselItem {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        let label = label.into();
        let detail = detail.into();
        assert!(!label.is_empty());
        assert!(!detail.is_empty());
        Self { label, detail }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimpleCarousel {
    id: ComponentId,
    items: Vec<CarouselItem>,
    selected: usize,
    state: VisualState,
}

impl SimpleCarousel {
    pub fn new(id: impl Into<String>, items: impl IntoIterator<Item = CarouselItem>) -> Self {
        let items: Vec<CarouselItem> = items.into_iter().collect();
        assert!(!items.is_empty());
        Self {
            id: ComponentId::new(id),
            items,
            selected: 0,
            state: VisualState::Normal,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_item(&self) -> &CarouselItem {
        &self.items[self.selected]
    }

    pub fn set_selected(&mut self, selected: usize) {
        assert!(selected < self.items.len());
        self.selected = selected;
    }

    fn move_selection(&mut self, delta: isize) {
        self.selected =
            (self.selected as isize + delta).rem_euclid(self.items.len() as isize) as usize;
    }
}

impl Component for SimpleCarousel {
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
            Input::Button1 | Input::EncoderPress => ComponentEvent::Activated(self.id.clone()),
            _ => ComponentEvent::Ignored,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let item = self.selected_item();
        let name_style = if self.state == VisualState::Focused {
            Style::FOCUSED
        } else {
            Style::NORMAL
        };
        render_centered(
            frame,
            Rect::new(area.x, area.y, area.width, 1),
            item.label(),
            name_style,
        );
        if area.height >= 2 {
            render_centered(
                frame,
                Rect::new(area.x, area.y + 1, area.width, 1),
                item.detail(),
                Style::SECONDARY,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextFallback;

    fn carousel() -> SimpleCarousel {
        let mut carousel = SimpleCarousel::new(
            "mode",
            [
                CarouselItem::new("LIVE", "Ready set"),
                CarouselItem::new("PLAY", "Browse plugins"),
                CarouselItem::new("CONFIG", "System setup"),
            ],
        );
        carousel.set_focused(true);
        carousel
    }

    #[test]
    fn renders_name_on_primary_line_and_detail_on_secondary_line() {
        let carousel = carousel();
        let mut frame = Frame::new(18, 2);
        carousel.render(&mut frame, Rect::new(0, 0, 18, 2));
        assert_eq!(TextFallback::default().row(&frame, 0), "[      LIVE      ]");
        assert_eq!(TextFallback::default().row(&frame, 1), "    Ready set     ");
        assert_eq!(frame.cell(0, 0).unwrap().style, Style::FOCUSED);
        assert_eq!(frame.cell(0, 1).unwrap().style, Style::SECONDARY);
    }

    #[test]
    fn navigation_wraps() {
        let mut carousel = carousel();
        assert!(matches!(
            carousel.handle(Input::Button2),
            ComponentEvent::Changed(_)
        ));
        assert_eq!(carousel.selected(), 2);
    }
}
