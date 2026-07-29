use crate::{Component, ComponentId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FocusRing {
    ids: Vec<ComponentId>,
    focused: usize,
}

impl FocusRing {
    pub fn new(ids: impl IntoIterator<Item = ComponentId>) -> Self {
        Self {
            ids: ids.into_iter().collect(),
            focused: 0,
        }
    }

    pub fn current(&self) -> Option<&ComponentId> {
        self.ids.get(self.focused)
    }

    pub fn previous(&mut self) {
        if !self.ids.is_empty() {
            self.focused = (self.focused + self.ids.len() - 1) % self.ids.len();
        }
    }

    pub fn next(&mut self) {
        if !self.ids.is_empty() {
            self.focused = (self.focused + 1) % self.ids.len();
        }
    }

    pub fn apply(&self, components: &mut [&mut dyn Component]) {
        let current = self.current();
        for component in components {
            component.set_focused(current == Some(component.id()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_wraps_in_both_directions() {
        let mut ring = FocusRing::new([
            ComponentId::new("one"),
            ComponentId::new("two"),
            ComponentId::new("three"),
        ]);
        ring.previous();
        assert_eq!(ring.current().unwrap().as_str(), "three");
        ring.next();
        assert_eq!(ring.current().unwrap().as_str(), "one");
    }
}
