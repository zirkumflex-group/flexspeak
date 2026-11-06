//! The Rune interface: the window, effects that leave the UI thread, and every view.
//!
//! `App::update` keeps the application's rules. Views turn presses into `Message`s through
//! [`send`]; [`dispatch`] runs the message, carries out the returned [`Task`] with the frame's
//! `Tx`, and mirrors the app's text values into the fields that edit them.

mod charts;
mod footer;
mod home;
pub mod icons;
mod modals;
mod onboarding;
mod scenes;
mod settings;
mod statistics;
mod streams;
mod theme;
mod widgets;

use std::{sync::atomic::Ordering, time::Instant};

use rune::{AppSpec, Cx, El, TextEdit, Tx, Waker, col, el};
use tokio::sync::mpsc;

use crate::{App, Message, Tab, client, shortcuts, usage};

pub use theme::Theme;

/// What an update asks the window or a worker thread to do after it returns.
pub enum Task {
    None,
    Batch(Vec<Task>),
    Exit,
    Copy(String),
    Focus(Field),
    Resize((f32, f32)),
    /// Blocking work for a worker thread; its message returns to the UI thread.
    Perform(Box<dyn FnOnce() -> Message + Send>),
    /// A session's backend events, forwarded until the worker closes the channel.
    Events(mpsc::Receiver<client::Event>),
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
                Err(error) => Message::Backend(client::Event::Warning {
                    source: crate::alerts::Source::Worker,
                    detail: format!("Could not start a background task: {error}"),
                }),
            }
        }))
    }
}

/// Text fields whose keyboard focus an update can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    ChannelPassword,
}

/// Interface-only state: field editors and the open picker.
pub struct UiState {
    pub address: TextEdit,
    pub nickname: TextEdit,
    pub password: TextEdit,
    pub channel_password: TextEdit,
    pub mute_key: TextEdit,
    pub deafen_key: TextEdit,
    pub ptt_key: TextEdit,
    /// The device or source picker whose list is open.
    pub open: Option<&'static str>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            address: TextEdit::new(""),
            nickname: TextEdit::new(""),
            password: TextEdit::new("").secure(),
            channel_password: TextEdit::new("").secure(),
            mute_key: TextEdit::new(""),
            deafen_key: TextEdit::new(""),
            ptt_key: TextEdit::new(""),
            open: None,
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
        mirror(&mut self.ui.channel_password, &self.channel_password);
        mirror(&mut self.ui.mute_key, &self.keybind_draft.toggle_mute);
        mirror(&mut self.ui.deafen_key, &self.keybind_draft.toggle_deafen);
        mirror(&mut self.ui.ptt_key, &self.keybind_draft.push_to_talk);
    }
}

/// Runs one message on the UI thread and carries out what it returns.
pub fn dispatch(s: &mut App, tx: &mut Tx, message: Message) {
    let task = s.update(message);
    execute(s, tx, task);
    s.sync_fields();
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
        Task::Resize(size) => {
            tx.resize_window(size);
            s.window_size = size;
        }
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
        Task::Events(mut events) => {
            let Some(handle) = s.handle.clone() else {
                return;
            };
            let spawned = std::thread::Builder::new()
                .name("flexspeak-events".into())
                .spawn(move || {
                    while let Some(event) = events.blocking_recv() {
                        handle.update(move |s, tx| dispatch(s, tx, Message::Backend(event)));
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
    s.streaming.surface.set_waker(waker);
}

/// Sampling timers and video frame bookkeeping. True only when something visible changed.
fn poll(s: &mut App, _: f64, tx: &mut Tx) -> bool {
    crate::window_icon::apply();
    let mut changed = false;
    let now = Instant::now();
    if now >= s.next_usage_tick {
        let compact = s.performance_mode.load(Ordering::Relaxed);
        dispatch(
            s,
            tx,
            if compact {
                Message::CompactUsageTick
            } else {
                Message::UsageTick
            },
        );
        s.next_usage_tick = now
            + if compact {
                usage::COMPACT_REFRESH_INTERVAL
            } else {
                usage::REFRESH_INTERVAL
            };
        changed = true;
    }
    tx.wake_after(s.next_usage_tick.saturating_duration_since(now));
    if s.streaming.sync_frame() {
        changed = true;
    }
    changed
}

fn on_close(s: &mut App, tx: &mut Tx) {
    dispatch(s, tx, Message::Close);
}

fn on_key(s: &mut App, key: &rune::KeyEvent, tx: &mut Tx) -> bool {
    // Escape returns from a utility page to the server, as the back button does.
    if key.key == rune::Key::Escape
        && s.onboarding.is_none()
        && s.tab != Tab::Home
        && s.password_channel.is_none()
        && s.alerts.current().is_none()
    {
        dispatch(s, tx, Message::ShowTab(Tab::Home));
        return true;
    }
    false
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let _trace = crate::profiler::scope(crate::profiler::Category::Ui, "ui.view");
    let t = Theme::new();
    let k = t.kit();
    let mut layer = Vec::new();
    let compact = s.performance_mode.load(Ordering::Relaxed);
    let content = if let Some(step) = s.onboarding {
        onboarding::view(s, cx, &t, &k, &mut layer, step)
    } else if compact {
        home::performance(s, &t, &k)
    } else {
        home::normal(s, cx, &t, &k, &mut layer)
    };
    modals::build(s, &t, &k, &mut layer);
    let base = el()
        .key("window")
        .w_full()
        .h_full()
        .fill(t.canvas)
        // Remembered for performance mode's restore. The view never reads it, so recording it
        // must not commit: a committing layout handler would re-run layout every frame.
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
    rune_kit::root(base, layer)
        .hover_motion_within(rune::Spring::new(0.12, 0.0), rune::Spring::new(0.28, 0.0))
}

/// Opens the window and runs until the app quits.
pub fn run() {
    rune::prewarm_gpu();
    // Wayland app id and X11 class: desktops match it to flexspeak.desktop and its icon.
    rune::set_app_id("flexspeak");
    let spec = AppSpec::<App> {
        min_size: Some(crate::COMPACT_MIN_SIZE),
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
