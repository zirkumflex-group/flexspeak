//! The connected right pane: streams in your channel, watched on a stage that fills the pane.
//!
//! One watched stream fills the stage. With several, they share it in a grid until you focus
//! one; the focused stream takes the stage and the others line up below or beside it. Dragging
//! one stream onto another trades their places. Each stream has its own volume. Codec and
//! transport details hide behind the info button.
//! Sharing your own screen starts from the empty pane's own buttons or the one beside
//! Disconnect, in [`share_dialog`].

use rune::{Color, Cx, Drag, El, Phase, SurfaceFit, Transition, Tx, col, el, icon, row, text};
use rune_kit::{Checkbox, Dialog, Kit};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Range, Variant, button, faint, icon_button, lit_dot, meter, muted, picker, pulse, scroll_page,
    slider,
};
use super::{TileDrag, dispatch, icons, theme};
use crate::streaming::{
    self, Strip, Watching,
    publishing::{Access, Preset, PreviewState, Resolution, ShareSource, SourceKind},
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
                .on_hover_change(|s: &mut App, hovering, _| s.ui.streams_hover = hovering)
                .child(stage(s, t, k, supported, room))
                .children(
                    s.ui.streams_hover
                        .then(|| stage_tools(s, t, supported, watching)),
                ),
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
            tools(s, t, supported, watching)
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
        state = state.child(
            text(share_summary(s))
                .font(ty::SMALL)
                .color(t.faint)
                .align(rune::Align::Center),
        );
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

/// Your own share in a line: how many watch it, and what it shows.
fn share_summary(s: &App) -> String {
    let sharing = &s.server().streaming.sharing;
    let source = if sharing.status.detail.is_empty() {
        sharing.quality.label()
    } else {
        let detail = sharing.status.detail.strip_prefix("Sharing ");
        detail.unwrap_or(&sharing.status.detail).to_owned()
    };
    format!(
        "{} / {} viewers · {source}",
        sharing.status.viewers.len(),
        streaming::publishing::MAX_VIEWERS
    )
}

/// A note on the main pane, recessed into it.
fn notice(t: &Theme) -> El {
    super::widgets::well(t).padding(size::CARD_PAD)
}

/// Your own share while it runs, for the voice panel: viewers, requests to watch, and the way to
/// stop. The source comes last so a narrow sidebar only loses the end of it.
pub fn sharing_status(s: &App, t: &Theme) -> El {
    let status = &s.server().streaming.sharing.status;
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
                .child(icon_button(
                    t,
                    "manage-share",
                    &icons::INFO,
                    "Sharing details",
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
        .child(faint(t, share_summary(s)).truncate_fade());
    if status.audio_frames > 0 {
        tile = tile.child(meter(t, "share-audio", status.audio_level.min(1.0), t.live));
    }
    for (id, name) in &status.requests {
        tile = tile.child(request_row(t, status, *id, name));
    }
    tile
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
        return area.padding(0.0).child(tile(s, t, main, Mode::Solo, true));
    }
    let Some(main) = s.server().streaming.stage() else {
        let columns = grid_columns(watching.len() + available.len(), space);
        let volume_slider = cell_width(space, columns.unwrap_or(2)) >= MIN_CELL_WITH_SLIDER;
        let cells = watching
            .iter()
            .map(|watch| tile(s, t, watch, Mode::Grid, volume_slider))
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
        .map(|watch| tile(s, t, watch, Mode::Thumbnail, false))
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
    let main = tile(
        s,
        t,
        main,
        Mode::Stage,
        stage_width >= MIN_STAGE_WITH_SLIDER,
    );
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
const MIN_CELL_WITH_SLIDER: f32 = 260.0;
/// The stage's slider is wider, and it has a button more.
const MIN_STAGE_WITH_SLIDER: f32 = 360.0;

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

/// A click puts the stream on the stage, a double-click on the stage goes back to the grid, and
/// a drag onto another stream trades their places.
fn movable(video: El, stream: &streaming::StreamInfo, mode: Mode) -> El {
    let id = stream.id.clone();
    let on_stage = matches!(mode, Mode::Stage | Mode::Solo);
    video
        .role("button")
        .label(format!("Move or focus {}", stream.name))
        .help(if on_stage {
            "Drag onto another stream to swap · double-click to show every stream"
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
                            dispatch(s, tx, Message::FocusStream(Some(id.clone())))
                        }
                        None if d.clicks >= 2 => dispatch(s, tx, Message::FocusStream(None)),
                        None => {}
                    }
                }
            }
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

fn tile(s: &App, t: &Theme, watch: &Watching, mode: Mode, volume_slider: bool) -> El {
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
    }
    if s.server().streaming.show_stats && mode != Mode::Thumbnail {
        video = video.child(stats(s, watch).absolute(10.0, 10.0));
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
                    &icons::MAXIMIZE,
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
        let bar = s.ui.streams_hover || !receiving;
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
                    controls(s, t, watch, mode, volume_slider)
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
        frame.child(thumbnail_label(s, t, watch.stream.publisher, dot))
    } else {
        frame.child(controls(s, t, watch, mode, volume_slider))
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
        .px(8.0)
        .py(5.0)
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

/// Under a stream: who and what, its volume, focus and stop.
fn controls(s: &App, t: &Theme, watch: &Watching, mode: Mode, volume_slider: bool) -> El {
    let id = watch.stream.id.clone();
    let publisher = watch.stream.publisher;
    let volume = s.server().streaming.volume(publisher);
    let receiving = watch.phase == streaming::Phase::Receiving;
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
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(
                    text(watch.stream.name.clone())
                        .font(ty::BODY_STRONG)
                        .color(t.text)
                        .truncate_fade(),
                )
                .child(
                    text(if receiving {
                        publisher_name(s, publisher)
                    } else {
                        format!("{} · {}", publisher_name(s, publisher), watch.phase.label())
                    })
                    .font(ty::CAPTION)
                    .color(t.faint)
                    .truncate_fade(),
                ),
        );
    if watch.stream.has_audio {
        bar = bar.child(icon_button(
            t,
            "stream-mute",
            if volume == 0 {
                &icons::VOLUME_OFF
            } else {
                &icons::VOLUME_2
            },
            if volume == 0 {
                "Unmute stream"
            } else {
                "Mute stream"
            },
            Variant::Ghost,
            Some(Message::StreamVolume(
                publisher,
                if volume == 0 { 100 } else { 0 },
            )),
        ));
        // In a narrow box the name matters more; maximizing brings the slider back.
        if volume_slider {
            bar = bar.child(
                el().w(if matches!(mode, Mode::Stage | Mode::Solo) {
                    120.0
                } else {
                    64.0
                })
                .child(slider(
                    t,
                    "stream-volume",
                    "Stream volume",
                    i32::from(volume),
                    format!("{volume}%"),
                    Range::PERCENT,
                    move |s: &App| i32::from(s.server().streaming.volume(publisher)),
                    move |value| Message::StreamVolume(publisher, value.clamp(0, 100) as u8),
                )),
            );
        }
    }
    if mode == Mode::Stage && others(s) > 0 {
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
    if mode == Mode::Stage && s.server().streaming.watching.len() > 1 {
        bar = bar.child(icon_button(
            t,
            "unfocus",
            &icons::LAYOUT_GRID,
            "Show every stream side by side",
            Variant::Ghost,
            Some(Message::FocusStream(None)),
        ));
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
            watch.ui_updates,
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

/// The one picture of the chosen source, taken when the dialog opened or it was picked.
fn source_preview(s: &App, t: &Theme) -> Option<El> {
    let sharing = &s.server().streaming.sharing;
    let source = sharing.selected.as_ref()?;
    if source.kind == SourceKind::Screen(None) {
        return Some(faint(
            t,
            "Your Mac shows its own previews when you start sharing.",
        ));
    }
    let caption = |words: &'static str| text(words).font(ty::SMALL).color(Color::WHITE.alpha(0.7));
    let content = match sharing.preview(source) {
        Some(PreviewState::Ready(pixels)) => {
            el().w_full().h_full().pixels_fit(pixels, SurfaceFit::Fit)
        }
        Some(PreviewState::Unavailable) => caption("No preview available"),
        _ => col()
            .gap(8.0)
            .center()
            .child(pulse(t.accent))
            .child(caption("Taking a preview…")),
    };
    Some(
        el().key("share-preview")
            .w_full()
            .h(SHARE_PREVIEW_HEIGHT)
            .radius(radius::ROW)
            .clip_rounded(radius::ROW)
            .fill(Color::BLACK)
            .center()
            .child(content),
    )
}

/// Choosing what to share, its quality and audio; or, while sharing, who watches.
pub fn share_dialog(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if !s.server().streaming.share_open {
        return;
    }
    let sharing = &s.server().streaming.sharing;
    let quality = sharing.quality;
    let status = &sharing.status;
    let mut menus = Vec::new();
    let height = cx
        .layout_size("window")
        .map_or(s.window_size.1, |size| size[1]);
    let actions;
    let mut body = col().key("share-body").w_full().px(16.0).pb(16.0).gap(14.0);
    if status.active {
        body = body
            .child(muted(t, status.detail.clone()))
            .child(faint(
                t,
                format!(
                    "{} / {} viewers · {} video frames · {} audio blocks",
                    status.viewers.len(),
                    streaming::publishing::MAX_VIEWERS,
                    status.video_frames,
                    status.audio_frames
                ),
            ))
            .child(meter(
                t,
                "share-audio-level",
                status.audio_level.min(1.0),
                t.live,
            ));
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
            body = body.child(request_row(t, status, *id, name));
        }
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
        let names: Vec<String> = sharing
            .sources
            .iter()
            .map(|source| source.name.clone())
            .collect();
        body = body
            .child(field_label(t, "Screen, window or camera"))
            .child(
                row()
                    .w_full()
                    .gap(10.0)
                    .center_y()
                    .child(el().grow(1.0).min_w(0.0).child(picker(
                        k,
                        s,
                        "share-source",
                        names,
                        sharing.selected.as_ref().map(|source| source.name.as_str()),
                        if sharing.loading {
                            "Looking for sources…"
                        } else {
                            "Choose a screen, window or camera…"
                        },
                        true,
                        SHARE_PICKER,
                        {
                            let sources = sharing.sources.clone();
                            move |index| Message::ShareSource(sources[index].clone())
                        },
                        &mut menus,
                    )))
                    .child(icon_button(
                        t,
                        "find-sources",
                        &icons::REFRESH,
                        "Refresh screens, windows and cameras",
                        Variant::Secondary,
                        (!sharing.loading).then_some(Message::RefreshShareSources),
                    )),
            )
            .children(source_preview(s, t))
            .child(field_label(t, "Preset"))
            .child(preset_picker(s, k, &mut menus))
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
                            .child(resolution_picker(s, k, &mut menus)),
                    )
                    .child(col().grow(1.0).min_w(0.0).child(quality_slider(
                        t,
                        "share-fps",
                        "Frame rate",
                        quality.frame_rate as i32,
                        format!("{} fps", quality.frame_rate),
                        Range { min: 5, max: 60, step: 1, key_step: 1 },
                        |s: &App| s.server().streaming.sharing.quality.frame_rate as i32,
                        |value| Message::ShareFrameRate(value as u32),
                    ))),
            )
            .child(quality_slider(
                t,
                "share-video-bitrate",
                "Video bitrate",
                quality.video_kbps as i32,
                format!("{} kbps", quality.video_kbps),
                Range { min: 250, max: 20_000, step: 50, key_step: 50 },
                |s: &App| s.server().streaming.sharing.quality.video_kbps as i32,
                |value| Message::ShareVideoBitrate(value as u32),
            ))
            .child(faint(
                t,
                "Resolution, frame rate and bitrate are maximums. Actual quality depends on the source and connection. Bitrate is per viewer.",
            ))
            .child(field_label(t, "Who can watch"))
            .child(picker(
                k,
                s,
                "share-access",
                Access::ALL.iter().map(|access| access.label().to_owned()).collect(),
                Some(sharing.access.label()),
                "Access",
                true,
                SHARE_PICKER,
                |index| Message::ShareAccess(Access::ALL[index]),
                &mut menus,
            ))
            .child(
                Checkbox::new(sharing.audio)
                    .label("Share system audio")
                    .on_toggle_tx(move |s: &mut App, tx| {
                        let next = !s.server().streaming.sharing.audio;
                        dispatch(s, tx, Message::ShareAudio(next));
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
                    &mut menus,
                ))
                .child(quality_slider(
                    t,
                    "share-audio-bitrate",
                    "Audio bitrate",
                    quality.audio_kbps as i32,
                    format!("{} kbps", quality.audio_kbps),
                    Range { min: 32, max: 320, step: 16, key_step: 16 },
                    |s: &App| s.server().streaming.sharing.quality.audio_kbps as i32,
                    |value| Message::ShareAudioBitrate(value as u32),
                ))
                .child(faint(
                    t,
                    "Shares everything on this output, including notifications. Your microphone stays separate.",
                ));
        }
        // Experimental and Windows-only.
        if cfg!(windows) {
            body = body
                .child(
                    Checkbox::new(sharing.hardware_av1)
                        .label("Hardware AV1 encoding (experimental)")
                        .on_toggle_tx(move |s: &mut App, tx| {
                            let next = !s.server().streaming.sharing.hardware_av1;
                            dispatch(s, tx, Message::ShareHardwareAv1(next));
                        })
                        .build(k),
                )
                .child(faint(
                    t,
                    "Encodes on NVIDIA RTX 40 series and newer graphics cards instead of the CPU. Other cards, and viewers who can't play AV1, get VP8 as before.",
                ));
        }
        // Cameras have their own capture.
        if cfg!(windows)
            && !sharing
                .selected
                .as_ref()
                .is_some_and(|s| s.kind.is_camera())
        {
            body = body
                .child(
                    Checkbox::new(sharing.fast_capture)
                        .label("Fast capture (experimental)")
                        .on_toggle_tx(move |s: &mut App, tx| {
                            let next = !s.server().streaming.sharing.fast_capture;
                            dispatch(s, tx, Message::ShareFastCapture(next));
                        })
                        .build(k),
                )
                .child(faint(
                    t,
                    "Captures and resizes on the graphics card, so a busy game no longer stalls your stream or loads the CPU. Turn it off if the picture stays black or freezes.",
                ));
        }
        if !status.detail.is_empty() {
            body = body.child(muted(t, status.detail.clone()));
        }
        actions = row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .child(
                faint(
                    t,
                    if sharing.access == Access::Public {
                        "Anyone in your channel can join. Up to four viewers."
                    } else {
                        "You approve each viewer. Up to four can watch."
                    },
                )
                .grow(1.0)
                .min_w(0.0),
            )
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

/// A preset fills all four quality values; any change after that reads "Custom".
fn preset_picker(s: &App, k: &Kit, menus: &mut Vec<El>) -> El {
    let label = |preset: Preset| format!("{} · {}", preset.label(), preset.quality().label());
    let current = s
        .server()
        .streaming
        .sharing
        .quality
        .preset()
        .map_or_else(|| "Custom".to_owned(), label);
    picker(
        k,
        s,
        "share-quality",
        Preset::ALL.into_iter().map(label).collect(),
        Some(&current),
        "Preset",
        true,
        SHARE_PICKER,
        |index| Message::ShareQuality(Preset::ALL[index].quality()),
        menus,
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
const SHARE_PREVIEW_HEIGHT: f32 = 200.0;

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
