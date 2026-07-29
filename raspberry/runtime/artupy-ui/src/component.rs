use crate::{Frame, Input, Rect};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ComponentId(String);

impl ComponentId {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        assert!(!value.is_empty());
        Self(value)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualState {
    Normal,
    Focused,
    Pressed,
    Disabled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentEvent {
    Ignored,
    Changed(ComponentId),
    Activated(ComponentId),
}

pub trait Component {
    fn id(&self) -> &ComponentId;
    fn state(&self) -> VisualState;
    fn set_focused(&mut self, focused: bool);
    fn handle(&mut self, input: Input) -> ComponentEvent;
    fn render(&self, frame: &mut Frame, area: Rect);
}
