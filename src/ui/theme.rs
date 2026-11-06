//! Flexspeak's tokens. See docs/design-system.md for how each is used.

use rune::{Color, Shadow, TextStyle};
use rune_kit::{Kit, PaletteFocus, PaletteSeed};

use crate::usage::Health;

/// sRGB `0xrrggbb` to Rune's linear colour.
pub fn rgb(hex: u32) -> Color {
    let channel = |shift: u32| {
        let c = ((hex >> shift) & 0xff) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Color {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: 1.0,
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    /// The window behind everything.
    pub canvas: Color,
    /// Header, sidebar and footer chrome.
    pub chrome: Color,
    /// Cards.
    pub surface: Color,
    /// Rows and controls on a card, menus and selected navigation.
    pub raised: Color,
    pub hairline: Color,
    /// The light top edge of a bevelled surface, and its darker bottom edge.
    pub bevel_top: Color,
    pub bevel_bottom: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub good: Color,
    pub fair: Color,
    pub poor: Color,
    pub unknown: Color,
    /// Live and recording states: a warm signal colour, used sparingly.
    pub live: Color,
    pub cpu: Color,
    pub ram: Color,
}

pub mod size {
    pub const CONTROL: f32 = 28.0;
    pub const ICON_BUTTON: f32 = 28.0;
    pub const ROW: f32 = 26.0;
    pub const GAP: f32 = 8.0;
    pub const PAGE: f32 = 24.0;
    pub const CARD_PAD: f32 = 14.0;
    /// A label column beside controls in a settings group.
    pub const FORM_LABEL: f32 = 180.0;
    /// Fields and pickers on the trailing side of a settings row.
    pub const FORM_CONTROL: f32 = 300.0;
    pub const HEADER: f32 = 52.0;
    pub const SIDEBAR: f32 = 260.0;
    pub const CHART_MIN_WIDTH: f32 = 110.0;
    pub const CHART_GAP: f32 = 12.0;
}

pub mod radius {
    pub const CARD: f32 = 10.0;
    pub const CONTROL: f32 = 6.0;
    pub const ROW: f32 = 6.0;
    pub const PILL: f32 = 999.0;
}

pub mod motion {
    use rune::Spring;
    /// Hover, press and small state changes.
    pub const QUICK: Spring = Spring::new(0.22, 0.0);
    /// Panels and layout moving into place.
    pub const SETTLE: Spring = Spring::new(0.38, 0.08);
    /// Numbers, meters and charts easing to a new reading.
    pub const READING: Spring = Spring::new(0.45, 0.0);
}

pub mod ty {
    use rune::TextStyle;
    pub const DISPLAY: TextStyle = TextStyle::new(22.0, 650.0).tracking(-0.3);
    pub const TITLE: TextStyle = TextStyle::new(17.0, 620.0).tracking(-0.2);
    pub const HEADING: TextStyle = TextStyle::new(13.0, 600.0);
    pub const BODY: TextStyle = TextStyle::new(13.0, 400.0);
    pub const BODY_STRONG: TextStyle = TextStyle::new(13.0, 560.0);
    pub const LABEL: TextStyle = TextStyle::new(13.0, 400.0);
    pub const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
    pub const CAPTION: TextStyle = TextStyle::new(11.0, 400.0);
    pub const OVERLINE: TextStyle = TextStyle::new(11.0, 600.0);
    pub const VALUE: TextStyle = TextStyle::new(15.0, 560.0).features("'tnum' 1");
    pub const BIG_VALUE: TextStyle = TextStyle::new(20.0, 600.0).features("'tnum' 1");
    pub const NUMERIC: TextStyle = TextStyle::new(11.5, 500.0).features("'tnum' 1");
}

impl Theme {
    pub fn new() -> Self {
        Self {
            canvas: rgb(0x1c1d20),
            chrome: rgb(0x232427),
            surface: rgb(0x26272b),
            raised: rgb(0x34353a),
            hairline: Color::WHITE.alpha(0.08),
            bevel_top: Color::WHITE.alpha(0.10),
            bevel_bottom: Color::WHITE.alpha(0.06),
            text: rgb(0xebebee),
            muted: rgb(0xa0a1a8),
            faint: rgb(0x75767e),
            accent: rgb(0x8ea8ff),
            on_accent: rgb(0x0b0f1c),
            good: rgb(0x6fdc9a),
            fair: rgb(0xf2c46b),
            poor: rgb(0xff7a8a),
            unknown: rgb(0x8c95ab),
            live: rgb(0xff7a3d),
            cpu: rgb(0x46cfe0),
            ram: rgb(0xa98ff8),
        }
    }

    pub fn health(&self, health: Health) -> Color {
        match health {
            Health::Good => self.good,
            Health::Fair => self.fair,
            Health::Poor => self.poor,
            Health::Unknown => self.unknown,
        }
    }

    /// rune-kit's components (fields, pickers, switches, dialogs) on Flexspeak's palette.
    pub fn kit(&self) -> Kit {
        let mut k = Kit::from_palette_focus(
            PaletteSeed {
                bg: self.surface,
                fg: self.text,
                accent: self.accent,
            },
            PaletteFocus::Accent,
        );
        k.radius.sm = 4.0;
        k.radius.md = radius::CONTROL;
        k.radius.lg = radius::CARD;
        k.c.layer_01 = self.surface;
        k.c.surface_float = self.raised;
        k.c.text_base = self.text;
        k.c.text_muted = self.muted;
        k.c.text_faint = self.faint;
        k.c.bg_accent = self.accent;
        k.c.scrim = Color::BLACK.alpha(0.6);
        k.ty.ui_compact = TextStyle::new(13.0, 400.0);
        k
    }
}

pub fn shadow(x: f32, y: f32, blur: f32, color: Color) -> Shadow {
    Shadow {
        x,
        y,
        blur,
        spread: 0.0,
        color,
        inset: false,
    }
}
