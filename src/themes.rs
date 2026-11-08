//! Themes beyond the five built in: the bundled collections and the user's own, listed under
//! `custom_themes` in the settings file.
//!
//! The format is T3 Code's: `{ id, label, appearance, colors, variants, collection }`, with the
//! same colour role names, so its themes can be pasted in. Roles Flexspeak has no use for are
//! ignored. A theme that can't be read is skipped and reported, and never affects the settings.

use std::collections::BTreeMap;

use rune::Color;
use serde::Deserialize;
use serde_json::Value;

use crate::ui::rgb;

const BUNDLED: &str = include_str!("../assets/themes.json");
const CUSTOM_COLLECTION: &str = "Custom";

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Appearance {
    Light,
    Dark,
}

type Roles = BTreeMap<String, String>;

#[derive(Deserialize, Default)]
struct Variants {
    light: Option<Roles>,
    dark: Option<Roles>,
}

#[derive(Deserialize)]
struct Collection {
    label: String,
}

#[derive(Deserialize)]
struct Definition {
    id: String,
    label: String,
    appearance: Appearance,
    #[serde(default)]
    colors: Roles,
    #[serde(default)]
    variants: Variants,
    collection: Option<Collection>,
}

/// One palette, in the roles Flexspeak draws with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colors {
    pub canvas: Color,
    pub chrome: Color,
    pub surface: Color,
    pub raised: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub border: Option<Color>,
    pub error: Option<Color>,
    pub warning: Option<Color>,
    pub success: Option<Color>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extra {
    pub id: String,
    pub label: String,
    pub collection: String,
    pub light: Option<Colors>,
    pub dark: Option<Colors>,
}

impl Extra {
    pub fn colors(&self, dark: bool) -> Option<&Colors> {
        if dark {
            self.dark.as_ref()
        } else {
            self.light.as_ref()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Library {
    pub themes: Vec<Extra>,
    /// One line for each custom theme that couldn't be used.
    pub problems: Vec<String>,
}

impl Library {
    pub fn new(custom: &[Value]) -> Self {
        let bundled: Vec<Value> = serde_json::from_str(BUNDLED).unwrap_or_default();
        let mut library = Self::default();
        for value in bundled.iter() {
            if let Ok(theme) = parse(value, "") {
                library.themes.push(theme);
            }
        }
        for (index, value) in custom.iter().enumerate() {
            match parse(value, CUSTOM_COLLECTION) {
                Ok(theme) => match library.themes.iter_mut().find(|t| t.id == theme.id) {
                    Some(existing) => *existing = theme,
                    None => library.themes.push(theme),
                },
                Err(problem) => {
                    let name = value["label"]
                        .as_str()
                        .or(value["id"].as_str())
                        .map_or_else(|| format!("Theme {}", index + 1), str::to_owned);
                    library.problems.push(format!("{name}: {problem}"));
                }
            }
        }
        library
    }

    pub fn find(&self, id: &str) -> Option<&Extra> {
        self.themes.iter().find(|theme| theme.id == id)
    }

    /// The collections in display order, with their themes.
    pub fn collections(&self) -> Vec<(&str, Vec<&Extra>)> {
        let mut groups: Vec<(&str, Vec<&Extra>)> = Vec::new();
        for theme in &self.themes {
            match groups
                .iter_mut()
                .find(|(name, _)| *name == theme.collection)
            {
                Some((_, themes)) => themes.push(theme),
                None => groups.push((theme.collection.as_str(), vec![theme])),
            }
        }
        groups
    }
}

fn parse(value: &Value, default_collection: &str) -> Result<Extra, String> {
    let definition: Definition =
        serde_json::from_value(value.clone()).map_err(|_| "needs an id, label and appearance")?;
    if definition.id.trim().is_empty() {
        return Err("needs an id".into());
    }
    let main = (!definition.colors.is_empty()).then_some(&definition.colors);
    let side = |variant: Appearance, roles: Option<&Roles>| {
        roles
            .or(main.filter(|_| definition.appearance == variant))
            .map(colors)
            .transpose()
    };
    let light = side(Appearance::Light, definition.variants.light.as_ref())?;
    let dark = side(Appearance::Dark, definition.variants.dark.as_ref())?;
    if light.is_none() && dark.is_none() {
        return Err("has no colors".into());
    }
    let collection = definition
        .collection
        .map_or_else(|| default_collection.to_owned(), |c| c.label);
    Ok(Extra {
        id: definition.id,
        label: definition.label,
        collection,
        light,
        dark,
    })
}

fn colors(roles: &Roles) -> Result<Colors, String> {
    let optional = |role: &str| -> Result<Option<Color>, String> {
        roles
            .get(role)
            .map(|value| color(value).ok_or_else(|| format!("{role} isn't a color")))
            .transpose()
    };
    let required = |role: &str| optional(role)?.ok_or_else(|| format!("is missing {role}"));
    let canvas = required("canvas")?;
    let surface = required("surface")?;
    let text = required("text")?;
    let accent = required("accent")?;
    let muted = optional("textMuted")?.unwrap_or_else(|| text.mix(surface, 0.3));
    Ok(Colors {
        canvas,
        chrome: optional("chrome")?.unwrap_or(canvas),
        surface,
        raised: optional("surfaceRaised")?.unwrap_or_else(|| surface.mix(text, 0.09)),
        text,
        muted,
        faint: optional("placeholder")?.unwrap_or_else(|| muted.mix(surface, 0.3)),
        accent,
        on_accent: optional("accentForeground")?.unwrap_or(if accent.luminance() > 0.3 {
            Color::BLACK
        } else {
            Color::WHITE
        }),
        border: optional("border")?,
        error: optional("error")?,
        warning: optional("warning")?,
        success: optional("success")?,
    })
}

/// `#rgb`, `#rrggbb`, either with alpha, `rgb()`/`rgba()`, or `oklch()`.
pub fn color(value: &str) -> Option<Color> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return hex_color(hex);
    }
    let (function, arguments) = value.strip_suffix(')')?.split_once('(')?;
    let arguments = arguments.replace(['/', ','], " ");
    let parts: Vec<&str> = arguments.split_whitespace().collect();
    let number = |index: usize| parts.get(index)?.parse::<f32>().ok();
    let fraction = |index: usize| -> Option<f32> {
        let part = parts.get(index)?;
        match part.strip_suffix('%') {
            Some(percent) => Some(percent.parse::<f32>().ok()? / 100.0),
            None => part.parse().ok(),
        }
    };
    let alpha = if parts.len() > 3 { fraction(3)? } else { 1.0 };
    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }
    match function.trim().to_ascii_lowercase().as_str() {
        "rgb" | "rgba" => {
            let channel = |index| number(index).map(|c: f32| c.clamp(0.0, 255.0).round() as u32);
            let hex = (channel(0)? << 16) | (channel(1)? << 8) | channel(2)?;
            Some(rgb(hex).alpha(alpha))
        }
        "oklch" => Some(rune::oklcha(fraction(0)?, number(1)?, number(2)?, alpha)),
        _ => None,
    }
}

fn hex_color(hex: &str) -> Option<Color> {
    if !hex.is_ascii() {
        return None;
    }
    let expanded: String = match hex.len() {
        3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => hex.to_owned(),
        _ => return None,
    };
    let value = u32::from_str_radix(&expanded[..6], 16).ok()?;
    let alpha = match expanded.get(6..8) {
        Some(alpha) => f32::from(u8::from_str_radix(alpha, 16).ok()?) / 255.0,
        None => 1.0,
    };
    Some(rgb(value).alpha(alpha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_bundled_theme_loads_with_the_expected_collections() {
        let library = Library::new(&[]);
        assert!(library.problems.is_empty());
        let count = |name: &str| {
            library
                .themes
                .iter()
                .filter(|theme| theme.collection == name)
                .count()
        };
        assert_eq!(count("Tokyo Night"), 4);
        assert_eq!(count("Catppuccin"), 4);
        assert_eq!(count("Omarchy"), 23);
        let bundled: Vec<Value> = serde_json::from_str(BUNDLED).unwrap();
        assert_eq!(library.themes.len(), bundled.len(), "none were skipped");
        assert!(library.find("tokyo-night-day").unwrap().light.is_some());
        assert!(library.find("catppuccin-mocha").unwrap().dark.is_some());
    }

    #[test]
    fn a_t3_code_theme_can_be_pasted_in_as_is() {
        let library = Library::new(&[json!({
            "id": "mine",
            "label": "Mine",
            "appearance": "dark",
            "sidebarArtwork": true,
            "colors": {
                "canvas": "oklch(0.2 0.01 250)",
                "surface": "#222",
                "text": "rgb(240, 240, 240)",
                "accent": "oklch(70% 0.1 200 / 0.9)",
                "terminalCursor": "#ffffff"
            },
            "variants": { "light": {
                "canvas": "#fff", "surface": "#ffffff", "text": "#000000", "accent": "#0000ff80"
            }}
        })]);
        assert!(library.problems.is_empty(), "{:?}", library.problems);
        let mine = library.find("mine").unwrap();
        assert_eq!(mine.collection, CUSTOM_COLLECTION);
        assert!(mine.dark.is_some() && mine.light.is_some());
    }

    #[test]
    fn bad_custom_themes_are_reported_without_harming_the_rest() {
        let bundled = Library::new(&[]).themes.len();
        let library = Library::new(&[
            json!({ "id": "a", "label": "No colors", "appearance": "dark" }),
            json!({ "id": "b", "label": "Bad", "appearance": "dark", "colors": {
                "canvas": "#zzz", "surface": "#000", "text": "#fff", "accent": "#fff"
            }}),
            json!({ "id": "c", "label": "Missing", "appearance": "dark", "colors": {
                "canvas": "#000"
            }}),
            json!("not a theme"),
        ]);
        assert_eq!(library.themes.len(), bundled);
        assert_eq!(library.problems.len(), 4, "{:?}", library.problems);
        assert!(library.problems[1].contains("canvas"));
    }

    #[test]
    fn a_custom_theme_can_replace_a_bundled_one() {
        let bundled = Library::new(&[]).themes.len();
        let library = Library::new(&[json!({
            "id": "catppuccin-mocha", "label": "My Mocha", "appearance": "dark",
            "colors": { "canvas": "#000", "surface": "#111", "text": "#fff", "accent": "#f0f" }
        })]);
        assert_eq!(library.themes.len(), bundled);
        assert_eq!(library.find("catppuccin-mocha").unwrap().label, "My Mocha");
    }
}
