use iced::{
    Alignment, Font,
    widget::{Row, Text, row, text},
};

pub const FONT_BYTES: &[u8] = include_bytes!("../assets/lucide/lucide-status.ttf");
const FONT: Font = Font::with_name("lucide");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Cpu,
    Memory,
    Mic,
    MicOff,
    Headphones,
    HeadphoneOff,
    Volume,
    VolumeOff,
    Server,
    Channel,
}

impl Icon {
    pub fn view(self, size: u32) -> Text<'static> {
        let glyph = match self {
            Self::Cpu => "\u{e0a9}",
            Self::Memory => "\u{e445}",
            Self::Mic => "\u{e118}",
            Self::MicOff => "\u{e119}",
            Self::Headphones => "\u{e0f1}",
            Self::HeadphoneOff => "\u{e629}",
            Self::Volume => "\u{e1ab}",
            Self::VolumeOff => "\u{e1ac}",
            Self::Server => "\u{e153}",
            Self::Channel => "\u{e0ef}",
        };
        text(glyph).font(FONT).size(size).line_height(1.0)
    }
}

pub fn label<'a, Message: 'a>(
    icon: Icon,
    label: impl text::IntoFragment<'a>,
    size: u32,
) -> Row<'a, Message> {
    row![icon.view(size + 2), text(label).size(size)]
        .spacing(8)
        .align_y(Alignment::Center)
}
