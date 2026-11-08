//! Flexspeak's tokens. See docs/design-system.md for how each is used.

use rune::{Color, Shadow, TextStyle};
use rune_kit::{Kit, PaletteFocus, PaletteSeed};

use crate::settings::ThemeName;
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
    /// Whether this is a dark palette; overlays and wells darken or lighten accordingly.
    pub dark: bool,
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
    /// The ink of hover and press overlays: white on dark palettes, black on light ones.
    pub ink: Color,
    /// The fill of a recessed well.
    pub recess: Color,
    /// Slider knobs.
    pub knob: Color,
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

/// What sets one theme's palette apart: its tinted neutrals and its accent.
struct Palette {
    canvas: u32,
    chrome: u32,
    surface: u32,
    raised: u32,
    text: u32,
    muted: u32,
    faint: u32,
    accent: u32,
    on_accent: u32,
}

const fn palette(name: ThemeName, dark: bool) -> Palette {
    macro_rules! p {
        ($canvas:expr, $chrome:expr, $surface:expr, $raised:expr, $text:expr, $muted:expr,
         $faint:expr, $accent:expr, $on_accent:expr) => {
            Palette {
                canvas: $canvas,
                chrome: $chrome,
                surface: $surface,
                raised: $raised,
                text: $text,
                muted: $muted,
                faint: $faint,
                accent: $accent,
                on_accent: $on_accent,
            }
        };
    }
    match (name, dark) {
        (ThemeName::Flexspeak, true) => p!(
            0x1c1d20, 0x232427, 0x26272b, 0x34353a, 0xebebee, 0xa0a1a8, 0x75767e, 0x8ea8ff,
            0x0b0f1c
        ),
        (ThemeName::Flexspeak, false) => p!(
            0xf3f4f6, 0xeceef1, 0xffffff, 0xf1f2f5, 0x1c1d21, 0x5d5f68, 0x81838e, 0x264adc,
            0xffffff
        ),
        (ThemeName::Grove, true) => p!(
            0x191d1b, 0x1f2421, 0x222825, 0x2f3632, 0xe8ede9, 0x9ca69f, 0x707a73, 0x62d39b,
            0x07170f
        ),
        (ThemeName::Grove, false) => p!(
            0xf1f4f1, 0xe8ede9, 0xffffff, 0xeff3ef, 0x1a201c, 0x59625c, 0x7c857f, 0x166741,
            0xffffff
        ),
        (ThemeName::Ocean, true) => p!(
            0x171c21, 0x1d232a, 0x20272e, 0x2c353e, 0xe6edf3, 0x9aa6b2, 0x6f7b87, 0x5cb8ec,
            0x06141d
        ),
        (ThemeName::Ocean, false) => p!(
            0xeff4f8, 0xe6edf3, 0xffffff, 0xedf2f7, 0x17202a, 0x556271, 0x778491, 0x155f8f,
            0xffffff
        ),
        (ThemeName::Ember, true) => p!(
            0x1e1b19, 0x25211e, 0x292420, 0x37302b, 0xf0e9e4, 0xaaa097, 0x7d746c, 0xf39a5e,
            0x1c0d04
        ),
        (ThemeName::Ember, false) => p!(
            0xf7f3f0, 0xf0e9e4, 0xffffff, 0xf5f0ec, 0x241c17, 0x675c54, 0x8c7f75, 0x9c3b00,
            0xffffff
        ),
        (ThemeName::Iris, true) => p!(
            0x1b1a21, 0x222028, 0x26242d, 0x34313c, 0xece9f3, 0xa4a0b0, 0x787484, 0xb39dff,
            0x120a24
        ),
        (ThemeName::Iris, false) => p!(
            0xf4f2f8, 0xece9f2, 0xffffff, 0xf2f0f7, 0x1e1b26, 0x5f5a6c, 0x847f92, 0x5a3cd2,
            0xffffff
        ),
    }
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
    pub const SMALL_STRONG: TextStyle = TextStyle::new(11.5, 600.0);
    pub const CAPTION: TextStyle = TextStyle::new(11.0, 400.0);
    pub const OVERLINE: TextStyle = TextStyle::new(11.0, 600.0);
    pub const VALUE: TextStyle = TextStyle::new(15.0, 560.0).features("'tnum' 1");
    pub const BIG_VALUE: TextStyle = TextStyle::new(20.0, 600.0).features("'tnum' 1");
    pub const NUMERIC: TextStyle = TextStyle::new(11.5, 500.0).features("'tnum' 1");
}

impl Theme {
    /// `name`'s light or dark palette. Status colours are shared by every theme, darkened in
    /// light palettes so they stay readable on white.
    pub fn new(name: ThemeName, dark: bool) -> Self {
        let p = palette(name, dark);
        let neutral = Self {
            dark,
            canvas: rgb(p.canvas),
            chrome: rgb(p.chrome),
            surface: rgb(p.surface),
            raised: rgb(p.raised),
            text: rgb(p.text),
            muted: rgb(p.muted),
            faint: rgb(p.faint),
            accent: rgb(p.accent),
            on_accent: rgb(p.on_accent),
            ..Self::dark_status()
        };
        if dark {
            neutral
        } else {
            Self {
                hairline: Color::BLACK.alpha(0.09),
                bevel_top: Color::BLACK.alpha(0.07),
                bevel_bottom: Color::BLACK.alpha(0.14),
                ink: Color::BLACK,
                recess: Color::BLACK.alpha(0.035),
                knob: Color::WHITE,
                good: rgb(0x1c9354),
                fair: rgb(0xb2750f),
                poor: rgb(0xd43d55),
                unknown: rgb(0x6b7385),
                live: rgb(0xe0483e),
                cpu: rgb(0x0d8fa1),
                ram: rgb(0x7655dd),
                ..neutral
            }
        }
    }

    /// A palette from the More Themes section, over the shared edges and status colours.
    pub fn extra(colors: &crate::themes::Colors, dark: bool) -> Self {
        let base = Self::new(ThemeName::default(), dark);
        Self {
            canvas: colors.canvas,
            chrome: colors.chrome,
            surface: colors.surface,
            raised: colors.raised,
            text: colors.text,
            muted: colors.muted,
            faint: colors.faint,
            accent: colors.accent,
            on_accent: colors.on_accent,
            hairline: colors.border.unwrap_or(base.hairline),
            poor: colors.error.unwrap_or(base.poor),
            fair: colors.warning.unwrap_or(base.fair),
            good: colors.success.unwrap_or(base.good),
            ..base
        }
    }

    /// The dark palette's edges, overlays and status colours, around Flexspeak's neutrals.
    fn dark_status() -> Self {
        Self {
            dark: true,
            canvas: rgb(0x1c1d20),
            chrome: rgb(0x232427),
            surface: rgb(0x26272b),
            raised: rgb(0x34353a),
            hairline: Color::WHITE.alpha(0.08),
            bevel_top: Color::WHITE.alpha(0.10),
            bevel_bottom: Color::WHITE.alpha(0.06),
            ink: Color::WHITE,
            recess: Color::BLACK.alpha(0.18),
            knob: rgb(0xebebee),
            text: rgb(0xebebee),
            muted: rgb(0xa0a1a8),
            faint: rgb(0x75767e),
            accent: rgb(0x8ea8ff),
            on_accent: rgb(0x0b0f1c),
            good: rgb(0x6fdc9a),
            fair: rgb(0xf2c46b),
            poor: rgb(0xff7a8a),
            unknown: rgb(0x8c95ab),
            live: rgb(0xff5a52),
            cpu: rgb(0x46cfe0),
            ram: rgb(0xa98ff8),
        }
    }

    /// A hover, press or track overlay at `alpha`, readable on this palette's surfaces.
    pub fn wash(&self, alpha: f32) -> Color {
        self.ink.alpha(alpha)
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
        k.c.scrim = Color::BLACK.alpha(if self.dark { 0.6 } else { 0.32 });
        // macOS switches keep a white knob in both appearances.
        k.c.knob = Color::WHITE;
        k.c.knob_on = Color::WHITE;
        // An off switch's track, which rune-kit's light layers leave nearly invisible on chrome.
        if !self.dark {
            k.c.layer_04 = self.text.alpha(0.2);
        }
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
