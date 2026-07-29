#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    Button1,
    Button2,
    Button3,
    Button4,
    EncoderLeft,
    EncoderRight,
    EncoderPress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationAction {
    Previous,
    Next,
    Back,
    Select,
}

impl Input {
    pub const ALL: [Self; 7] = [
        Self::Button1,
        Self::Button2,
        Self::Button3,
        Self::Button4,
        Self::EncoderLeft,
        Self::EncoderRight,
        Self::EncoderPress,
    ];

    pub fn default_navigation(self) -> NavigationAction {
        match self {
            Self::Button1 => NavigationAction::Select,
            Self::Button2 | Self::EncoderLeft => NavigationAction::Previous,
            Self::Button3 | Self::EncoderRight => NavigationAction::Next,
            Self::Button4 => NavigationAction::Back,
            Self::EncoderPress => NavigationAction::Select,
        }
    }
}
