//! Flexspeak's icon in the Windows notification area or the macOS menu bar, as TeamSpeak has
//! one: a ring while your microphone is open that lights up while you talk, a struck
//! microphone while muted and struck headphones while deafened. It follows the server on
//! screen, the one your microphone goes to, and shows the app's badge when that one isn't
//! connected. A click brings the window forward; the menu mutes, deafens, shows or quits.
//!
//! Windows uses Rune's own tray. macOS stays on the `tray-icon` crate: Rune's tray opens its
//! menu on a secondary click there, where a menu bar item opens on any click. Linux has none
//! yet; Rune's tray could give it one (StatusNotifierItem), but nobody has tried it there.
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};

use crate::ui::icons;

/// Whether this platform has a tray icon at all.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

/// Set by `main` for a real run, so headless scenes and tests never put an icon on screen.
static ALLOWED: AtomicBool = AtomicBool::new(false);

pub fn allow() {
    ALLOWED.store(true, Ordering::Release);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Offline,
    Open,
    Talking,
    Muted,
    Deafened,
}

impl State {
    #[cfg(test)]
    pub const ALL: [State; 5] = [
        State::Offline,
        State::Open,
        State::Talking,
        State::Muted,
        State::Deafened,
    ];

    fn label(self) -> &'static str {
        match self {
            State::Offline => "Not connected",
            State::Open => "Microphone on",
            State::Talking => "Talking",
            State::Muted => "Microphone muted",
            State::Deafened => "Deafened",
        }
    }
}

/// What the icon, its tooltip and its menu show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    /// The server on screen; empty while it isn't connected.
    pub server: String,
    pub muted: bool,
    pub deafened: bool,
}

impl Status {
    pub fn connected(&self) -> bool {
        self.state != State::Offline
    }

    pub fn tooltip(&self) -> String {
        if self.connected() {
            // Windows keeps 127 characters of a tooltip.
            let server: String = self.server.chars().take(80).collect();
            format!("Flexspeak · {} · {server}", self.state.label())
        } else {
            "Flexspeak · Not connected".to_owned()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Show,
    Mute,
    Deafen,
    Disconnect,
    Quit,
}

/// Colours for the taskbar or menu bar behind the icon, not the app's theme: the default
/// themes' status colours, the glyph as grey as the tree's.
struct Ink {
    glyph: &'static str,
    good: &'static str,
    poor: &'static str,
}

fn ink(dark: bool) -> Ink {
    if dark {
        Ink {
            glyph: "#c9c9cf",
            good: "#6fdc9a",
            poor: "#ff7a8a",
        }
    } else {
        Ink {
            glyph: "#55555c",
            good: "#1c9354",
            poor: "#d43d55",
        }
    }
}

/// An icon's SVG body with its `currentColor` replaced and its strokes a little heavier, so
/// they hold up at 16 px.
fn paint(icon: &rune::Icon, color: &str, weight: &str) -> String {
    icon.body
        .replace("currentColor", color)
        .replace(
            r#"stroke-width="2.6""#,
            &format!(r#"stroke-width="{weight}""#),
        )
        .replace(
            r#"stroke-width="2""#,
            &format!(r#"stroke-width="{weight}""#),
        )
}

/// The icon for `state` on a dark or light taskbar, as a 24 × 24 SVG.
pub fn svg(state: State, dark: bool) -> String {
    let ink = ink(dark);
    let body = match state {
        State::Offline => return include_str!("../assets/icon/flexspeak.svg").to_owned(),
        State::Open => format!(
            r#"<circle cx="12" cy="12" r="6.6" fill="none" stroke="{}" stroke-width="2.6"/>"#,
            ink.glyph
        ),
        State::Talking => format!(
            r#"<defs><radialGradient id="glow"><stop offset="0.45" stop-color="{good}" stop-opacity="0.55"/><stop offset="1" stop-color="{good}" stop-opacity="0"/></radialGradient></defs><circle cx="12" cy="12" r="12" fill="url(#glow)"/><circle cx="12" cy="12" r="7.9" fill="{good}"/>"#,
            good = ink.good
        ),
        State::Muted => {
            paint(&icons::MIC_OFF_GLYPH, ink.glyph, "2.4") + &paint(&icons::STRIKE, ink.poor, "2.9")
        }
        State::Deafened => {
            paint(&icons::HEADPHONE_OFF_GLYPH, ink.glyph, "2.4")
                + &paint(&icons::STRIKE, ink.poor, "2.9")
        }
    };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">{body}</svg>"#
    )
}

/// The icon as straight RGBA, `size` pixels square.
pub fn pixels(state: State, dark: bool, size: u32) -> Vec<u8> {
    use resvg::{tiny_skia, usvg};

    let size = size.max(1);
    let Some(mut pixmap) = tiny_skia::Pixmap::new(size, size) else {
        return vec![0; size as usize * size as usize * 4];
    };
    if let Ok(tree) = usvg::Tree::from_str(&svg(state, dark), &usvg::Options::default()) {
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
    }
    pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}

/// Shows `status` in the tray, adding the icon if needed, or removes it for `None`. Call on
/// the UI thread. `actions` is asked for a handler only when the icon is first added.
#[cfg(any(windows, target_os = "macos"))]
pub fn sync<F>(status: Option<Status>, actions: impl FnOnce() -> Option<F>)
where
    F: Fn(Action) + Send + Sync + 'static,
{
    if !ALLOWED.load(Ordering::Acquire) {
        return;
    }
    native::sync(status, actions);
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn sync<F>(_: Option<Status>, _: impl FnOnce() -> Option<F>)
where
    F: Fn(Action) + Send + Sync + 'static,
{
}

/// Windows: Rune's tray, which keeps the icon on a thread of its own, puts it back when
/// Explorer restarts and takes it off the taskbar as the app quits.
#[cfg(windows)]
mod native {
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };

    use rune::{Tray, TrayEvent, TrayIcon, TrayItem, TraySpec};

    use super::{Action, State, Status, pixels};

    /// How often the taskbar's light or dark mode is read again.
    const APPEARANCE_CHECK: Duration = Duration::from_secs(2);

    struct Live {
        tray: Tray,
        shown: Status,
        dark: bool,
        checked_at: Instant,
    }

    thread_local! {
        /// Made, changed and dropped on the UI thread.
        static TRAY: RefCell<Option<Live>> = const { RefCell::new(None) };
    }

    pub(super) fn sync<F>(status: Option<Status>, actions: impl FnOnce() -> Option<F>)
    where
        F: Fn(Action) + Send + Sync + 'static,
    {
        TRAY.with_borrow_mut(|tray| {
            let Some(status) = status else {
                // Dropping it takes the icon off the taskbar.
                *tray = None;
                return;
            };
            match tray {
                Some(live) => live.show(status),
                None => {
                    if let Some(actions) = actions() {
                        match Live::new(status, actions) {
                            Ok(live) => *tray = Some(live),
                            Err(error) => tracing::warn!("Tray icon not shown: {error}"),
                        }
                    }
                }
            }
        });
    }

    fn icon(state: State, dark: bool) -> TrayIcon {
        let size = size();
        TrayIcon::rgba(size, size, pixels(state, dark, size))
    }

    /// The notification area's icon size at the system's scale: 16 px at 100%, 32 at 200%.
    fn size() -> u32 {
        use windows_sys::Win32::UI::{
            HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi},
            WindowsAndMessaging::SM_CXSMICON,
        };
        // SAFETY: Both calls only read system metrics.
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
        u32::try_from(size)
            .ok()
            .filter(|&size| size >= 16)
            .unwrap_or(16)
    }

    /// Whether the icon sits on a dark taskbar: Windows' "Windows mode" in Colours, which can
    /// differ from the app mode.
    fn dark() -> bool {
        use windows_sys::Win32::System::Registry::{
            HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW,
        };
        let key =
            windows_sys::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let value = windows_sys::core::w!("SystemUsesLightTheme");
        let mut light: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: The key and value names are NUL-terminated and `light` holds one DWORD.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key,
                value,
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut light as *mut u32).cast(),
                &mut size,
            )
        };
        // Without the value, Windows' taskbar is dark.
        status != 0 || light == 0
    }

    /// The menu a right click opens. The ticks and greyed items are part of it, so it is
    /// made again whenever they change.
    pub(super) fn menu(status: &Status) -> Vec<TrayItem> {
        let connected = status.connected();
        vec![
            TrayItem::check("mute", "Mute Microphone", status.muted).enabled(connected),
            TrayItem::check("deafen", "Deafen", status.deafened).enabled(connected),
            TrayItem::action("disconnect", "Disconnect").enabled(connected),
            TrayItem::Separator,
            TrayItem::action("show", "Show Flexspeak"),
            TrayItem::action("quit", "Quit Flexspeak"),
        ]
    }

    /// What a click on the icon or a pick from its menu asks for.
    pub(super) fn action(event: &TrayEvent) -> Option<Action> {
        match event {
            TrayEvent::Activate { .. } => Some(Action::Show),
            TrayEvent::Menu(id) => match id.as_str() {
                "mute" => Some(Action::Mute),
                "deafen" => Some(Action::Deafen),
                "disconnect" => Some(Action::Disconnect),
                "show" => Some(Action::Show),
                "quit" => Some(Action::Quit),
                _ => None,
            },
            _ => None,
        }
    }

    impl Live {
        fn new(
            status: Status,
            actions: impl Fn(Action) + Send + Sync + 'static,
        ) -> Result<Self, String> {
            let dark = dark();
            let spec = TraySpec::new("flexspeak", "Flexspeak", icon(status.state, dark))
                .tooltip(status.tooltip())
                .menu(menu(&status));
            // Runs on the tray's own thread.
            let tray = Tray::new(spec, move |event| {
                if let Some(action) = action(&event) {
                    actions(action);
                }
            })
            .map_err(|error| error.to_string())?;
            Ok(Self {
                tray,
                shown: status,
                dark,
                checked_at: Instant::now(),
            })
        }

        fn show(&mut self, status: Status) {
            let now = Instant::now();
            let mut dark = self.dark;
            if now.saturating_duration_since(self.checked_at) >= APPEARANCE_CHECK {
                self.checked_at = now;
                dark = self::dark();
            }
            if (status.state != self.shown.state || dark != self.dark)
                && let Err(error) = self.tray.set_icon(icon(status.state, dark))
            {
                tracing::debug!("Tray icon not updated: {error}");
            }
            self.dark = dark;
            if status == self.shown {
                return;
            }
            if status.tooltip() != self.shown.tooltip() {
                self.tray.set_tooltip(status.tooltip());
            }
            self.tray.set_menu(menu(&status));
            self.shown = status;
        }
    }
}

/// macOS: the `tray-icon` crate, whose menu opens on a click as the menu bar's items do.
#[cfg(target_os = "macos")]
mod native {
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };

    use tray_icon::{
        Icon, TrayIcon, TrayIconBuilder,
        menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    };

    use super::{Action, State, Status, pixels};

    /// How often the menu bar's light or dark mode is read again.
    const APPEARANCE_CHECK: Duration = Duration::from_secs(2);

    struct Live {
        icon: TrayIcon,
        mute: CheckMenuItem,
        deafen: CheckMenuItem,
        disconnect: MenuItem,
        shown: Status,
        dark: bool,
        checked_at: Instant,
    }

    thread_local! {
        /// The tray's types aren't `Send`; they stay on the UI thread that made them.
        static TRAY: RefCell<Option<Live>> = const { RefCell::new(None) };
    }

    pub(super) fn sync<F>(status: Option<Status>, actions: impl FnOnce() -> Option<F>)
    where
        F: Fn(Action) + Send + Sync + 'static,
    {
        TRAY.with_borrow_mut(|tray| {
            let Some(status) = status else {
                // Dropping it takes the icon off the menu bar.
                *tray = None;
                return;
            };
            match tray {
                Some(live) => live.show(status),
                None => {
                    if let Some(actions) = actions() {
                        match Live::new(status, actions) {
                            Ok(live) => *tray = Some(live),
                            Err(error) => tracing::warn!("Tray icon not shown: {error}"),
                        }
                    }
                }
            }
        });
    }

    fn icon(state: State, dark: bool) -> Option<Icon> {
        let size = size();
        Icon::from_rgba(pixels(state, dark, size), size, size).ok()
    }

    /// The menu bar draws its icons 18 pt tall; two pixels a point for Retina screens.
    fn size() -> u32 {
        36
    }

    fn dark() -> bool {
        crate::system_appearance::is_dark().unwrap_or(true)
    }

    impl Live {
        fn new(
            status: Status,
            actions: impl Fn(Action) + Send + Sync + 'static,
        ) -> Result<Self, String> {
            let dark = super::native::dark();
            let mute = CheckMenuItem::with_id(
                "mute",
                "Mute Microphone",
                status.connected(),
                status.muted,
                None,
            );
            let deafen = CheckMenuItem::with_id(
                "deafen",
                "Deafen",
                status.connected(),
                status.deafened,
                None,
            );
            let menu = Menu::new();
            let separator = PredefinedMenuItem::separator();
            let disconnect =
                MenuItem::with_id("disconnect", "Disconnect", status.connected(), None);
            let show = MenuItem::with_id("show", "Show Flexspeak", true, None);
            let quit = MenuItem::with_id("quit", "Quit Flexspeak", true, None);
            menu.append_items(&[&mute, &deafen, &disconnect, &separator, &show, &quit])
                .map_err(|error| error.to_string())?;
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let action = match event.id.0.as_str() {
                    "mute" => Action::Mute,
                    "deafen" => Action::Deafen,
                    "disconnect" => Action::Disconnect,
                    "show" => Action::Show,
                    "quit" => Action::Quit,
                    _ => return,
                };
                actions(action);
            }));
            let mut builder = TrayIconBuilder::new()
                .with_id("flexspeak")
                .with_tooltip(status.tooltip())
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(true);
            if let Some(icon) = icon(status.state, dark) {
                builder = builder.with_icon(icon);
            }
            let icon = builder.build().map_err(|error| error.to_string())?;
            Ok(Self {
                icon,
                mute,
                deafen,
                disconnect,
                shown: status,
                dark,
                checked_at: Instant::now(),
            })
        }

        fn show(&mut self, status: Status) {
            let now = Instant::now();
            let mut dark = self.dark;
            if now.saturating_duration_since(self.checked_at) >= APPEARANCE_CHECK {
                self.checked_at = now;
                dark = super::native::dark();
            }
            if (status.state != self.shown.state || dark != self.dark)
                && let Err(error) = self.icon.set_icon(icon(status.state, dark))
            {
                tracing::debug!("Tray icon not updated: {error}");
            }
            if status.state != self.shown.state || status.server != self.shown.server {
                let _ = self.icon.set_tooltip(Some(status.tooltip()));
            }
            // A click ticks a check item by itself; set both to what's true every time.
            self.mute.set_enabled(status.connected());
            self.deafen.set_enabled(status.connected());
            self.disconnect.set_enabled(status.connected());
            self.mute.set_checked(status.muted);
            self.deafen.set_checked(status.deafened);
            self.shown = status;
            self.dark = dark;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_draws_something_opaque_at_tray_sizes() {
        for state in State::ALL {
            for dark in [false, true] {
                for size in [16, 20, 24, 32, 36] {
                    let rgba = pixels(state, dark, size);
                    assert_eq!(rgba.len(), size as usize * size as usize * 4);
                    let opaque = rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .filter(|p| p[3] > 200)
                        .count();
                    assert!(opaque > 8, "{state:?} at {size} px is nearly empty");
                }
            }
        }
    }

    #[test]
    fn only_the_strike_is_red() {
        let rgba = pixels(State::Muted, true, 32);
        let red = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 200 && p[0] > 200 && p[1] < 160)
            .count();
        let grey = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 200 && p[0].abs_diff(p[1]) < 12)
            .count();
        assert!(red > 10 && grey > 10, "red {red}, grey {grey}");
    }

    #[test]
    fn the_tooltip_names_the_state_and_server() {
        let status = Status {
            state: State::Muted,
            server: "ts.zirkumflex.io".into(),
            muted: true,
            deafened: false,
        };
        assert_eq!(
            status.tooltip(),
            "Flexspeak · Microphone muted · ts.zirkumflex.io"
        );
        let offline = Status {
            state: State::Offline,
            server: String::new(),
            muted: true,
            deafened: false,
        };
        assert_eq!(offline.tooltip(), "Flexspeak · Not connected");
    }

    /// `FLEXSPEAK_TRAY_SHEET=out.png cargo test tray_sheet -- --ignored` draws every state on
    /// a light and a dark taskbar, at 16, 24 and 32 px and magnified, for a look at them.
    #[test]
    #[ignore]
    fn tray_sheet() {
        use resvg::tiny_skia::{Color, IntSize, Pixmap, PixmapPaint, Transform};
        let path = std::env::var("FLEXSPEAK_TRAY_SHEET").expect("FLEXSPEAK_TRAY_SHEET");
        let sizes = [16_u32, 24, 32];
        let zoom = 4_u32;
        let cell = 32 * zoom + 24;
        let columns = State::ALL.len() as u32;
        let band = cell + 48;
        let mut sheet = Pixmap::new(columns * cell + 24, 2 * band).unwrap();
        for (row, dark) in [false, true].into_iter().enumerate() {
            let bar = if dark {
                Color::from_rgba8(0x1c, 0x1c, 0x1c, 255)
            } else {
                Color::from_rgba8(0xee, 0xee, 0xee, 255)
            };
            let top = row as u32 * band;
            let mut fill = Pixmap::new(sheet.width(), band).unwrap();
            fill.fill(bar);
            sheet.draw_pixmap(
                0,
                top as i32,
                fill.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            for (column, state) in State::ALL.into_iter().enumerate() {
                let left = 24 + column as u32 * cell;
                let mut x = left;
                for size in sizes {
                    let icon = Pixmap::from_vec(
                        premultiply(pixels(state, dark, size)),
                        IntSize::from_wh(size, size).unwrap(),
                    )
                    .unwrap();
                    sheet.draw_pixmap(
                        x as i32,
                        (top + 12) as i32,
                        icon.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                    x += size + 10;
                }
                // The 16 px icon magnified, pixel for pixel, as it lands at 100% scale.
                let small = Pixmap::from_vec(
                    premultiply(pixels(state, dark, 16)),
                    IntSize::from_wh(16, 16).unwrap(),
                )
                .unwrap();
                let paint = PixmapPaint {
                    quality: resvg::tiny_skia::FilterQuality::Nearest,
                    ..PixmapPaint::default()
                };
                let scale = (zoom * 2) as f32;
                sheet.draw_pixmap(
                    0,
                    0,
                    small.as_ref(),
                    &paint,
                    Transform::from_row(scale, 0.0, 0.0, scale, left as f32, (top + 52) as f32),
                    None,
                );
            }
        }
        sheet.save_png(path).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn the_menu_ticks_and_greys_what_the_status_says() {
        use rune::TrayItem;
        let mut status = Status {
            state: State::Muted,
            server: "ts.zirkumflex.io".into(),
            muted: true,
            deafened: false,
        };
        let menu = native::menu(&status);
        assert!(matches!(
            &menu[0],
            TrayItem::Check { id, enabled: true, checked: true, .. } if id == "mute"
        ));
        assert!(matches!(
            &menu[1],
            TrayItem::Check { id, enabled: true, checked: false, .. } if id == "deafen"
        ));
        assert!(matches!(
            &menu[2],
            TrayItem::Action { id, enabled: true, .. } if id == "disconnect"
        ));
        status.state = State::Offline;
        let menu = native::menu(&status);
        for item in &menu[..3] {
            assert!(matches!(
                item,
                TrayItem::Check { enabled: false, .. } | TrayItem::Action { enabled: false, .. }
            ));
        }
        // Showing the window and quitting work without a server.
        for item in &menu[4..] {
            assert!(matches!(item, TrayItem::Action { enabled: true, .. }));
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_click_shows_the_window_and_every_menu_item_has_its_action() {
        use rune::{TrayEvent, TrayItem};
        assert_eq!(
            native::action(&TrayEvent::Activate { token: None }),
            Some(Action::Show)
        );
        let status = Status {
            state: State::Open,
            server: "ts.zirkumflex.io".into(),
            muted: false,
            deafened: false,
        };
        let actions: Vec<Action> = native::menu(&status)
            .into_iter()
            .filter_map(|item| match item {
                TrayItem::Check { id, .. } | TrayItem::Action { id, .. } => {
                    Some(native::action(&TrayEvent::Menu(id)).expect("an action for every item"))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            actions,
            [
                Action::Mute,
                Action::Deafen,
                Action::Disconnect,
                Action::Show,
                Action::Quit
            ]
        );
        assert_eq!(native::action(&TrayEvent::Menu("other".into())), None);
    }

    fn premultiply(mut rgba: Vec<u8>) -> Vec<u8> {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        rgba
    }
}
