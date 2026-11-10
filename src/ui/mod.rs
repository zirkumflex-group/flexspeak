//! The Rune interface: the window, effects that leave the UI thread, and every view.
//!
//! `App::update` keeps the application's rules. Views turn presses into `Message`s through
//! [`send`]; [`dispatch`] runs the message, carries out the returned [`Task`] with the frame's
//! `Tx`, and mirrors the app's text values into the fields that edit them.

mod charts;
mod footer;
mod fx;
mod home;
pub mod icons;
mod members;
mod modals;
pub mod notices;
mod onboarding;
mod scenes;
mod settings;
mod statistics;
mod streams;
mod theme;
mod video;
pub(crate) use theme::rgb;
mod updates;
mod widgets;

use std::time::{Duration, Instant};

use rune::{AppSpec, Cx, Drag, El, Phase, TextEdit, Tx, Waker, col, el};
use tokio::sync::mpsc;

use crate::{App, Message, Tab, client, shortcuts, usage};

pub use theme::Theme;

/// A [`Task::Stream`] job: interim messages go to the callback, the result is the last one.
pub type StreamJob = dyn FnOnce(&dyn Fn(Message)) -> Message + Send;

/// What an update asks the window or a worker thread to do after it returns.
/// How often the one-click share suggestions look for a game again.
const QUICK_SOURCE_REFRESH: Duration = Duration::from_secs(15);

/// The server rail while it hides behind the left edge: with one server it only needs to show
/// when asked. Derived from when the pointer last came or went, so views stay pure.
#[derive(Debug, Clone, Copy, Default)]
pub struct RailPeek {
    /// The pointer is over the edge strip.
    strip: bool,
    /// The pointer is over the open rail.
    card: bool,
    /// Opened from the keyboard, so no pointer will close it.
    pinned: bool,
    was_open: bool,
    changed: Option<Instant>,
    /// What the view last drew, so `poll` redraws when a delay runs out.
    drawn: bool,
}

impl RailPeek {
    /// Long enough that crossing the edge on the way somewhere else doesn't open it.
    const OPEN_AFTER: Duration = Duration::from_millis(160);
    /// Forgives the pointer briefly slipping off the rail.
    const CLOSE_AFTER: Duration = Duration::from_millis(280);
    /// A wake just past either delay redraws the rail in its new state.
    const WAKE: [Duration; 2] = [Duration::from_millis(170), Duration::from_millis(290)];

    fn inside(&self) -> bool {
        self.strip || self.card
    }

    pub fn open(&self, now: Instant) -> bool {
        let waited = self
            .changed
            .map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
        self.pinned
            || if self.inside() {
                self.was_open || waited >= Self::OPEN_AFTER
            } else {
                self.was_open && waited < Self::CLOSE_AFTER
            }
    }

    fn hover(&mut self, now: Instant, change: impl FnOnce(&mut Self)) {
        let (open, inside) = (self.open(now), self.inside());
        change(self);
        if self.inside() != inside {
            self.was_open = open;
            self.changed = Some(now);
        }
    }

    /// A click opens it at once; from the keyboard it stays until it's pressed again or Escape.
    fn press(&mut self) {
        if self.inside() {
            self.was_open = true;
        } else {
            self.pinned = !self.pinned;
        }
    }

    /// Whether the rail's delay has run out since the last draw.
    fn expired(&mut self, now: Instant) -> bool {
        let open = self.open(now);
        std::mem::replace(&mut self.drawn, open) != open
    }

    fn unpin(&mut self) -> bool {
        std::mem::take(&mut self.pinned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupsMenu {
    Server,
    Channel,
}

pub enum Task {
    None,
    Batch(Vec<Task>),
    Exit,
    Copy(String),
    Focus(Field),
    /// Blocking work for a worker thread; its message returns to the UI thread.
    Perform(Box<dyn FnOnce() -> Message + Send>),
    /// Blocking work for a worker thread that reports along the way: it gets a sender for
    /// interim messages (download progress) and returns its final one.
    Stream(Box<StreamJob>),
    /// A session's backend events, forwarded until the worker closes the channel.
    Events(crate::hub::SessionId, mpsc::Receiver<client::Event>),
}

impl Task {
    pub fn none() -> Self {
        Self::None
    }

    pub fn batch(tasks: impl IntoIterator<Item = Task>) -> Self {
        Self::Batch(tasks.into_iter().collect())
    }

    /// Runs `future` to completion on a worker's own current-thread runtime.
    pub fn perform<T: Send + 'static>(
        future: impl Future<Output = T> + Send + 'static,
        map: impl FnOnce(T) -> Message + Send + 'static,
    ) -> Self {
        Self::Perform(Box::new(move || {
            let output = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map(|runtime| runtime.block_on(future));
            match output {
                Ok(value) => map(value),
                // Report through the normal alert path rather than losing the request.
                Err(error) => Message::Alert(
                    crate::alerts::Source::Worker,
                    format!("Could not start a background task: {error}"),
                ),
            }
        }))
    }
}

impl Task {
    pub fn stream(job: impl FnOnce(&dyn Fn(Message)) -> Message + Send + 'static) -> Self {
        Self::Stream(Box::new(job))
    }
}

/// Text fields whose keyboard focus an update can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    ChannelPassword,
    PokeMessage,
    ModerationReason,
}

/// Members being dragged onto a channel.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberDrag {
    pub ids: Vec<u16>,
    /// The pointer, in window points.
    pub at: (f32, f32),
    /// The channel under the pointer, where a drop would move them.
    pub target: Option<u64>,
}

/// A watched stream being dragged onto another to trade places.
#[derive(Debug, Clone, PartialEq)]
pub struct TileDrag {
    pub id: String,
    /// The pointer, in window points.
    pub at: (f32, f32),
    /// The watched stream under the pointer, which a drop would trade places with.
    pub target: Option<String>,
}

/// Who the volume dialog is adjusting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserVolumeTarget {
    pub uid: String,
    pub name: String,
}

/// Interface-only state: field editors and the open picker.
pub struct UiState {
    pub user_volume: Option<UserVolumeTarget>,
    /// The rail's + is open, listing saved servers to connect to.
    pub server_picker: bool,
    pub address: TextEdit,
    pub nickname: TextEdit,
    pub password: TextEdit,
    pub channel_password: TextEdit,
    pub key_capture: TextEdit,
    pub settings_search: TextEdit,
    pub server_name: TextEdit,
    pub server_address: TextEdit,
    pub server_password: TextEdit,
    pub poke_message: TextEdit,
    pub moderation_reason: TextEdit,
    /// The device or source picker whose list is open.
    pub open: Option<&'static str>,
    /// The member a context menu is open for, and where it opened.
    pub member_menu: Option<(u16, rune::PointerPress)>,
    /// The server in the rail a context menu is open for, and where it opened.
    pub rail_menu: Option<(crate::hub::SessionId, rune::PointerPress)>,
    /// The context menu's Move submenu is open.
    pub move_menu: bool,
    pub rail_peek: RailPeek,
    /// What one click can share, refreshed while you could go live.
    pub quick_sources: Vec<crate::streaming::publishing::ShareSource>,
    pub quick_checking: bool,
    pub next_quick_check: Instant,
    /// Which group submenu of the member menu is open.
    pub groups_menu: Option<GroupsMenu>,
    pub drag: Option<MemberDrag>,
    pub tile_drag: Option<TileDrag>,
    /// The pointer is over the update notice, which then lists what's new.
    pub update_hover: bool,
    /// Whether the pointer is over the streams, which shows their toolbar.
    pub streams_hover: bool,
    /// The share dialog shows its Advanced tab: resolution, frame rate and bitrates one by one.
    pub share_advanced: bool,
    /// The share dialog's Screen, Window or Camera choice while nothing of that kind is picked.
    pub share_group: Option<crate::streaming::publishing::SourceGroup>,
    /// Whether the pointer is over the server log, which shows its search button.
    pub log_hover: bool,
    /// Whether the pointer is over the window's close button, which turns its glyph white.
    #[cfg(windows)]
    pub close_hover: bool,
    /// The server log's splitter is being dragged.
    pub log_resizing: bool,
    pub log_people: bool,
    pub log_search: TextEdit,
    /// The first frame still has to build the frosted backdrop's pipelines.
    pub warm_blur: bool,
    /// Settings → Updates lists every release, not only the latest.
    pub changelog_open: bool,
    /// Where the current press started, in window points.
    pub press_point: (f32, f32),
    /// The window position of the member row or stream last under the pointer, to place drags.
    pub row_origin: (f32, f32),
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            server_picker: false,
            address: TextEdit::new(""),
            nickname: TextEdit::new(""),
            password: TextEdit::new("").secure(),
            channel_password: TextEdit::new("").secure(),
            key_capture: TextEdit::new(""),
            settings_search: TextEdit::new(""),
            server_name: TextEdit::new(""),
            server_address: TextEdit::new(""),
            server_password: TextEdit::new("").secure(),
            poke_message: TextEdit::new(""),
            moderation_reason: TextEdit::new(""),
            open: None,
            member_menu: None,
            rail_menu: None,
            move_menu: false,
            rail_peek: RailPeek::default(),
            quick_sources: Vec::new(),
            quick_checking: false,
            next_quick_check: Instant::now(),
            groups_menu: None,
            drag: None,
            tile_drag: None,
            update_hover: false,
            streams_hover: false,
            share_advanced: false,
            share_group: None,
            log_hover: false,
            #[cfg(windows)]
            close_hover: false,
            log_resizing: false,
            log_people: false,
            log_search: TextEdit::new(""),
            warm_blur: true,
            changelog_open: false,
            user_volume: None,
            press_point: (0.0, 0.0),
            row_origin: (0.0, 0.0),
        }
    }
}

fn mirror(edit: &mut TextEdit, value: &str) {
    if edit.text != value {
        edit.replace_text(value);
    }
}

impl App {
    /// Shows the app's current values in their editors. The app's strings stay authoritative,
    /// so an edit `update` rejects (a locked field while connected) is reverted here.
    pub(crate) fn sync_fields(&mut self) {
        mirror(&mut self.ui.address, &self.address);
        mirror(&mut self.ui.nickname, &self.nickname);
        mirror(&mut self.ui.password, &self.password);
        let channel_password = self.server().channel_password.clone();
        mirror(&mut self.ui.channel_password, &channel_password);
        mirror(&mut self.ui.key_capture, "");
        mirror(&mut self.ui.settings_search, &self.settings_query);
        let log_query = self.server().log.query().to_owned();
        mirror(&mut self.ui.log_search, &log_query);
        let editor = self.server_editor.clone().unwrap_or_default();
        mirror(&mut self.ui.server_name, &editor.name);
        mirror(&mut self.ui.server_address, &editor.address);
        mirror(&mut self.ui.server_password, &editor.password);
        let poke = self
            .server()
            .poke
            .as_ref()
            .map_or(String::new(), |draft| draft.message.clone());
        mirror(&mut self.ui.poke_message, &poke);
        let reason = self
            .server()
            .moderation
            .as_ref()
            .map_or(String::new(), |draft| draft.reason.clone());
        mirror(&mut self.ui.moderation_reason, &reason);
    }
}

/// Runs one message on the UI thread and carries out what it returns.
pub fn dispatch(s: &mut App, tx: &mut Tx, message: Message) {
    if !dispatch_visible(s, tx, message) {
        tx.live_only();
    }
}

/// `dispatch`, saying whether the message changed anything on screen. Backend updates
/// deliver through `update_if` with this, since `live_only` can't stop their commit.
fn dispatch_visible(s: &mut App, tx: &mut Tx, message: Message) -> bool {
    // Exact equality preserves the meter's accessibility value as well as its pixels.
    // Statistics consumes counters and buffer sizes even when the meter is unchanged.
    let invisible = match &message {
        Message::Backend(
            id,
            crate::client::Event::Meter {
                level,
                sent,
                received,
                audio_buffer_bytes,
            },
        ) => s.session(*id).is_none_or(|session| {
            let active = s.active == Some(*id);
            (!active || s.level == *level)
                && (s.tab != Tab::Statistics
                    || (session.sent == *sent
                        && session.received == *received
                        && (!active || s.audio_buffer_bytes == *audio_buffer_bytes)))
        }),
        _ => false,
    };
    let task = s.update(message);
    execute(s, tx, task);
    s.sync_fields();
    sync_tray(s);
    !invisible
}

/// Keeps the notification-area icon in step with the server on screen. Cheap when nothing
/// changed: the icon is redrawn only when its state does.
fn sync_tray(s: &App) {
    let handle = s.handle.clone();
    crate::tray::sync(s.tray_status(), move || {
        let handle = handle?;
        Some(move |action| tray_action(&handle, action))
    });
}

/// Runs on the tray's event handler, so everything but Show goes through the UI thread.
fn tray_action(handle: &rune::AppHandle<App>, action: crate::tray::Action) {
    use crate::tray::Action;
    let message = match action {
        Action::Show => return handle.open(),
        Action::Mute => Message::Mute,
        Action::Deafen => Message::Deafen,
        Action::Quit => Message::Close,
    };
    handle.update(move |s, tx| dispatch(s, tx, message));
}

/// A press handler that sends `message`.
pub fn send(message: Message) -> impl Fn(&mut App, &mut Tx) + Clone + 'static {
    move |s: &mut App, tx: &mut Tx| dispatch(s, tx, message.clone())
}

fn execute(s: &mut App, tx: &mut Tx, task: Task) {
    match task {
        Task::None => {}
        Task::Batch(tasks) => {
            for task in tasks {
                execute(s, tx, task);
            }
        }
        Task::Exit => tx.quit(),
        Task::Copy(text) => tx.copy(text),
        Task::Focus(Field::ChannelPassword) => s.ui.channel_password.focused = true,
        Task::Focus(Field::PokeMessage) => s.ui.poke_message.focused = true,
        Task::Focus(Field::ModerationReason) => s.ui.moderation_reason.focused = true,
        Task::Perform(job) => {
            let handle = s.handle.clone();
            let spawned = std::thread::Builder::new()
                .name("flexspeak-task".into())
                .spawn(move || {
                    let message = job();
                    if let Some(handle) = handle {
                        handle.update(move |s, tx| dispatch(s, tx, message));
                    }
                });
            if let Err(error) = spawned {
                s.alerts.raise(
                    crate::alerts::Source::Worker,
                    format!("Could not start a background task: {error}"),
                );
            }
        }
        Task::Stream(job) => {
            let handle = s.handle.clone();
            let spawned = std::thread::Builder::new()
                .name("flexspeak-task".into())
                .spawn(move || {
                    let deliver = |message: Message| {
                        if let Some(handle) = &handle {
                            handle.update(move |s, tx| dispatch(s, tx, message));
                        }
                    };
                    let message = job(&deliver);
                    deliver(message);
                });
            if let Err(error) = spawned {
                s.alerts.raise(
                    crate::alerts::Source::Worker,
                    format!("Could not start a background task: {error}"),
                );
            }
        }
        Task::Events(id, mut events) => {
            let Some(handle) = s.handle.clone() else {
                return;
            };
            let spawned = std::thread::Builder::new()
                .name("flexspeak-events".into())
                .spawn(move || {
                    while let Some(event) = events.blocking_recv() {
                        let mut batch = vec![event];
                        // Bound the drain to the events already queued, so a busy producer
                        // cannot postpone delivery indefinitely.
                        for _ in 0..events.len() {
                            let Ok(event) = events.try_recv() else {
                                break;
                            };
                            batch.push(event);
                        }
                        // One visible event redraws the batch, wherever it falls in it.
                        handle.update_if(move |s, tx| {
                            batch.into_iter().fold(false, |visible, event| {
                                dispatch_visible(s, tx, Message::Backend(id, event)) | visible
                            })
                        });
                    }
                });
            if let Err(error) = spawned {
                s.alerts.raise(
                    crate::alerts::Source::Worker,
                    format!("Could not receive audio worker events: {error}"),
                );
            }
        }
    }
}

/// Where the pointer is during a drag, in window points. Measured from the press, because the
/// row under it can change or move, and moving over another row updates `row_origin`.
pub(crate) fn drag_point(s: &mut App, d: &Drag) -> (f32, f32) {
    if d.phase == Phase::Down {
        s.ui.press_point = (s.ui.row_origin.0 + d.x, s.ui.row_origin.1 + d.y);
    }
    (s.ui.press_point.0 + d.dx, s.ui.press_point.1 + d.dy)
}

/// Delivers global shortcut presses to the UI thread.
pub fn forward_shortcuts(
    handle: Option<rune::AppHandle<App>>,
    mut events: mpsc::UnboundedReceiver<shortcuts::Action>,
) {
    let Some(handle) = handle else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("flexspeak-shortcuts".into())
        .spawn(move || {
            while let Some(action) = events.blocking_recv() {
                handle.update(move |s, tx| dispatch(s, tx, Message::Shortcut(action)));
            }
        });
}

fn setup(s: &mut App, waker: Waker) {
    s.server().streaming.surfaces.set_waker(waker);
}

/// Sampling timers and video frame bookkeeping. True only when something visible changed.
fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    crate::window_icon::apply();
    let mut changed = false;
    let now = Instant::now();
    if now >= s.next_usage_tick {
        let resources = footer::resources(s);
        let connection = s.footer_connection_status();
        let old_style = style(s);
        // Histories and time-dependent member health remain visible on these pages.
        let ticking_view = s.tab == Tab::Statistics
            || (s.server().connected && (!s.footer_hidden || s.tab == Tab::Home))
            || s.onboarding.is_some()
            || s.settings_page.is_some()
            || s.recording.is_some();
        dispatch(s, tx, Message::UsageTick);
        // Following the system's appearance costs one cheap query per tick.
        if s.appearance.scheme == crate::settings::ColorScheme::System
            && let Some(dark) = crate::system_appearance::is_dark()
        {
            s.system_dark = dark;
        }
        // Kept through the moment a theme switch has the folder swapped out.
        if let Some(current) = crate::omarchy::poll(&s.theme_library)
            && s.omarchy.as_ref() != Some(&current)
        {
            s.omarchy = Some(current);
        }
        s.next_usage_tick = now + usage::REFRESH_INTERVAL;
        changed |= ticking_view
            || connection != s.footer_connection_status()
            || resources != footer::resources(s)
            || !std::rc::Rc::ptr_eq(&old_style, &style(s));
    }
    if s.resume.is_some() && !s.resume_started {
        s.resume_started = true;
        dispatch(s, tx, Message::Resume);
        changed = true;
    }
    // Once, at launch. A restart for an update already returns to where you were.
    if !std::mem::replace(&mut s.auto_connect_checked, true)
        && !s.resume_started
        && s.onboarding.is_none()
    {
        dispatch(s, tx, Message::AutoConnect);
        changed = true;
    }
    if s.updater.next_check.is_some_and(|at| now >= at) {
        dispatch(s, tx, Message::Updater(crate::updater::Event::Check));
        changed = true;
    }
    if now >= s.ui.next_quick_check && s.live() {
        s.ui.next_quick_check = now + QUICK_SOURCE_REFRESH;
        dispatch(s, tx, Message::RefreshQuickSources);
    }
    // One-off failures fade on their own.
    changed |= s.alerts.expire(now);
    let wake = [s.updater.next_check, s.alerts.next_expiry()]
        .into_iter()
        .flatten()
        .fold(s.next_usage_tick, Instant::min);
    tx.wake_after(wake.saturating_duration_since(now));
    if s.server_mut().streaming.sync_frame() {
        changed = true;
    }
    changed | s.ui.rail_peek.expired(now)
}

fn on_close(s: &mut App, tx: &mut Tx) {
    dispatch(s, tx, Message::Close);
}

fn on_key(s: &mut App, key: &rune::KeyEvent, tx: &mut Tx) -> bool {
    // Focus left the recorder, as a click elsewhere does: it's still listening.
    if s.key_capture.is_some() {
        let recorded = shortcuts::record(key, tx.physical_key());
        dispatch(s, tx, Message::Recorded(recorded));
        return true;
    }
    // ⌘, opens Settings, as in every Mac app.
    if key.is_command(',') && s.onboarding.is_none() && s.settings_page.is_none() {
        dispatch(s, tx, Message::OpenSettings(s.settings_recent));
        return true;
    }
    // Escape leaves the log's search, wherever the focus is.
    if key.key == rune::Key::Escape && s.server().log.searching && s.settings_page.is_none() {
        dispatch(s, tx, Message::LogSearch(false));
        return true;
    }
    if key.key == rune::Key::Escape && s.ui.rail_peek.unpin() {
        return true;
    }
    // Escape drops a member selection first.
    if key.key == rune::Key::Escape
        && !s.server().selected_members.is_empty()
        && s.settings_page.is_none()
        && s.ui.member_menu.is_none()
        && s.server().moderation.is_none()
    {
        dispatch(s, tx, Message::ClearSelection);
        return true;
    }
    // Escape returns from a utility page to the server, as the back button does.
    if key.key == rune::Key::Escape
        && s.onboarding.is_none()
        && s.settings_page.is_none()
        && s.tab != Tab::Home
        && s.server().password_channel.is_none()
    {
        dispatch(s, tx, Message::ShowTab(Tab::Home));
        return true;
    }
    false
}

/// The palette for the chosen scheme and theme.
pub fn theme(s: &App) -> Theme {
    if !s.appearance.ignore_omarchy
        && let Some(current) = &s.omarchy
    {
        return Theme::extra(&current.colors, current.dark);
    }
    let dark = s.appearance.is_dark(s.system_dark);
    s.appearance
        .extra_theme(dark)
        .and_then(|id| s.theme_library.find(id)?.colors(dark))
        .map_or_else(
            || Theme::new(s.appearance.theme(dark), dark),
            |colors| Theme::extra(colors, dark),
        )
}

type CachedStyle = (
    crate::settings::Appearance,
    bool,
    Option<crate::themes::Colors>,
    std::rc::Rc<(Theme, rune_kit::Kit)>,
);

fn style(s: &App) -> std::rc::Rc<(Theme, rune_kit::Kit)> {
    thread_local! {
        static STYLE: std::cell::RefCell<Option<CachedStyle>> = const { std::cell::RefCell::new(None) };
    }
    let dark = s.appearance.is_dark(s.system_dark);
    let (dark, colors) = if !s.appearance.ignore_omarchy
        && let Some(current) = &s.omarchy
    {
        (current.dark, Some(current.colors))
    } else {
        (
            dark,
            s.appearance
                .extra_theme(dark)
                .and_then(|id| s.theme_library.find(id)?.colors(dark))
                .copied(),
        )
    };
    STYLE.with_borrow_mut(|cache| {
        if let Some((appearance, cached_dark, cached_colors, value)) = cache
            && appearance == &s.appearance
            && *cached_dark == dark
            && *cached_colors == colors
        {
            return value.clone();
        }
        let t = theme(s);
        let value = std::rc::Rc::new((t, t.kit()));
        *cache = Some((s.appearance.clone(), dark, colors, value.clone()));
        value
    })
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let _trace = crate::profiler::scope(crate::profiler::Category::Ui, "ui.view");
    // Left off, the system's own Reduce Motion setting still applies.
    let reduced = s.appearance.disable_animations || cx.system_reduced_motion();
    cx.reduced_motion(reduced);
    let style = style(s);
    let t = style.0;
    let k = &style.1;
    let mut layer = Vec::new();
    let content = if let Some(step) = s.onboarding {
        onboarding::view(s, cx, &t, k, &mut layer, step)
    } else {
        home::normal(s, cx, &t, k, &mut layer)
    };
    #[cfg(windows)]
    let content = if s.onboarding.is_some() {
        col()
            .w_full()
            .h_full()
            .child(rune::row().w_full().h(theme::size::HEADER).window_drag())
            .child(rune::el().grow(1.0).min_h(0.0).w_full().child(content))
    } else {
        content
    };
    settings::window(s, cx, &t, k, &mut layer);
    // Over Settings, so a problem there shows; under any dialog, whose scrim covers it too.
    layer.extend(notices::stack(s, &t));
    modals::build(s, &t, k, &mut layer);
    let base = el()
        .key("window")
        .w_full()
        .h_full()
        .fill(t.canvas)
        // Settings reads it for its margins. Recording it must not commit: a committing layout
        // handler would re-run layout every frame.
        .on_layout_tx(|s: &mut App, r: [f32; 4], tx: &mut Tx| {
            s.window_size = (r[2], r[3]);
            tx.live_only();
        });
    let base = base.child(
        col()
            .key("content")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .child(content),
    );
    #[cfg(windows)]
    layer.push(window_controls(s, &t));
    if s.ui.warm_blur {
        layer.push(blur_warm_up(&t));
    }
    rune_kit::root(base, layer)
        .hover_motion_within(rune::Spring::new(0.12, 0.0), rune::Spring::new(0.28, 0.0))
}

/// A one-point speck of frosted glass in the canvas colour, for the first frame only. The
/// renderer builds its blur pipelines the first time it draws one, which froze the window for
/// a moment when Settings first opened; drawn here, launch absorbs that wait instead. The fade
/// makes it a layer, as Settings is while it opens, which has a pipeline of its own.
fn blur_warm_up(t: &Theme) -> El {
    el().key("blur-warm-up")
        .absolute(0.0, 0.0)
        .size(1.0, 1.0)
        .opacity(0.5)
        .fill(t.canvas)
        .a11y_hidden()
        .child(el().size(1.0, 1.0).backdrop_blur(1.0).fill(t.canvas))
}

/// Opens the window and runs until the app quits.
pub fn run() {
    rune::prewarm_gpu();
    // Wayland app id and X11 class: desktops match it to flexspeak.desktop and its icon.
    rune::set_app_id("flexspeak");
    let spec = AppSpec::<App> {
        min_size: Some(crate::MIN_SIZE),
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: scenes::SCENES,
        ..AppSpec::new("Flexspeak", (900.0, 790.0), placeholder, view)
    };
    let app = rune::App::new(spec);
    let handle = app.handle();
    app.init(move || App::new(Some(handle)))
        // macOS: the header is the titlebar. The traffic lights sit centred in its 52 pt band
        // and only `window_drag` regions move the window, so header controls stay clickable.
        .chrome(rune::Chrome {
            full_size_content: true,
            traffic_lights: Some((20.0, 19.0)),
            drag_regions: true,
        })
        .window_options(rune::WindowOptions {
            borderless: cfg!(windows),
            ..Default::default()
        })
        .compositor(fx::Effects::default())
        .keep_running()
        .on_close(on_close)
        .run();
}

/// `AppSpec` needs an `init`; the real state comes from `App::init`, which has the handle.
fn placeholder() -> App {
    App::with_devices(crate::audio::Devices::default(), None)
}

/// Flags handled by Rune's headless tooling rather than the app.
pub fn is_tool_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--snap"
            | "--scene"
            | "--list-scenes"
            | "--tree"
            | "--script"
            | "--trace"
            | "--frames"
            | "--video"
            | "--size"
            | "--scale"
            | "--at"
            | "--out"
            | "--strip"
    )
}

/// Keep native window actions reachable in setup, Settings and modal dialogs too. They follow
/// Windows 11's caption buttons: 46 pt wide, thin 10 pt glyphs, a soft wash on hover, and the
/// system's red under a white glyph on Close. Maximize turns into Restore while maximized.
#[cfg(windows)]
fn window_controls(s: &App, t: &Theme) -> El {
    use rune::{Color, icon, row};
    let control = |key: &str, label: &str| {
        el().key(key.to_owned())
            .role("button")
            .label(label.to_owned())
            .help(label.to_owned())
            .size(46.0, theme::size::HEADER)
            .center()
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(t.wash(if t.dark { 0.06 } else { 0.05 }))
            .press(t.wash(if t.dark { 0.04 } else { 0.03 }))
    };
    let maximized = crate::window_icon::is_maximized();
    let glyph = if s.ui.close_hover {
        Color::WHITE
    } else {
        t.text
    };
    row()
        .key("window-controls")
        .pin(None, Some(0.0), Some(0.0), None)
        .child(
            control("window-minimize", "Minimize")
                .child(
                    icon(&icons::CAPTION_MINIMIZE, 10.0)
                        .color(t.text)
                        .a11y_hidden(),
                )
                .on_press_tx(|_: &mut App, _| crate::window_icon::minimize()),
        )
        .child(
            control(
                "window-maximize",
                if maximized {
                    "Restore down"
                } else {
                    "Maximize"
                },
            )
            .child(
                icon(
                    if maximized {
                        &icons::CAPTION_RESTORE
                    } else {
                        &icons::CAPTION_MAXIMIZE
                    },
                    10.0,
                )
                .color(t.text)
                .a11y_hidden(),
            )
            .on_press_tx(|_: &mut App, _| crate::window_icon::toggle_maximized()),
        )
        .child(
            control("window-close", "Close Flexspeak")
                .hover(theme::rgb(0xc42b1c))
                .press(theme::rgb(0xc42b1c).alpha(0.9))
                .on_hover_change(|s: &mut App, hovering, _| s.ui.close_hover = hovering)
                .child(icon(&icons::CAPTION_CLOSE, 10.0).color(glyph).a11y_hidden())
                .on_press_tx(send(Message::Close)),
        )
}
