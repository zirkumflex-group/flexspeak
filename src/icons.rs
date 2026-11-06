//! Status icons the app's logic chooses between. The interface draws them from `ui::icons`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Mic,
    MicOff,
    Headphones,
    HeadphoneOff,
}

impl Icon {
    pub fn glyph(self) -> &'static rune::Icon {
        use crate::ui::icons;
        match self {
            Self::Mic => &icons::MIC,
            Self::MicOff => &icons::MIC_OFF,
            Self::Headphones => &icons::HEADPHONES,
            Self::HeadphoneOff => &icons::HEADPHONE_OFF,
        }
    }
}
