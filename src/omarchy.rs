//! Follows the desktop theme on Omarchy.
//!
//! `omarchy-theme-set` stages the active theme, with its `colors.toml`, under
//! `~/.local/state/omarchy/current` (`~/.config/omarchy/current` before mid-2026). Reading that
//! file covers themes the user installed as well as the ones Omarchy ships.

use std::fs;
use std::path::Path;

use rune::Color;

use crate::themes::{self, Colors, color};

const LAYOUTS: [&str; 2] = [".local/state/omarchy/current", ".config/omarchy/current"];

#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    pub name: String,
    pub dark: bool,
    pub colors: Colors,
}

/// Omarchy's active theme. `None` anywhere but an Omarchy desktop, and during the moment a
/// switch has the theme folder swapped out.
pub fn poll(library: &themes::Library) -> Option<Current> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    read(directories::BaseDirs::new()?.home_dir(), library)
}

fn read(home: &Path, library: &themes::Library) -> Option<Current> {
    LAYOUTS.iter().find_map(|layout| {
        let current = home.join(layout);
        let theme = current.join("theme");
        if !theme.is_dir() {
            return None;
        }
        let name = theme_name(&current, &theme)?;
        match fs::read_to_string(theme.join("colors.toml"))
            .ok()
            .and_then(|text| parse(&text))
        {
            Some((dark, colors)) => Some(Current {
                name: display_name(&name),
                dark,
                colors,
            }),
            // Themes older than colors.toml, which Omarchy ships a palette for here.
            None => {
                let bundled = library.find(&format!("omarchy-{name}"))?;
                let dark = bundled.dark.is_some();
                Some(Current {
                    name: bundled.label.clone(),
                    dark,
                    colors: *bundled.colors(dark)?,
                })
            }
        }
    })
}

/// `theme.name`, or the link target in layouts that predate it.
fn theme_name(current: &Path, theme: &Path) -> Option<String> {
    let written = fs::read_to_string(current.join("theme.name"))
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty());
    written.or_else(|| Some(fs::read_link(theme).ok()?.file_name()?.to_str()?.to_owned()))
}

fn display_name(name: &str) -> String {
    name.split('-')
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (a.luminance() + 0.05, b.luminance() + 0.05);
    a.max(b) / a.min(b)
}

/// Moves `color` toward `toward` just far enough to read against `background`.
fn readable(color: Color, toward: Color, background: Color, minimum: f32) -> Color {
    (0..=20)
        .map(|step| color.mix(toward, step as f32 / 20.0))
        .find(|candidate| contrast(*candidate, background) >= minimum)
        .unwrap_or(toward)
}

/// The same mapping as the bundled Omarchy themes in `assets/themes.json`.
fn parse(text: &str) -> Option<(bool, Colors)> {
    let value = |key: &str| {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key)
                .then(|| color(value.trim().trim_matches(|c| c == '"' || c == '\'')))
                .flatten()
        })
    };
    let surface = value("background")?;
    let foreground = value("foreground")?;
    let accent = value("accent")?;
    let dark = match text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == "mode").then(|| value.trim().trim_matches('"').to_owned())
    }) {
        Some(mode) => mode != "light",
        None => surface.luminance() < 0.3,
    };
    let extreme = if dark { Color::WHITE } else { Color::BLACK };
    let text_color = readable(
        value("bright_foreground").unwrap_or(foreground),
        extreme,
        surface,
        7.0,
    );
    let shade = |key: &str, amount: f32| value(key).unwrap_or(surface.mix(Color::BLACK, amount));
    let (canvas, chrome, mut raised) = if dark {
        (
            shade("darker_background", 0.3),
            shade("dark_background", 0.15),
            value("lighter_background").unwrap_or(surface.mix(text_color, 0.09)),
        )
    } else {
        let dimmer = shade("dark_background", 0.04);
        (dimmer, shade("darker_background", 0.1), dimmer)
    };
    if contrast(raised, surface) < 1.08 {
        raised = surface.mix(text_color, 0.09);
    }
    let muted = if foreground == text_color {
        text_color.mix(surface, 0.3)
    } else {
        foreground
    };
    let accent = readable(accent, text_color, surface, 3.0);
    let status = |key: &str| value(key).map(|c| readable(c, text_color, surface, 3.0));
    Some((
        dark,
        Colors {
            canvas,
            chrome,
            surface,
            raised,
            text: text_color,
            muted: readable(muted, text_color, surface, 4.5),
            faint: readable(
                value("dark_foreground").unwrap_or(muted),
                text_color,
                surface,
                3.0,
            ),
            accent,
            on_accent: if contrast(Color::BLACK, accent) >= contrast(Color::WHITE, accent) {
                Color::BLACK
            } else {
                Color::WHITE
            },
            border: None,
            error: status("red"),
            warning: status("yellow"),
            success: status("green"),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NIGHT: &str = r##"
mode = "dark"
accent = "#7aa2f7"
background = "#1a1b26"
dark_background = "#13141c"
darker_background = "#0e0e14"
lighter_background = "#24283b"
foreground = "#a9b1d6"
dark_foreground = "#565f89"
bright_foreground = "#c0caf5"
red = "#f7768e"
"##;
    const PAPER: &str = r##"
mode = "light"
accent = "#205ea6"
background = "#fffcf0"
foreground = "#100f0f"
"##;

    fn folder(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("flexspeak-omarchy-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn install(home: &Path, layout: &str, name: &str, colors: Option<&str>) {
        let current = home.join(layout);
        fs::create_dir_all(current.join("theme")).unwrap();
        fs::write(current.join("theme.name"), format!("{name}\n")).unwrap();
        if let Some(colors) = colors {
            fs::write(current.join("theme/colors.toml"), colors).unwrap();
        }
    }

    #[test]
    fn follows_the_active_theme_and_notices_a_switch() {
        let home = folder("switch");
        let library = themes::Library::new(&[]);
        assert_eq!(read(&home, &library), None, "not Omarchy");
        install(&home, LAYOUTS[0], "my-own-theme", Some(NIGHT));
        let first = read(&home, &library).unwrap();
        assert_eq!(first.name, "My Own Theme");
        assert!(first.dark);
        assert_eq!(first.colors.surface, color("#1a1b26").unwrap());
        assert!(contrast(first.colors.text, first.colors.surface) >= 7.0);
        install(&home, LAYOUTS[0], "paper", Some(PAPER));
        let second = read(&home, &library).unwrap();
        assert_eq!(second.name, "Paper");
        assert!(!second.dark);
        assert_ne!(first.colors, second.colors);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn reads_the_older_layout_and_themes_without_colors() {
        let home = folder("older");
        let library = themes::Library::new(&[]);
        install(&home, LAYOUTS[1], "nord", None);
        let current = read(&home, &library).unwrap();
        assert_eq!(current.name, "Nord");
        assert!(current.dark);
        install(&home, LAYOUTS[1], "unknown-and-empty", None);
        assert_eq!(read(&home, &library), None);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_broken_colors_file_is_not_a_theme() {
        assert!(parse("mode = \"dark\"\naccent = \"#fff\"\n").is_none());
        assert!(parse("background = \"not a color\"\n").is_none());
    }
}
