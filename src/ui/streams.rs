//! The connected right pane: streams in your channel, watched on a stage that fills the pane.
//!
//! One watched stream fills the stage. With several, they share it in a grid until you focus
//! one; the focused stream takes the stage and the others line up below or beside it. Dragging
//! one stream onto another trades their places. Each stream has its own volume. Codec and
//! transport details hide behind the info button.
//! Sharing your own screen starts from the empty pane's own buttons or the one beside
//! Disconnect, in [`share_dialog`].

use rune::{Color, Cx, Drag, El, Phase, SurfaceFit, Transition, Tx, col, el, icon, row, text};
use rune_kit::{Checkbox, Dialog, Kit, SegmentedControl};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Range, Variant, button, choice, divider, faint, icon_button, lit_dot, meter, muted, picker,
    pulse, scroll_page, slider,
};
use super::{TileDrag, dispatch, icons, theme};
use crate::streaming::{
    self, Strip, Watching, health, publishing,
    publishing::{
        Access, Preset, PreviewState, Resolution, ShareSource, SourceGroup, SourceKind, Struggle,
    },
};
use crate::{App, Message, usage};

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let connected = s.server().connected && !s.loopback && !s.server().stopping;
    let supported = connected && s.server().state.server_version.starts_with("6.");
    share_dialog(s, cx, t, k, layer);
    drag_ghost(s, t, layer);
    let watching = s.server().streaming.active();
    let mut pane = col().key("streams").w_full().h_full();
    if on_stage(s) {
        let room = pane_size(s, cx);
        pane = pane.child(
            col()
                .key("stage-area")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .on_hover_change(|s: &mut App, hovering, _| {
                    s.ui.streams_hover = hovering;
                    s.ui.streams_moved = std::time::Instant::now();
                    s.ui.streams_idle = false;
                })
                .on_pointer_move_local(|s: &mut App, _, tx: &mut Tx| {
                    s.ui.streams_moved = std::time::Instant::now();
                    if !std::mem::take(&mut s.ui.streams_idle) {
                        tx.live_only();
                    }
                })
                .child(stage(s, t, k, supported, room))
                .children(overlays(s).then(|| stage_tools(s, t, supported, watching))),
        );
    } else {
        pane = pane
            .child(
                row()
                    .key("streams-header")
                    .w_full()
                    .px(size::PAGE)
                    .pt(20.0)
                    .pb(16.0)
                    .gap(12.0)
                    .child(channel_title(s, t))
                    .child(tools(s, t, supported, watching)),
            )
            .child(no_streams(s, t, supported));
    }
    pane
}

/// Whether streams fill the pane: one you watch, or one in the channel you could.
pub fn on_stage(s: &App) -> bool {
    s.server().streaming.active() || !unwatched(s).is_empty()
}

/// How long the pointer rests on the streams before their toolbar and bar step aside.
pub const OVERLAY_IDLE: std::time::Duration = std::time::Duration::from_secs(3);

/// Whether the overlays over the streams show: the pointer is over them and has moved lately.
fn overlays(s: &App) -> bool {
    s.ui.streams_hover && (!s.ui.streams_idle || s.ui.streams_bar_hover)
}

/// Keeps the overlays up while the pointer is on one of them, even resting.
fn holds_overlays(bar: El) -> El {
    bar.on_hover_change(|s: &mut App, hovering, _| s.ui.streams_bar_hover = hovering)
}

/// Whether a stream could fill the screen: you watch one, on the server's own page.
pub fn fullscreen_allowed(s: &App) -> bool {
    s.tab == crate::Tab::Home
        && s.onboarding.is_none()
        && s.server().connected
        && !s.server().streaming.watching.is_empty()
}

/// Whether the stream pane is all the window shows.
pub fn fullscreen(s: &App) -> bool {
    s.ui.fullscreen.is_some() && fullscreen_allowed(s)
}

fn fullscreen_button(s: &App, t: &Theme) -> El {
    let on = fullscreen(s);
    icon_button(
        t,
        "fullscreen",
        if on { &icons::MINIMIZE } else { &icons::MAXIMIZE },
        if on {
            "Leave full screen (Esc)"
        } else {
            "Full screen (F)"
        },
        Variant::Ghost,
        Some(Message::StreamFullscreen(!on)),
    )
}

/// What can go live with one click right now: your main screen, then the game you're playing.
/// Nothing while you share, or where sharing isn't on offer.
pub fn quick_sources(s: &App) -> &[ShareSource] {
    let session = s.server();
    let available = session.connected
        && !s.loopback
        && !session.stopping
        && session.state.server_version.starts_with("6.")
        && streaming::AVAILABLE
        && !session.streaming.sharing.status.active;
    let sources = if available {
        s.ui.quick_sources.as_slice()
    } else {
        &[]
    };
    &sources[..sources.len().min(3)]
}

/// A quick source's icon: a game is a window, the main screen a screen.
pub fn quick_icon(source: &ShareSource) -> &'static rune::Icon {
    if matches!(source.kind, SourceKind::Window(_)) {
        &icons::GAMEPAD
    } else {
        &icons::MONITOR_UP
    }
}

/// The toolbar floating over the streams, at the top right; each stream's own button is at its
/// top left. The solid, frosted backing keeps the icons readable over any picture.
fn stage_tools(s: &App, t: &Theme, supported: bool, watching: bool) -> El {
    row()
        .key("stage-tools")
        .absolute(0.0, STAGE_PAD + 8.0)
        .w_full()
        .px(STAGE_PAD + 8.0)
        .child(el().grow(1.0))
        .child(
            holds_overlays(tools(s, t, supported, watching))
                .padding(4.0)
                .radius(radius::CARD)
                .fill(t.surface.alpha(0.86))
                .border(1.0, t.hairline)
                .backdrop_blur(12.0)
                .transition(Transition::opacity()),
        )
}

fn tools(s: &App, t: &Theme, supported: bool, watching: bool) -> El {
    let mut bar = row().key("streams-tools").gap(10.0).center_y();
    if watching {
        let on = s.server().streaming.show_stats;
        bar = bar.child(icon_button(
            t,
            "stream-stats",
            &icons::INFO,
            if on {
                "Hide stream details"
            } else {
                "Show stream details: codec, resolution, frame rate, transport"
            },
            if on { Variant::Active } else { Variant::Ghost },
            Some(Message::ToggleStreamStats),
        ));
        if s.server().streaming.watching.len() > 1 {
            let current = s
                .server()
                .streaming
                .stage()
                .is_some()
                .then_some(s.server().streaming.strip);
            for (key, glyph, help, layout) in [
                (
                    "layout-grid",
                    &icons::LAYOUT_GRID,
                    "Show every stream in a grid",
                    None,
                ),
                (
                    "layout-below",
                    &icons::PANEL_BOTTOM,
                    "One stream large, the others below it",
                    Some(Strip::Below),
                ),
                (
                    "layout-beside",
                    &icons::PANEL_RIGHT,
                    "One stream large, the others beside it",
                    Some(Strip::Beside),
                ),
            ] {
                bar = bar.child(icon_button(
                    t,
                    key,
                    glyph,
                    help,
                    if current == layout {
                        Variant::Active
                    } else {
                        Variant::Ghost
                    },
                    Some(Message::StreamLayout(layout)),
                ));
            }
            bar = bar.child(button(
                t,
                "stop-all",
                "Stop All",
                Some(&icons::STOP),
                Variant::Secondary,
                Some(Message::StopWatching(None)),
            ));
        }
    }
    bar.child(icon_button(
        t,
        "refresh-streams",
        &icons::REFRESH,
        "Look for streams again",
        Variant::Ghost,
        supported.then_some(Message::RefreshStreams),
    ))
}

/// The channel you are in, and how many are in it with you.
fn channel_title(s: &App, t: &Theme) -> El {
    let state = &s.server().state;
    let Some(channel) = state.current.as_ref() else {
        return el().grow(1.0);
    };
    let count = state.current_members().len();
    col()
        .key("channel-title")
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(
            text(channel.name.clone())
                .font(ty::TITLE)
                .color(t.text)
                .truncate_fade(),
        )
        .child(
            text(if count == 1 {
                "1 member".to_owned()
            } else {
                format!("{count} members")
            })
            .font(ty::SMALL)
            .color(t.muted),
        )
}

/// Streams in the channel that you aren't watching yet.
fn unwatched(s: &App) -> Vec<&streaming::StreamInfo> {
    s.server()
        .streaming
        .catalog
        .iter()
        .filter(|stream| s.server().streaming.watching(&stream.id).is_none())
        .collect()
}

fn publisher_name(s: &App, id: u16) -> String {
    s.server()
        .state
        .current_members()
        .iter()
        .find(|member| member.id == id)
        .map_or_else(|| "Someone".to_owned(), |member| member.name.clone())
}

/// Why Watch is unavailable for this stream, if it is.
fn watch_blocked(s: &App, supported: bool, stream: &streaming::StreamInfo) -> Option<&'static str> {
    if !supported || !streaming::AVAILABLE {
        Some("Unavailable here")
    } else if stream.server_relayed {
        Some("Server relay unavailable")
    } else if s.server().streaming.watching.len() >= streaming::MAX_WATCHES {
        Some("Watching the most at once")
    } else {
        None
    }
}

/// A source's name, cut short so its button stays a button.
fn short(name: &str) -> String {
    const MAX: usize = 28;
    if name.chars().count() <= MAX {
        return name.to_owned();
    }
    let cut: String = name.chars().take(MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// The ways to start sharing, in the empty pane: each quick source goes live with one click,
/// always public, and the last button opens the dialog for everything else.
fn go_live(s: &App, t: &Theme) -> El {
    let quick = quick_sources(s);
    let mut actions = row()
        .key("go-live")
        .gap(size::GAP)
        .mt(4.0)
        .center()
        .flex_wrap();
    for (index, source) in quick.iter().enumerate() {
        actions = actions.child(
            button(
                t,
                &format!("go-live-{index}"),
                short(&source.name),
                Some(quick_icon(source)),
                if index == 0 {
                    Variant::Primary
                } else {
                    Variant::Secondary
                },
                Some(Message::QuickShare(source.clone())),
            )
            .help(format!("Go live publicly: {}", source.name)),
        );
    }
    let only = quick.is_empty();
    actions.child(
        button(
            t,
            "go-live-options",
            if only {
                "Share Screen…"
            } else {
                "More Options…"
            },
            only.then_some(&icons::MONITOR_UP),
            if only {
                Variant::Primary
            } else {
                Variant::Ghost
            },
            Some(Message::OpenShare),
        )
        .help("Choose a screen, window or camera, its quality and who may watch"),
    )
}

/// What the pane shows while no stream is running in the channel, centred in the space left
/// below the header however wide the window is: the ways to start one, or your own share
/// while it is the only one.
fn no_streams(s: &App, t: &Theme, supported: bool) -> El {
    let sharing = s.server().streaming.sharing.status.active;
    let mut state = col()
        .key("no-streams")
        .grow(1.0)
        .w_full()
        // A little above the middle reads as centred; the header already sits on top.
        .pb(56.0)
        .gap(10.0)
        .center()
        // One line and the ways to go live; the rules live in the tooltip.
        .child(
            text(if sharing {
                "Only you are sharing in this channel."
            } else {
                "No one in this channel is sharing."
            })
            .font(ty::BODY)
            .color(t.muted)
            .help(
                "Watch up to five streams at once. Switching channels stops watching and \
                 sharing; switching servers stops watching only.",
            ),
        );
    if sharing {
        state = state.child(share_summary(
            s,
            text(share_summary_text(s))
                .font(ty::SMALL)
                .color(t.faint)
                .align(rune::Align::Center),
        ));
    } else if supported && streaming::AVAILABLE {
        state = state.child(go_live(s, t));
    }
    let mut content = col()
        .key("no-streams-area")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .px(size::PAGE)
        .pb(size::PAGE)
        .gap(10.0);
    if !supported {
        content = content.child(notice(t).child(muted(
            t,
            "This server does not advertise TeamSpeak 6. Server-channel streaming is unavailable.",
        )));
    } else if !streaming::AVAILABLE {
        content = content.child(notice(t).child(muted(
            t,
            "Native screen sharing is available on Mac and Windows.",
        )));
    }
    content.child(state)
}

/// Your own share in a few words: how many watch it and who may. Until it is live, what it is
/// waiting for. The encoder and capture details move to the tooltip and the share dialog.
fn share_summary(s: &App, line: El) -> El {
    let detail = &s.server().streaming.sharing.status.detail;
    if detail.is_empty() {
        line
    } else {
        line.help(detail.clone())
    }
}

fn share_summary_text(s: &App) -> String {
    let status = &s.server().streaming.sharing.status;
    if !status.live && !status.detail.is_empty() {
        return status.detail.clone();
    }
    let watching = match status.viewers.len() {
        0 => "No one watching yet".to_owned(),
        count => format!("{count} of {} watching", streaming::publishing::MAX_VIEWERS),
    };
    format!("{watching} · {}", status.access.label())
}

/// A note on the main pane, recessed into it.
fn notice(t: &Theme) -> El {
    super::widgets::well(t).padding(size::CARD_PAD)
}

/// Your own share while it runs, for the voice panel: viewers, requests to watch, and the way to
/// stop. The source comes last so a narrow sidebar only loses the end of it.
pub fn sharing_status(s: &App, t: &Theme) -> El {
    let sharing = &s.server().streaming.sharing;
    let status = &sharing.status;
    let mut tile = col()
        .key("sharing-status")
        .w_full()
        .gap(6.0)
        .padding(8.0)
        .radius(radius::ROW)
        .fill(t.live.alpha(0.08))
        .border(1.0, t.live.alpha(0.35))
        .transition(Transition::opacity())
        .child(
            row()
                .w_full()
                .gap(6.0)
                .center_y()
                .child(lit_dot(t.live))
                .child(
                    text("You are sharing")
                        .font(ty::BODY_STRONG)
                        .color(t.text)
                        .grow(1.0)
                        .min_w(0.0)
                        .truncate_fade(),
                )
                .child(share_audio_toggle(t, sharing))
                .child(icon_button(
                    t,
                    "manage-share",
                    &icons::SETTINGS,
                    "Stream settings: Performance or Quality, audio, and who watches",
                    Variant::Ghost,
                    Some(Message::OpenShare),
                ))
                .child(icon_button(
                    t,
                    "stop-sharing",
                    &icons::STOP,
                    "Stop sharing",
                    Variant::Danger,
                    Some(Message::StopSharing),
                )),
        )
        .child(share_summary(
            s,
            faint(t, share_summary_text(s)).truncate_fade(),
        ));
    if sharing.audio && status.audio_frames > 0 {
        tile = tile.child(meter(t, "share-audio", status.audio_level.min(1.0), t.live));
    }
    for (id, name) in &status.requests {
        tile = tile.child(request_row(t, status, *id, name));
    }
    tile
}

/// Whether your share sends system audio, lit while it does; a press turns it on or off
/// without stopping the share.
fn share_audio_toggle(t: &Theme, sharing: &streaming::publishing::Ui) -> El {
    let on = sharing.audio;
    let help = if !on {
        "System audio is not shared. Share it"
    } else if sharing.status.audio_level > 0.01 {
        "Sharing system audio: sound is going out. Stop sharing it"
    } else {
        "Sharing system audio: nothing is playing right now. Stop sharing it"
    };
    icon_button(
        t,
        "share-audio-toggle",
        if on {
            &icons::VOLUME_2
        } else {
            &icons::VOLUME_OFF
        },
        help,
        if on { Variant::Active } else { Variant::Ghost },
        Some(Message::ShareAudio(!on)),
    )
}

/// A request to watch on one line, for the wide share dialog.
fn request_inline(t: &Theme, status: &streaming::publishing::Status, id: u16, name: &str) -> El {
    row()
        .key(format!("request-{id}"))
        .w_full()
        .gap(8.0)
        .center_y()
        .transition(Transition::opacity().combined(Transition::scale(0.98)))
        .child(pulse(t.fair))
        .child(
            text(format!("{name} wants to watch"))
                .font(ty::BODY)
                .color(t.text)
                .grow(1.0)
                .min_w(0.0)
                .truncate_fade(),
        )
        .child(button(
            t,
            "allow",
            "Allow",
            None,
            Variant::Primary,
            (status.viewers.len() < streaming::publishing::MAX_VIEWERS)
                .then_some(Message::DecideViewer(id, true)),
        ))
        .child(button(
            t,
            "deny",
            "Deny",
            None,
            Variant::Secondary,
            Some(Message::DecideViewer(id, false)),
        ))
}

fn request_row(t: &Theme, status: &streaming::publishing::Status, id: u16, name: &str) -> El {
    col()
        .key(format!("request-{id}"))
        .w_full()
        .gap(6.0)
        .transition(Transition::opacity().combined(Transition::scale(0.98)))
        .child(
            text(format!("{name} wants to watch"))
                .font(ty::BODY_STRONG)
                .color(t.text)
                .truncate_fade(),
        )
        .child(
            row()
                .w_full()
                .gap(6.0)
                .child(el().grow(1.0))
                .child(button(
                    t,
                    "allow",
                    "Allow",
                    None,
                    Variant::Primary,
                    (status.viewers.len() < streaming::publishing::MAX_VIEWERS)
                        .then_some(Message::DecideViewer(id, true)),
                ))
                .child(button(
                    t,
                    "deny",
                    "Deny",
                    None,
                    Variant::Secondary,
                    Some(Message::DecideViewer(id, false)),
                )),
        )
}

/// Streams watched and not yet joined: one on the stage with the rest beneath it, or a grid.
fn stage(s: &App, t: &Theme, k: &Kit, supported: bool, space: (f32, f32)) -> El {
    let watching = &s.server().streaming.watching;
    let available = unwatched(s);
    let mut area = col()
        .key("stage")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .padding(STAGE_PAD)
        .gap(10.0);
    if solo(s) {
        // One stream and nothing else to show: it fills the pane edge to edge, as a player does.
        let main = s
            .server()
            .streaming
            .stage()
            .expect("a lone watched stream is on the stage");
        let width = space.0 + 2.0 * STAGE_PAD;
        return area.padding(0.0).child(tile(s, t, main, Mode::Solo, width));
    }
    let Some(main) = s.server().streaming.stage() else {
        let columns = grid_columns(watching.len() + available.len(), space);
        let width = cell_width(space, columns.unwrap_or(2));
        let cells = watching
            .iter()
            .map(|watch| tile(s, t, watch, Mode::Grid, width))
            .chain(
                available
                    .iter()
                    .map(|stream| preview(s, t, stream, supported, Mode::Grid)),
            )
            .collect::<Vec<_>>();
        return area.child(grid(k, cells, columns));
    };
    let others: Vec<&Watching> = watching
        .iter()
        .filter(|watch| watch.generation != main.generation)
        .collect();
    let room = MAX_STRIP.saturating_sub(others.len()).min(available.len());
    let (previews, overflow) = available.split_at(room);
    let thumbnails: Vec<El> = others
        .into_iter()
        .map(|watch| tile(s, t, watch, Mode::Thumbnail, 0.0))
        .chain(
            previews
                .iter()
                .map(|stream| preview(s, t, stream, supported, Mode::Thumbnail)),
        )
        .collect();
    let strip_width = (space.0 * 0.22).clamp(140.0, 280.0);
    let stage_width = match s.server().streaming.strip {
        Strip::Beside if !thumbnails.is_empty() => space.0 - strip_width - GRID_GAP,
        _ => space.0,
    };
    let main = tile(s, t, main, Mode::Stage, stage_width);
    area = match s.server().streaming.strip {
        Strip::Below if thumbnails.is_empty() => area.child(main),
        Strip::Below => area.child(main).child(
            row()
                .key("strip")
                .w_full()
                .h(STRIP_HEIGHT)
                .gap(10.0)
                .children(thumbnails),
        ),
        Strip::Beside => {
            let mut stage = row()
                .key("stage-row")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .gap(10.0)
                .child(main);
            if !thumbnails.is_empty() {
                stage = stage.child(
                    col()
                        .key("strip")
                        .w(strip_width)
                        .h_full()
                        .gap(10.0)
                        .children(thumbnails),
                );
            }
            area.child(stage)
        }
    };
    if !overflow.is_empty() {
        let mut chips = row()
            .key("also-live")
            .w_full()
            .gap(8.0)
            .center_y()
            .flex_wrap()
            .child(faint(t, "Also live:"));
        for stream in overflow {
            let blocked = watch_blocked(s, supported, stream);
            chips = chips.child(
                button(
                    t,
                    &format!("watch-{}", stream.id),
                    format!("{} · {}", publisher_name(s, stream.publisher), stream.name),
                    Some(&icons::MONITOR_PLAY),
                    Variant::Secondary,
                    blocked
                        .is_none()
                        .then(|| Message::WatchStream(stream.id.clone())),
                )
                .help(blocked.unwrap_or("Watch beside the others")),
            );
        }
        area = area.child(chips);
    }
    area
}

/// The margin around several streams on the stage.
const STAGE_PAD: f32 = 12.0;

/// How much of a lone stream may be cropped to fill the pane: 2%, a few pixels at each edge.
const SOLO_CROP: f32 = 0.02;

/// Whether the stream on the stage is all the pane shows: it is the only one, or the others
/// are tucked away.
fn solo(s: &App) -> bool {
    s.server().streaming.stage().is_some() && (others(s) == 0 || s.server().streaming.others_hidden)
}

/// The streams around the one on the stage: the others you watch and those you could.
fn others(s: &App) -> usize {
    let streaming = &s.server().streaming;
    if streaming.stage().is_none() {
        return 0;
    }
    streaming.watching.len() - 1 + unwatched(s).len()
}

/// The tab at the foot of a stream on its own that brings the tucked-away streams back.
fn show_others(t: &Theme, count: usize) -> El {
    button(
        t,
        "show-others",
        if count == 1 {
            "1 more stream".to_owned()
        } else {
            format!("{count} more streams")
        },
        Some(&icons::CHEVRON_UP),
        Variant::Secondary,
        Some(Message::HideOtherStreams(false)),
    )
    .help("Show the other streams again")
}

/// Tiles that fit beneath the stage; further unjoined streams become "Also live" chips.
const MAX_STRIP: usize = 4;
const STRIP_HEIGHT: f32 = 132.0;
const SCROLLED_ROW_HEIGHT: f32 = 240.0;
const GRID_GAP: f32 = 10.0;
/// The name and controls under a picture.
const TILE_BAR: f32 = 46.0;
/// Below this picture width the grid stops squeezing streams in and scrolls instead.
const MIN_PICTURE_WIDTH: f32 = 200.0;
/// Everything around the stage in the window: header, footer and margins.
const CHROME_HEIGHT: f32 = 150.0;

/// The room inside the stage. Reading its layout re-runs the view when the window resizes;
/// until it has been laid out, estimate from the window.
fn pane_size(s: &App, cx: &mut Cx<App>) -> (f32, f32) {
    cx.layout_size("stage").map_or_else(
        || {
            let (width, height) = s.window_size;
            (
                width - size::SIDEBAR - 2.0 * STAGE_PAD,
                height - CHROME_HEIGHT,
            )
        },
        |[width, height]| (width - 2.0 * STAGE_PAD, height - 2.0 * STAGE_PAD),
    )
}

fn cell_width(room: (f32, f32), columns: usize) -> f32 {
    let columns = columns as f32;
    (room.0 - GRID_GAP * (columns - 1.0)) / columns
}

/// How wide a 16:9 picture gets with this many columns.
fn picture_width(count: usize, columns: usize, (width, height): (f32, f32)) -> f32 {
    let rows = count.div_ceil(columns) as f32;
    let cell_height = (height - GRID_GAP * (rows - 1.0)) / rows - TILE_BAR;
    cell_width((width, height), columns).min(cell_height * 16.0 / 9.0)
}

/// The column count that makes the pictures largest, or `None` when even that is too small
/// to share the pane and the grid should scroll.
fn grid_columns(count: usize, pane: (f32, f32)) -> Option<usize> {
    let best = (1..=count.clamp(1, 3))
        .max_by(|a, b| picture_width(count, *a, pane).total_cmp(&picture_width(count, *b, pane)))?;
    (picture_width(count, best, pane) >= MIN_PICTURE_WIDTH).then_some(best)
}

/// Rows of equal boxes; a short last row is centred. `None` columns means a scrolling list.
fn grid(k: &Kit, cells: Vec<El>, columns: Option<usize>) -> El {
    let fitted = columns.is_some();
    let columns = columns.unwrap_or(2);
    let mut grid = col().key("grid").w_full().gap(GRID_GAP);
    if fitted {
        grid = grid.grow(1.0).min_h(0.0);
    }
    let mut cells = cells.into_iter().peekable();
    let mut index = 0;
    while cells.peek().is_some() {
        let boxes: Vec<El> = cells.by_ref().take(columns).collect();
        let margin = (columns - boxes.len()) as f32 / 2.0;
        let mut line = row()
            .key(format!("grid-row-{index}"))
            .w_full()
            .gap(GRID_GAP);
        line = if fitted {
            line.grow(1.0).min_h(0.0)
        } else {
            line.h(SCROLLED_ROW_HEIGHT)
        };
        if margin > 0.0 {
            line = line.child(el().grow(margin).min_w(0.0));
        }
        line = line.children(boxes);
        if margin > 0.0 {
            line = line.child(el().grow(margin).min_w(0.0));
        }
        grid = grid.child(line);
        index += 1;
    }
    if fitted {
        grid
    } else {
        scroll_page(k, "streams-scroll", grid)
    }
}

/// How far the pointer travels before a press on a stream becomes a drag.
const DRAG_START: f32 = 4.0;

/// The watched stream whose tile holds an element, from its key path. The longest match wins,
/// so one stream's ID being the start of another's can't pick the wrong one.
fn watched_in_path(s: &App, path: &str) -> Option<String> {
    s.server()
        .streaming
        .watching
        .iter()
        .map(|watch| &watch.stream.id)
        .filter(|id| path.contains(&format!("tile-{id}")))
        .max_by_key(|id| id.len())
        .cloned()
}

/// A click puts the stream on the stage, a double-click on the stage takes it full screen (or
/// back), and a drag onto another stream trades their places. The second click of a
/// double-click on a grid tile lands on the stage it just moved to, so it counts there only
/// when the first click did too.
fn movable(video: El, stream: &streaming::StreamInfo, mode: Mode) -> El {
    let id = stream.id.clone();
    let on_stage = matches!(mode, Mode::Stage | Mode::Solo);
    video
        .role("button")
        .label(format!("Move or focus {}", stream.name))
        .help(if on_stage {
            "Drag onto another stream to swap · double-click for full screen"
        } else {
            "Click to make this the large stream · drag onto another stream to swap"
        })
        .cursor(rune::Cursor::Pointer)
        .on_pointer_move_local(|s: &mut App, p, tx: &mut Tx| {
            s.ui.row_origin = (p.window_x - p.x, p.window_y - p.y);
            tx.live_only();
        })
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            let at = super::drag_point(s, d);
            match d.phase {
                Phase::Down => s.ui.tile_drag = None,
                Phase::Move => {
                    if s.ui.tile_drag.is_none() && d.dx.hypot(d.dy) >= DRAG_START {
                        s.ui.tile_drag = Some(TileDrag {
                            id: id.clone(),
                            at,
                            target: None,
                        });
                    }
                    if let Some(drag) = &mut s.ui.tile_drag {
                        drag.at = at;
                        tx.element_at(at.0, at.1, |s: &mut App, picked, _tx: &mut Tx| {
                            let under = picked.and_then(|p| watched_in_path(s, p.path()));
                            if let Some(drag) = &mut s.ui.tile_drag {
                                drag.target = under.filter(|under| *under != drag.id);
                            }
                        });
                    }
                }
                Phase::Up => {
                    let drag = s.ui.tile_drag.take();
                    if d.cancelled() {
                        return;
                    }
                    match drag {
                        Some(TileDrag {
                            id: from,
                            target: Some(to),
                            ..
                        }) => dispatch(s, tx, Message::SwapStreams(from, to)),
                        Some(_) => {}
                        None if !on_stage => {
                            s.ui.focused_at = Some(std::time::Instant::now());
                            dispatch(s, tx, Message::FocusStream(Some(id.clone())))
                        }
                        None if d.clicks >= 2 && !just_focused(s) => {
                            let on = s.ui.fullscreen.is_none();
                            dispatch(s, tx, Message::StreamFullscreen(on));
                        }
                        None => {}
                    }
                }
            }
        })
}

/// Whether a click just moved a stream onto the stage, so a second click there belongs to it.
fn just_focused(s: &App) -> bool {
    s.ui.focused_at
        .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(700))
}

/// A double-click on a lone stream takes it full screen, or back.
fn double_click_fullscreen(video: El) -> El {
    video
        .help("Double-click for full screen")
        .on_double_press_tx(|s: &mut App, tx: &mut Tx| {
            let on = s.ui.fullscreen.is_none();
            dispatch(s, tx, Message::StreamFullscreen(on));
        })
}

/// The label that follows the pointer while a stream is dragged.
fn drag_ghost(s: &App, t: &Theme, layer: &mut Vec<El>) {
    let Some(drag) = &s.ui.tile_drag else {
        return;
    };
    let name = |id: &str| {
        s.server()
            .streaming
            .watching(id)
            .map_or_else(|| "Stream".to_owned(), |watch| watch.stream.name.clone())
    };
    let label = match &drag.target {
        Some(target) => format!("{} ⇄ {}", name(&drag.id), name(target)),
        None => name(&drag.id),
    };
    // Offset from the pointer, so picking the stream underneath never finds the label.
    layer.push(
        row()
            .key("stream-drag")
            .absolute(drag.at.0 + 14.0, drag.at.1 + 12.0)
            .gap(6.0)
            .px(10.0)
            .py(5.0)
            .center_y()
            .radius(radius::PILL)
            .fill(t.raised)
            .border(1.0, t.accent.alpha(0.5))
            .box_shadow(theme::shadow(0.0, 4.0, 12.0, Color::BLACK.alpha(0.3)))
            .a11y_hidden()
            .child(icon(&icons::MOVE, 13.0).color(t.accent))
            .child(text(label).font(ty::SMALL).color(t.text)),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The only thing in the pane: the picture edge to edge, its bar over it on hover.
    Solo,
    /// The focused stream, filling the stage above or beside the others.
    Stage,
    /// One of several equals.
    Grid,
    /// A small tile under the focused stream.
    Thumbnail,
}

/// `width` is the tile's, which decides whether its volume slider fits beside the name.
fn tile(s: &App, t: &Theme, watch: &Watching, mode: Mode, width: f32) -> El {
    let id = watch.stream.id.clone();
    let several = s.server().streaming.watching.len() > 1;
    let receiving = watch.phase == streaming::Phase::Receiving;
    let mut video = el()
        .key("video")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .fill(Color::BLACK)
        .center();
    video = match &watch.frame {
        Some(frame) => video.child(
            // A lone stream fills the pane, cropping a sliver rather than leaving hairline bars
            // when the pane is a hair off the stream's shape.
            super::video::filling(
                watch.slot,
                frame.rotation,
                if mode == Mode::Solo { SOLO_CROP } else { 0.0 },
            )
            .w_full()
            .h_full(),
        ),
        None => video.child(
            col().gap(10.0).center().child(pulse(t.accent)).child(
                text(watch.phase.label())
                    .font(ty::SMALL)
                    .color(Color::WHITE.alpha(0.7)),
            ),
        ),
    };
    if several {
        video = movable(video, &watch.stream, mode);
    } else {
        video = double_click_fullscreen(video);
    }
    if s.server().streaming.show_stats && mode != Mode::Thumbnail {
        video = video.child(stats(s, watch).absolute(10.0, 10.0));
    }
    if let Some(problem) = s.stream_problem(watch) {
        video = video.child(
            row()
                .key("trouble-corner")
                .absolute(0.0, 0.0)
                .w_full()
                .padding(if mode == Mode::Thumbnail { 6.0 } else { 10.0 })
                // Below the toolbar that floats over the top right while the pointer is here.
                .pt(if overlays(s) && mode != Mode::Thumbnail {
                    TOOLS_CLEARANCE
                } else if mode == Mode::Thumbnail {
                    6.0
                } else {
                    10.0
                })
                .child(el().grow(1.0))
                .child(trouble(t, problem, mode == Mode::Thumbnail)),
        );
    }
    if mode == Mode::Grid {
        video = video.child(
            row()
                .key("maximize")
                .absolute(0.0, 0.0)
                .w_full()
                .padding(6.0)
                .child(icon_button(
                    t,
                    "focus",
                    &icons::ZOOM_IN,
                    "Make this the large stream",
                    Variant::Secondary,
                    Some(Message::FocusStream(Some(id.clone()))),
                ))
                .child(el().grow(1.0)),
        );
    }
    let drag = s.ui.tile_drag.as_ref();
    if drag.is_some_and(|drag| drag.target.as_deref() == Some(id.as_str())) {
        // Over the picture, because the picture covers the box's own border.
        video = video.child(
            el().key("drop")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .fill(t.accent.alpha(0.28))
                .border(2.0, t.accent)
                .center()
                .a11y_hidden()
                .child(text("Swap").font(ty::BODY_STRONG).color(Color::WHITE)),
        );
    }
    if mode == Mode::Solo {
        // No box: the picture is the pane. Its bar floats over the bottom while the pointer is
        // over the stream, and stays while it connects so Stop is always at hand.
        // Tucked-away streams leave a small tab above the bar, shown with it while the pointer is
        // over the stream, so nothing covers the picture otherwise.
        let hidden = others(s);
        let bar = overlays(s) || !receiving;
        let overlay = bar.then(|| {
            col()
                .key("solo-overlay")
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .justify_end()
                .gap(8.0)
                .padding(STAGE_PAD)
                .children((hidden > 0).then(|| show_others(t, hidden).self_center()))
                .child(
                    holds_overlays(controls(s, t, watch, mode, width))
                        .key("solo-bar")
                        .radius(radius::CARD)
                        .fill(t.surface.alpha(0.86))
                        .border(1.0, t.hairline)
                        .backdrop_blur(12.0),
                )
                .transition(Transition::opacity())
        });
        return col()
            .key(format!("tile-{id}"))
            .grow(1.0)
            .min_w(0.0)
            .min_h(0.0)
            .fill(Color::BLACK)
            .transition(Transition::opacity())
            .motion(motion::SETTLE)
            .child(video)
            .children(overlay);
    }
    let edge = if receiving {
        t.hairline
    } else {
        t.fair.alpha(0.4)
    };
    let mut frame = tile_frame(t, &id, edge, video);
    if drag.is_some_and(|drag| drag.id == id) {
        frame = frame.opacity(0.45);
    }
    if mode == Mode::Thumbnail {
        let dot = if receiving {
            lit_dot(t.live)
        } else {
            pulse(t.fair)
        };
        frame.child(thumbnail_label(s, t, watch.stream.publisher, dot).child(mute_button(
            s,
            t,
            watch,
            Variant::Ghost,
        )))
    } else {
        frame.child(controls(s, t, watch, mode, width))
    }
}

/// The box around a stream's picture. A preview and its watched tile share the key, so pressing
/// Watch keeps the box and swaps what is in it.
fn tile_frame(t: &Theme, id: &str, edge: Color, video: El) -> El {
    col()
        .key(format!("tile-{id}"))
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, edge)
        .clip_rounded(radius::CARD)
        .transition(Transition::opacity().combined(Transition::scale(0.98)))
        .motion(motion::SETTLE)
        .child(video)
}

fn thumbnail_label(s: &App, t: &Theme, publisher: u16, dot: El) -> El {
    row()
        .w_full()
        .pl(8.0)
        .pr(2.0)
        .py(2.0)
        .min_h(32.0)
        .gap(6.0)
        .center_y()
        .child(dot)
        .child(
            text(publisher_name(s, publisher))
                .font(ty::SMALL)
                .color(t.text)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        )
}

/// A running stream you haven't joined: the box it will play in, with Watch in the centre.
fn preview(s: &App, t: &Theme, stream: &streaming::StreamInfo, supported: bool, mode: Mode) -> El {
    let blocked = watch_blocked(s, supported, stream);
    let thumbnail = mode == Mode::Thumbnail;
    let video = el()
        .key("video")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .fill(Color::BLACK)
        .center()
        .child(
            button(
                t,
                &format!("watch-{}", stream.id),
                // A thumbnail is too narrow for the reason, which stays in the tooltip.
                if thumbnail {
                    "Watch"
                } else {
                    blocked.unwrap_or("Watch")
                },
                blocked.is_none().then_some(&icons::MONITOR_PLAY),
                if blocked.is_none() {
                    Variant::Primary
                } else {
                    Variant::Secondary
                },
                blocked
                    .is_none()
                    .then(|| Message::WatchStream(stream.id.clone())),
            )
            .help(blocked.unwrap_or("Join this stream")),
        );
    let frame = tile_frame(t, &stream.id, t.hairline, video);
    if thumbnail {
        return frame.child(thumbnail_label(s, t, stream.publisher, lit_dot(t.live)));
    }
    let viewers = match stream.viewers {
        1 => "1 viewer".to_owned(),
        count => format!("{count} viewers"),
    };
    let audio = if stream.has_audio {
        "Includes audio"
    } else {
        "No audio"
    };
    frame.child(
        row()
            .key("controls")
            .w_full()
            .px(10.0)
            .py(8.0)
            .gap(8.0)
            .center_y()
            .child(lit_dot(t.live))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(
                        text(stream.name.clone())
                            .font(ty::BODY_STRONG)
                            .color(t.text)
                            .truncate_fade(),
                    )
                    .child(
                        text(format!(
                            "{} · {viewers} · {audio}",
                            publisher_name(s, stream.publisher)
                        ))
                        .font(ty::CAPTION)
                        .color(t.faint)
                        .truncate_fade(),
                    ),
            ),
    )
}

/// A stream's volume slider: up to 300%, remembered for each streamer.
const STREAM_VOLUME: Range = Range {
    min: 0,
    max: streaming::MAX_VOLUME as i32,
    step: 1,
    key_step: 5,
};

/// Under a stream: who and what, its volume, focus and stop.
/// A stream's two lines: what it shows over who shares it, or the publisher first when the
/// stream's own name only says it is a screen share.
fn stream_lines(s: &App, stream: &streaming::StreamInfo) -> (String, String) {
    let publisher = publisher_name(s, stream.publisher);
    if publishing::generic_name(&stream.name) {
        (publisher, stream.name.clone())
    } else {
        (stream.name.clone(), publisher)
    }
}

/// Mutes a stream's sound, or brings it back; a stream without sound gets nothing.
fn mute_button(s: &App, t: &Theme, watch: &Watching, variant: Variant) -> El {
    let publisher = watch.stream.publisher;
    if !(watch.stream.has_audio || watch.stats.audio) {
        return el();
    }
    let muted = s.server().streaming.volume(publisher) == 0;
    icon_button(
        t,
        "stream-mute",
        if muted {
            &icons::VOLUME_OFF
        } else {
            &icons::VOLUME_2
        },
        if muted {
            "Unmute stream"
        } else {
            "Mute stream"
        },
        variant,
        Some(Message::ToggleStreamMute(publisher)),
    )
}

/// Roughly how wide a line of text is set, in points, for deciding what fits beside it.
fn text_width(line: &str, points: f32) -> f32 {
    line.chars().count() as f32 * points * 0.56
}

fn controls(s: &App, t: &Theme, watch: &Watching, mode: Mode, width: f32) -> El {
    let id = watch.stream.id.clone();
    let publisher = watch.stream.publisher;
    let volume = s.server().streaming.volume(publisher);
    let receiving = watch.phase == streaming::Phase::Receiving;
    let (title, subtitle) = stream_lines(s, &watch.stream);
    let audio = watch.stream.has_audio || watch.stats.audio;
    let large = matches!(mode, Mode::Stage | Mode::Solo);
    let hide_others = mode == Mode::Stage && others(s) > 0;
    let unfocus = mode == Mode::Stage && s.server().streaming.watching.len() > 1;
    // The name matters more than the slider: the slider shows only where the name still fits
    // beside it, or where a large stream leaves the name ample room anyway.
    let buttons = [audio, hide_others, unfocus, large, true]
        .into_iter()
        .filter(|on| *on)
        .count() as f32;
    let slider_width = if large { 120.0 } else { 64.0 };
    let name_room = width - 36.0 - buttons * (size::ICON_BUTTON + 8.0) - slider_width - 8.0;
    let name_width = text_width(&title, 13.0).max(text_width(&subtitle, 11.0));
    let volume_slider = audio && name_room >= name_width.min(if large { 220.0 } else { 160.0 });
    let mut bar = row()
        .key("controls")
        .w_full()
        .px(10.0)
        .py(8.0)
        .gap(8.0)
        .center_y()
        .child(if receiving {
            lit_dot(t.live)
        } else {
            pulse(t.fair)
        })
        .child(
            col()
                .key("stream-name")
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .help(format!(
                    "{} · {}",
                    watch.stream.name,
                    publisher_name(s, publisher)
                ))
                .child(
                    text(title)
                        .font(ty::BODY_STRONG)
                        .color(t.text)
                        .truncate_fade(),
                )
                .child(
                    text(if receiving {
                        subtitle
                    } else {
                        format!("{subtitle} · {}", watch.phase.label())
                    })
                    .font(ty::CAPTION)
                    .color(t.faint)
                    .truncate_fade(),
                ),
        );
    // A share can turn its audio on after it was announced without; audio arriving counts.
    if audio {
        bar = bar.child(mute_button(s, t, watch, Variant::Ghost));
        // In a narrow box the name matters more; the large stream brings the slider back.
        if volume_slider {
            bar = bar.child(
                el().w(slider_width).child(
                    slider(
                        t,
                        "stream-volume",
                        "Stream volume",
                        i32::from(volume),
                        format!("{volume}%"),
                        STREAM_VOLUME,
                        move |s: &App| i32::from(s.server().streaming.volume(publisher)),
                        move |value| {
                            Message::StreamVolume(
                                publisher,
                                value.clamp(0, STREAM_VOLUME.max) as u16,
                            )
                        },
                    )
                    .help(format!(
                        "Stream volume: {volume}%. Above 100% it is amplified."
                    )),
                ),
            );
        }
    }
    if hide_others {
        let beside = s.server().streaming.strip == Strip::Beside;
        bar = bar.child(icon_button(
            t,
            "hide-others",
            if beside {
                &icons::PANEL_RIGHT_CLOSE
            } else {
                &icons::PANEL_BOTTOM_CLOSE
            },
            "Hide the other streams so this one fills the pane",
            Variant::Ghost,
            Some(Message::HideOtherStreams(true)),
        ));
    }
    if unfocus {
        bar = bar.child(icon_button(
            t,
            "unfocus",
            &icons::LAYOUT_GRID,
            "Show every stream side by side",
            Variant::Ghost,
            Some(Message::FocusStream(None)),
        ));
    }
    if large {
        bar = bar.child(fullscreen_button(s, t));
    }
    bar.child(icon_button(
        t,
        "stop-watching",
        &icons::X,
        "Stop watching",
        Variant::Ghost,
        Some(Message::StopWatching(Some(id))),
    ))
}

/// How far a stream's trouble sign steps down while the toolbar floats over the top right.
const TOOLS_CLEARANCE: f32 = 72.0;

/// A stream's trouble over the top right of its picture: what is wrong and whose side it is
/// most likely on, in a few words, with the reasoning in the tooltip. A small tile shows the
/// sign alone.
fn trouble(t: &Theme, problem: health::Problem, compact: bool) -> El {
    use health::Trouble;
    let glyph = match (problem.trouble, problem.reconnecting) {
        (Trouble::YourComputer, _) => &icons::CPU,
        (Trouble::StreamerPaused, _) => &icons::MONITOR_PAUSE,
        (Trouble::Yours, false) => &icons::WIFI,
        (Trouble::Yours, true) => &icons::WIFI_OFF,
        (Trouble::Streamers, _) => &icons::SCREEN_SHARE,
        (Trouble::Between, false) => &icons::TRIANGLE_ALERT,
        (Trouble::Between, true) => &icons::ROTATE_CW,
    };
    let colour = if problem.serious() { t.poor } else { t.fair };
    let help = format!(
        "{}
{}",
        problem.label(),
        problem.explanation()
    );
    let mut badge = row()
        .key("trouble")
        .gap(6.0)
        .center_y()
        .px(if compact { 4.0 } else { 9.0 })
        .py(if compact { 4.0 } else { 5.0 })
        .radius(radius::ROW)
        .fill(Color::BLACK.alpha(0.62))
        .help(help.clone())
        .label(help)
        .transition(Transition::opacity())
        .child(icon(glyph, 14.0).color(colour));
    if !compact {
        badge = badge.child(
            text(problem.label())
                .font(ty::CAPTION)
                .color(Color::WHITE.alpha(0.9)),
        );
    }
    badge
}

/// Codec, size, frame rate and transport, over the corner of the picture.
fn stats(s: &App, watch: &Watching) -> El {
    let stats = &watch.stats;
    let line = |value: String| {
        text(value)
            .font(ty::CAPTION)
            .color(Color::WHITE.alpha(0.85))
    };
    let mut panel = col()
        .key("stats")
        .gap(2.0)
        .px(10.0)
        .py(8.0)
        .radius(radius::ROW)
        .fill(Color::BLACK.alpha(0.62))
        .a11y_hidden()
        .child(
            text(format!(
                "{} · {}×{} · {:.1} fps",
                if stats.codec.is_empty() {
                    "Negotiating codec"
                } else {
                    &stats.codec
                },
                stats.width,
                stats.height,
                stats.fps
            ))
            .font(ty::NUMERIC)
            .color(Color::WHITE),
        )
        .child(line(format!(
            "{} packets · {} frames · {} shown · {}",
            stats.packets,
            stats.decoded_frames,
            s.server().streaming.surfaces.get(watch.slot).presented(),
            usage::format_bytes(stats.bytes as f64)
        )))
        .child(line(format!(
            "Decoder: {} · ICE: {}",
            if stats.decoder.is_empty() {
                "—"
            } else {
                &stats.decoder
            },
            if stats.ice_path.is_empty() {
                "—"
            } else {
                &stats.ice_path
            }
        )));
    if !stats.codec_parameters.is_empty() {
        panel = panel.child(line(stats.codec_parameters.clone()));
    }
    if !stats.publisher_version.is_empty() {
        panel = panel.child(line(format!("Publisher: {}", stats.publisher_version)));
    }
    if !watch.detail.is_empty() {
        panel = panel.child(line(watch.detail.clone()));
    }
    let _ = s;
    panel
}

/// What to share, laid out rather than behind a menu: Screen, Window or Camera, then the
/// screens as small pictures to click, or the windows or cameras as rows beside a picture of the
/// chosen one. The main screen starts chosen, so Go Live works at once.
fn source_picker(s: &App, t: &Theme, k: &Kit) -> El {
    let sharing = &s.server().streaming.sharing;
    let groups: Vec<SourceGroup> = SourceGroup::ALL
        .into_iter()
        .filter(|group| {
            sharing
                .sources
                .iter()
                .any(|source| SourceGroup::of(&source.kind) == *group)
        })
        .collect();
    let current = sharing
        .selected
        .as_ref()
        .map(|source| SourceGroup::of(&source.kind))
        .or(s.ui.share_group)
        .filter(|group| groups.contains(group))
        .or(groups.first().copied());
    let mut header = row().w_full().gap(10.0).center_y();
    header = if groups.is_empty() {
        header.child(muted(
            t,
            if sharing.loading {
                "Looking for screens, windows and cameras…"
            } else {
                "No screen, window or camera found."
            },
        ))
    } else {
        header.child(choice(
            t,
            "share-group",
            "What to share",
            groups
                .iter()
                .map(|group| {
                    (
                        group_icon(*group),
                        group.label(),
                        Message::ShareGroup(*group),
                    )
                })
                .collect(),
            current.and_then(|current| groups.iter().position(|group| *group == current)),
        ))
    };
    header = header.child(el().grow(1.0)).child(icon_button(
        t,
        "find-sources",
        &icons::REFRESH,
        "Refresh screens, windows and cameras",
        Variant::Ghost,
        (!sharing.loading).then_some(Message::RefreshShareSources),
    ));
    let picker = col().key("share-source").w_full().gap(12.0).child(header);
    match current {
        Some(SourceGroup::Screen) => picker.child(screen_tiles(s, t)),
        Some(group) => picker.child(source_rows(s, t, k, group)),
        None => picker,
    }
}

fn group_icon(group: SourceGroup) -> &'static rune::Icon {
    match group {
        SourceGroup::Screen => &icons::MONITOR,
        SourceGroup::Window => &icons::APP_WINDOW,
        SourceGroup::Camera => &icons::VIDEO,
    }
}

/// Every screen as a small picture with its name below; a click picks it.
fn screen_tiles(s: &App, t: &Theme) -> El {
    let sharing = &s.server().streaming.sharing;
    let mut tiles = row()
        .key("share-screens")
        .w_full()
        .gap(SHARE_TILE_GAP)
        .flex_wrap();
    let mut screens: Vec<&ShareSource> = sharing
        .sources
        .iter()
        .filter(|source| SourceGroup::of(&source.kind) == SourceGroup::Screen)
        .collect();
    screens.sort_by_key(|source| !s.ui.quick_sources.iter().any(|q| q.kind == source.kind));
    for (index, source) in screens.into_iter().enumerate() {
        let on = sharing.selected.as_ref() == Some(source);
        let mut tile = col()
            .key(format!("share-screen-{index}"))
            .role("radio")
            .label(source.name.clone())
            .w(SHARE_TILE_WIDTH)
            .gap(6.0)
            .child(source_frame(s, t, Some(source), on))
            .child(
                text(source.name.clone())
                    .font(ty::CAPTION)
                    .color(if on { t.text } else { t.muted })
                    .truncate_fade()
                    .a11y_hidden(),
            );
        if !on {
            tile = tile
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(super::send(Message::ShareSource(source.clone())));
        }
        tiles = tiles.child(tile);
    }
    tiles
}

/// A source's picture at tile size: the still once taken, outlined in the accent while chosen.
fn source_frame(s: &App, t: &Theme, source: Option<&ShareSource>, on: bool) -> El {
    let sharing = &s.server().streaming.sharing;
    let caption = |words: &'static str| {
        text(words)
            .font(ty::CAPTION)
            .color(Color::WHITE.alpha(0.6))
            .align(rune::Align::Center)
            .w_full()
            .wrap()
    };
    let content = match source {
        None => caption("Pick one on the right"),
        // A Mac asks which screen or window once sharing starts, with its own previews.
        Some(source) if source.kind == SourceKind::Screen(None) => col()
            .w_full()
            .gap(6.0)
            .center()
            .px(10.0)
            .child(icon(&icons::MONITOR_UP, 18.0).color(Color::WHITE.alpha(0.6)))
            .child(caption("Your Mac asks which one when you go live")),
        Some(source) => match sharing.preview(source) {
            Some(PreviewState::Ready(pixels)) => {
                el().w_full().h_full().pixels_fit(pixels, SurfaceFit::Fit)
            }
            Some(PreviewState::Unavailable) => caption("No preview"),
            _ => pulse(t.accent),
        },
    };
    let picture = el()
        .w_full()
        .h_full()
        .radius(radius::ROW)
        .clip_rounded(radius::ROW)
        .fill(Color::BLACK)
        .border(1.0, t.hairline)
        .center()
        .child(content);
    let ring = el()
        .w(SHARE_TILE_WIDTH)
        .h(SHARE_TILE_HEIGHT)
        .padding(3.0)
        .radius(radius::ROW + 3.0);
    if on {
        ring.border(2.0, t.accent).child(picture)
    } else {
        ring.child(picture)
    }
}

/// Windows or cameras as rows, the game being played first, beside a picture of the chosen one.
fn source_rows(s: &App, t: &Theme, k: &Kit, group: SourceGroup) -> El {
    let sharing = &s.server().streaming.sharing;
    let game = |source: &ShareSource| {
        group == SourceGroup::Window && s.ui.quick_sources.iter().any(|q| q.kind == source.kind)
    };
    let mut sources: Vec<&ShareSource> = sharing
        .sources
        .iter()
        .filter(|source| SourceGroup::of(&source.kind) == group)
        .collect();
    sources.sort_by_key(|source| !game(source));
    let mut rows = col().key("share-rows").w_full().gap(2.0);
    for (index, source) in sources.iter().enumerate() {
        let on = sharing.selected.as_ref() == Some(*source);
        let glyph = if game(source) {
            &icons::GAMEPAD
        } else {
            group_icon(group)
        };
        let mut line = row()
            .key(format!("share-row-{index}"))
            .role("radio")
            .label(source.name.clone())
            .w_full()
            .h(SHARE_ROW)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::ROW)
            .child(
                icon(glyph, 14.0)
                    .color(if on { t.accent } else { t.muted })
                    .a11y_hidden(),
            )
            .child(
                text(source.name.clone())
                    .font(ty::BODY)
                    .color(if on { t.text } else { t.muted })
                    .truncate_fade()
                    .grow(1.0)
                    .min_w(0.0)
                    .a11y_hidden(),
            );
        line = if on {
            line.fill(t.wash(0.08))
        } else {
            line.focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(t.wash(0.05))
                .press(t.wash(0.09))
                .on_press_tx(super::send(Message::ShareSource((*source).clone())))
        };
        rows = rows.child(line);
    }
    let chosen = sharing
        .selected
        .as_ref()
        .filter(|source| SourceGroup::of(&source.kind) == group);
    row()
        .key("share-pick")
        .w_full()
        .gap(SHARE_TILE_GAP)
        .items_start()
        .child(source_frame(s, t, chosen, chosen.is_some()))
        .child(
            el().grow(1.0).min_w(0.0).child(
                k.scroll_bar(
                    col()
                        .key("share-rows-scroll")
                        .w_full()
                        .max_h(SHARE_ROW * 4.5 + 8.0)
                        .scroll(rune::ScrollSpec {
                            stick_bottom: false,
                        })
                        .scroll_edge_fade(8.0)
                        .child(rows),
                ),
            ),
        )
}

/// Choosing what to share, its quality and audio; or, while sharing, who watches.
pub fn share_dialog(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if !s.server().streaming.share_open {
        return;
    }
    let sharing = &s.server().streaming.sharing;
    let status = &sharing.status;
    let mut menus = Vec::new();
    let height = cx
        .layout_size("window")
        .map_or(s.window_size.1, |size| size[1]);
    let actions;
    let mut body = col().key("share-body").w_full().px(16.0).pb(16.0).gap(14.0);
    if status.active {
        // One line on what runs, then who watches, then the settings that change live.
        let mut summary = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(lit_dot(t.live))
            .child(
                text(format!(
                    "{} / {} watching",
                    status.viewers.len(),
                    streaming::publishing::MAX_VIEWERS
                ))
                .font(ty::BODY_STRONG)
                .color(t.text),
            );
        if !status.detail.is_empty() {
            summary = summary.child(
                faint(t, format!("· {}", status.detail))
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate_fade(),
            );
        }
        body = body.child(summary);
        if sharing.audio {
            body = body.child(meter(
                t,
                "share-audio-level",
                status.audio_level.min(1.0),
                t.live,
            ));
        }
        for (id, name, ready) in &status.viewers {
            body = body.child(
                row()
                    .key(format!("viewer-{id}"))
                    .gap(8.0)
                    .center_y()
                    .child(el().size(6.0, 6.0).radius(3.0).fill(if *ready {
                        t.good
                    } else {
                        t.fair
                    }))
                    .child(
                        text(format!(
                            "{name} · {}",
                            if *ready { "Connected" } else { "Connecting…" }
                        ))
                        .font(ty::BODY)
                        .color(t.text),
                    ),
            );
        }
        for (id, name) in &status.requests {
            body = body.child(request_inline(t, status, *id, name));
        }
        body = body.child(divider(t));
        // Quality and sound change while live; what is shared and who may watch stay.
        body = audio_field(s, k, quality_fields(s, t, k, body, false, &mut menus));
        actions = row()
            .w_full()
            .gap(size::GAP)
            .child(el().grow(1.0))
            .child(button(
                t,
                "close-share",
                "Close",
                None,
                Variant::Secondary,
                Some(Message::CloseShare),
            ))
            .child(button(
                t,
                "stop-sharing-dialog",
                "Stop Sharing",
                Some(&icons::STOP),
                Variant::Danger,
                Some(Message::StopSharing),
            ));
    } else {
        body = body.child(source_picker(s, t, k));
        body = audio_field(s, k, quality_fields(s, t, k, body, true, &mut menus));
        if !status.detail.is_empty() {
            body = body.child(muted(t, status.detail.clone()));
        }
        actions = row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .child(el().grow(1.0))
            .child(button(
                t,
                "cancel-share",
                "Cancel",
                None,
                Variant::Secondary,
                Some(Message::CloseShare),
            ))
            .child(
                button(
                    t,
                    "start-sharing",
                    "Go Live",
                    Some(&icons::MONITOR_UP),
                    Variant::Primary,
                    (!sharing.loading && sharing.selected.is_some())
                        .then_some(Message::StartSharing),
                )
                .default_button(),
            );
    }
    Dialog::new(
        "share",
        if status.active {
            "You are sharing"
        } else {
            "Share screen or camera"
        },
    )
    .open(true)
    .fit()
    .width(560.0)
    .on_close(|s: &mut App| {
        let _ = s.update(Message::CloseShare);
        s.sync_fields();
    })
    .child(
        k.scroll_bar(
            col()
                .key("share-scroll")
                .w_full()
                .min_h(0.0)
                // The centred dialog stays below Windows' in-app window buttons.
                .max_h((height - 180.0 - if cfg!(windows) { 72.0 } else { 0.0 }).max(220.0))
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .scroll_edge_fade(12.0)
                .child(body),
        ),
    )
    .child(el().w_full().padding(16.0).child(actions))
    .build(k, layer);
    layer.extend(menus);
}

/// Performance or Quality, and while choosing a share also who may watch at the far end,
/// with one line saying what it sends and a toggle for every value one by one. Editing a value
/// there makes the line read "Custom" until a preset is picked again.
fn quality_fields(s: &App, t: &Theme, k: &Kit, body: El, privacy: bool, menus: &mut Vec<El>) -> El {
    let sharing = &s.server().streaming.sharing;
    let quality = sharing.quality;
    let preset = quality.preset();
    let advanced = s.ui.share_advanced;
    let mut choices = row().w_full().gap(16.0).items_end().child(
        col()
            .gap(10.0)
            .child(field_label(t, "Preset"))
            .child(choice(
                t,
                "share-preset",
                "Preset",
                Preset::ALL
                    .iter()
                    .map(|p| {
                        let glyph = match p {
                            Preset::Performance => &icons::GAUGE,
                            Preset::Quality => &icons::SPARKLES,
                        };
                        (glyph, p.label(), Message::ShareQuality(p.quality()))
                    })
                    .collect(),
                Preset::ALL.iter().position(|p| Some(*p) == preset),
            )),
    );
    if privacy {
        // Set once and left alone, so it sits at the far end.
        choices = choices.child(el().grow(1.0)).child(
            col()
                .gap(10.0)
                .child(field_label(t, "Privacy"))
                .child(choice(
                    t,
                    "share-privacy",
                    "Privacy",
                    Access::ALL
                        .iter()
                        .map(|access| {
                            let glyph = match access {
                                Access::Public => &icons::GLOBE,
                                Access::Private => &icons::LOCK,
                            };
                            (glyph, access.label(), Message::ShareAccess(*access))
                        })
                        .collect(),
                    Access::ALL.iter().position(|a| *a == sharing.access),
                )),
        );
    }
    let mut line = format!(
        "{} · {} fps · up to {:.1} Mbit/s per viewer",
        quality.resolution.label(),
        quality.frame_rate,
        quality.upload_mbps()
    );
    if preset.is_none() {
        line = format!("Custom · {line}");
    }
    let body = body.child(choices).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .mt(-6.0)
            .child(faint(t, line).grow(1.0).min_w(0.0).truncate_fade())
            .child(button(
                t,
                "share-advanced",
                "Advanced",
                Some(if advanced {
                    &icons::CHEVRON_UP
                } else {
                    &icons::CHEVRON_DOWN
                }),
                Variant::Ghost,
                Some(Message::ShareAdvanced(!advanced)),
            )),
    );
    if advanced {
        advanced_fields(s, t, k, body, menus)
    } else {
        body
    }
}

/// Resolution, frame rate, bitrates, what gives way, and the audio output, one by one.
fn advanced_fields(s: &App, t: &Theme, k: &Kit, body: El, menus: &mut Vec<El>) -> El {
    let sharing = &s.server().streaming.sharing;
    let quality = sharing.quality;
    let mut body = body
        .child(
            row()
                .w_full()
                .gap(16.0)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(10.0)
                        .child(field_label(t, "Resolution"))
                        .child(resolution_picker(s, k, menus)),
                )
                .child(col().grow(1.0).min_w(0.0).child(quality_slider(
                    t,
                    "share-fps",
                    "Frame rate",
                    quality.frame_rate as i32,
                    format!("{} fps", quality.frame_rate),
                    Range {
                        min: 5,
                        max: 60,
                        step: 1,
                        key_step: 1,
                    },
                    |s: &App| s.server().streaming.sharing.quality.frame_rate as i32,
                    |value| Message::ShareFrameRate(value as u32),
                ))),
        )
        .child(quality_slider(
            t,
            "share-video-bitrate",
            "Video bitrate per viewer",
            quality.video_kbps as i32,
            format!("{} kbps", quality.video_kbps),
            Range {
                min: 250,
                max: 20_000,
                step: 50,
                key_step: 50,
            },
            |s: &App| s.server().streaming.sharing.quality.video_kbps as i32,
            |value| Message::ShareVideoBitrate(value as u32),
        ))
        .child(field_label(t, "When the connection struggles"))
        .child(
            SegmentedControl::new(
                &Struggle::ALL.map(Struggle::label),
                Struggle::ALL.iter().position(|s| *s == quality.struggle),
            )
            .label("When the connection struggles")
            .on_pick(|s: &mut App, index, tx| {
                dispatch(s, tx, Message::ShareStruggle(Struggle::ALL[index]));
            })
            .build(k),
        );
    if sharing.audio {
        let outputs: Vec<String> = s.devices.outputs.iter().map(|d| d.name.clone()).collect();
        let current = sharing
            .audio_device
            .as_ref()
            .or(s.devices.output.as_ref())
            .map(|device| device.name.clone());
        body = body
            .child(field_label(t, "System audio from"))
            .child(picker(
                k,
                s,
                "share-output",
                outputs,
                current.as_deref(),
                "System audio output",
                true,
                SHARE_PICKER,
                {
                    let outputs = s.devices.outputs.clone();
                    move |index| Message::ShareAudioOutput(outputs[index].clone())
                },
                menus,
            ))
            .child(quality_slider(
                t,
                "share-audio-bitrate",
                "Audio bitrate per viewer",
                quality.audio_kbps as i32,
                format!("{} kbps", quality.audio_kbps),
                Range {
                    min: 32,
                    max: 320,
                    step: 16,
                    key_step: 16,
                },
                |s: &App| s.server().streaming.sharing.quality.audio_kbps as i32,
                |value| Message::ShareAudioBitrate(value as u32),
            ));
    }
    body
}

/// Whether system audio is shared; Flexspeak's own voices and sounds are left out of it.
fn audio_field(s: &App, k: &Kit, body: El) -> El {
    let sharing = &s.server().streaming.sharing;
    body.child(
        Checkbox::new(sharing.audio)
            .label("Share system audio")
            .on_toggle_tx(move |s: &mut App, tx| {
                let next = !s.server().streaming.sharing.audio;
                dispatch(s, tx, Message::ShareAudio(next));
            })
            .build(k),
    )
}

fn resolution_picker(s: &App, k: &Kit, menus: &mut Vec<El>) -> El {
    picker(
        k,
        s,
        "share-resolution",
        Resolution::ALL
            .iter()
            .map(|resolution| resolution.label().to_owned())
            .collect(),
        Some(s.server().streaming.sharing.quality.resolution.label()),
        "Resolution",
        true,
        224.0,
        |index| Message::ShareResolution(Resolution::ALL[index]),
        menus,
    )
}

/// The share dialog's pickers span its width.
const SHARE_PICKER: f32 = 480.0;
/// Three source pictures fill the dialog's width, 16:9 like most screens.
const SHARE_TILE_GAP: f32 = 12.0;
const SHARE_TILE_WIDTH: f32 = (528.0 - 2.0 * SHARE_TILE_GAP) / 3.0;
const SHARE_TILE_HEIGHT: f32 = SHARE_TILE_WIDTH * 9.0 / 16.0;
const SHARE_ROW: f32 = 28.0;

fn field_label(t: &Theme, label: &str) -> El {
    text(label.to_owned())
        .font(ty::LABEL)
        .color(t.muted)
        .mb(-6.0)
}

#[allow(clippy::too_many_arguments)]
fn quality_slider(
    t: &Theme,
    key: &str,
    label: &str,
    value: i32,
    spoken: String,
    range: Range,
    current: fn(&App) -> i32,
    on_change: fn(i32) -> Message,
) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .center_y()
                .child(text(label.to_owned()).font(ty::LABEL).color(t.muted))
                .child(el().grow(1.0))
                .child(text(spoken.clone()).font(ty::NUMERIC).color(t.text)),
        )
        .child(slider(
            t, key, label, value, spoken, range, current, on_change,
        ))
}
