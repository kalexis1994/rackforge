#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    Button1,
    Button2,
    Button3,
    Button4,
    Button1Long,
    Button2Long,
    Button3Long,
    Button4Long,
    HomeChord,
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
    pub const PHYSICAL: [Self; 7] = [
        Self::Button1,
        Self::Button2,
        Self::Button3,
        Self::Button4,
        Self::EncoderLeft,
        Self::EncoderRight,
        Self::EncoderPress,
    ];

    pub const ALL: [Self; 12] = [
        Self::Button1,
        Self::Button2,
        Self::Button3,
        Self::Button4,
        Self::Button1Long,
        Self::Button2Long,
        Self::Button3Long,
        Self::Button4Long,
        Self::HomeChord,
        Self::EncoderLeft,
        Self::EncoderRight,
        Self::EncoderPress,
    ];

    pub const fn long_press(self) -> Option<Self> {
        match self {
            Self::Button1 => Some(Self::Button1Long),
            Self::Button2 => Some(Self::Button2Long),
            Self::Button3 => Some(Self::Button3Long),
            Self::Button4 => Some(Self::Button4Long),
            _ => None,
        }
    }

    pub const fn is_long_press(self) -> bool {
        matches!(
            self,
            Self::Button1Long | Self::Button2Long | Self::Button3Long | Self::Button4Long
        )
    }

    pub const fn default_navigation(self) -> Option<NavigationAction> {
        match self {
            Self::Button1 => Some(NavigationAction::Select),
            Self::Button2 | Self::EncoderLeft => Some(NavigationAction::Previous),
            Self::Button3 | Self::EncoderRight => Some(NavigationAction::Next),
            Self::Button4 => Some(NavigationAction::Back),
            Self::EncoderPress => Some(NavigationAction::Select),
            Self::Button1Long
            | Self::Button2Long
            | Self::Button3Long
            | Self::Button4Long
            | Self::HomeChord => None,
        }
    }
}
