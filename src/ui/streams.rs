//! The connected right pane: streams in your channel, watched on a stage that fills the pane.
//!
//! One watched stream fills the stage. With several, they share it in a grid until you focus
//! one; the focused stream takes the stage and the others line up beneath it, as in Discord.
//! Each stream has its own volume. Codec and transport details hide behind the info button.
//! Sharing your own screen starts from the button beside Disconnect, in [`share_dialog`].

use rune::{Color, Cx, El, SurfaceFit, Transition, col, el, icon, row, text};
use rune_kit::{Checkbox, Dialog, Kit};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Range, Variant, button, card, faint, group, group_note, group_title, icon_button, lit_dot,
    meter, muted, picker, pulse, scroll_page, slider, title,
};
use super::{dispatch, icons, send};
use crate::streaming::{
    self, Watching,
    publishing::{Access, Preset, Resolution},
};
use crate::{App, Message, usage};

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let connected = s.connected && !s.loopback && !s.stopping;
    let supported = connected && s.state.server_version.starts_with("6.");
    share_dialog(s, cx, t, k, layer);
    let watching = s.streaming.active();
    let mut pane = col()
        .key("streams")
        .w_full()
        .h_full()
        .child(header(s, t, supported, watching));
    if s.streaming.sharing.status.active {
        pane = pane.child(sharing_banner(s, t));
    }
    if watching {
        pane = pane.child(stage(s, t, k, supported));
    } else {
        pane = pane.child(scroll_page(k, "streams-scroll", catalog(s, t, supported)));
    }
    pane
}

fn header(s: &App, t: &Theme, supported: bool, watching: bool) -> El {
    let mut bar = row()
        .key("streams-header")
        .w_full()
        .px(size::PAGE)
        .pt(16.0)
        .pb(10.0)
        .gap(10.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(title(t, "Streams"))
                .child(muted(
                    t,
                    format!(
                        "Server {} · {}",
                        s.state.server_version,
                        s.state
                            .current
                            .as_ref()
                            .map_or("No channel", |channel| channel.name.as_str())
                    ),
                )),
        );
    if watching {
        let on = s.streaming.show_stats;
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
        if s.streaming.watching.len() > 1 {
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

/// Streams in the channel that you aren't watching yet.
fn unwatched(s: &App) -> Vec<&streaming::StreamInfo> {
    s.streaming
        .catalog
        .iter()
        .filter(|stream| s.streaming.watching(&stream.id).is_none())
        .collect()
}

fn publisher_name(s: &App, id: u16) -> String {
    s.state
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
    } else if s.streaming.sharing.status.active {
        Some("Stop sharing to watch")
    } else if s.streaming.watching.len() >= streaming::MAX_WATCHES {
        Some("Watching the most at once")
    } else {
        None
    }
}

/// The list shown while nothing is watched.
fn catalog(s: &App, t: &Theme, supported: bool) -> El {
    let mut content = col()
        .w_full()
        .max_w(860.0)
        .px(size::PAGE)
        .pb(size::PAGE)
        .gap(10.0);
    if !supported {
        content = content.child(card(t).child(muted(
            t,
            "This server does not advertise TeamSpeak 6. Server-channel streaming is unavailable.",
        )));
    } else if !streaming::AVAILABLE {
        content = content.child(card(t).child(muted(
            t,
            "Native screen sharing is available on Mac and Windows.",
        )));
    }
    content = content.child(group_title(t, "In this channel"));
    let mut rows = Vec::new();
    let available = unwatched(s);
    if available.is_empty() {
        rows.push(
            col()
                .w_full()
                .padding(28.0)
                .gap(8.0)
                .center()
                .child(icon(&icons::SCREEN_SHARE, 22.0).color(t.faint))
                .child(muted(t, "No one in this channel is sharing."))
                .child(faint(
                    t,
                    "Share your own screen with the button beside Disconnect.",
                )),
        );
    }
    for stream in available {
        let blocked = watch_blocked(s, supported, stream);
        rows.push(
            row()
                .key(format!("stream-{}", stream.id))
                .w_full()
                .px(size::CARD_PAD)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .transition(Transition::opacity())
                .child(lit_dot(t.live))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(
                            text(stream.name.clone())
                                .font(ty::BODY_STRONG)
                                .color(t.text)
                                .truncate_fade(),
                        )
                        .child(faint(
                            t,
                            format!(
                                "{} · {} viewers{}",
                                publisher_name(s, stream.publisher),
                                stream.viewers,
                                if stream.has_audio {
                                    " · includes audio"
                                } else {
                                    ""
                                }
                            ),
                        )),
                )
                .child(button(
                    t,
                    "watch",
                    blocked.unwrap_or("Watch"),
                    blocked.is_none().then_some(&icons::MONITOR_PLAY),
                    Variant::Secondary,
                    blocked
                        .is_none()
                        .then(|| Message::WatchStream(stream.id.clone())),
                )),
        );
    }
    content = content.child(group(t, rows)).child(group_note(
        t,
        "Watch up to four streams at once. Leaving this view or switching channels stops watching and sharing.",
    ));
    content
}

/// Your own share while it runs: viewers, requests to watch, and the way to stop.
fn sharing_banner(s: &App, t: &Theme) -> El {
    let status = &s.streaming.sharing.status;
    let mut banner = col()
        .key("sharing-banner")
        .w_full()
        .gap(8.0)
        .px(size::PAGE)
        .pb(10.0)
        .transition(Transition::opacity());
    let mut line = row()
        .w_full()
        .gap(10.0)
        .padding(10.0)
        .center_y()
        .radius(radius::ROW)
        .fill(t.live.alpha(0.08))
        .border(1.0, t.live.alpha(0.35))
        .child(lit_dot(t.live))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text("You are sharing").font(ty::BODY_STRONG).color(t.text))
                .child(faint(
                    t,
                    format!(
                        "{} · {} / {} viewers",
                        if status.detail.is_empty() {
                            s.streaming.sharing.quality.label()
                        } else {
                            status.detail.clone()
                        },
                        status.viewers.len(),
                        streaming::publishing::MAX_VIEWERS
                    ),
                )),
        )
        .child(button(
            t,
            "manage-share",
            "Details",
            None,
            Variant::Secondary,
            Some(Message::OpenShare),
        ))
        .child(button(
            t,
            "stop-sharing",
            "Stop",
            Some(&icons::STOP),
            Variant::Danger,
            Some(Message::StopSharing),
        ));
    if status.audio_frames > 0 {
        line = line.child(el().w(60.0).child(meter(
            t,
            "share-audio",
            status.audio_level.min(1.0),
            t.live,
        )));
    }
    banner = banner.child(line);
    for (id, name) in &status.requests {
        banner = banner.child(request_row(t, status, *id, name));
    }
    banner
}

fn request_row(t: &Theme, status: &streaming::publishing::Status, id: u16, name: &str) -> El {
    row()
        .key(format!("request-{id}"))
        .w_full()
        .gap(8.0)
        .center_y()
        .padding(10.0)
        .radius(radius::ROW)
        .fill(t.accent.alpha(0.08))
        .transition(Transition::opacity().combined(Transition::scale(0.98)))
        .child(
            text(format!("{name} wants to watch"))
                .font(ty::BODY_STRONG)
                .color(t.text)
                .grow(1.0),
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

/// The watched streams: one on the stage with the rest beneath it, or a grid of equals.
fn stage(s: &App, t: &Theme, k: &Kit, supported: bool) -> El {
    let watching = &s.streaming.watching;
    let mut area = col()
        .key("stage")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .px(size::PAGE)
        .pb(size::PAGE)
        .gap(10.0);
    if let Some(main) = s.streaming.stage() {
        area = area.child(tile(s, t, main, Mode::Stage));
        let others: Vec<&Watching> = watching
            .iter()
            .filter(|watch| watch.generation != main.generation)
            .collect();
        if !others.is_empty() {
            let mut strip = row().key("strip").w_full().h(132.0).gap(10.0);
            for watch in others {
                strip = strip.child(tile(s, t, watch, Mode::Thumbnail));
            }
            area = area.child(strip);
        }
    } else {
        // Two columns: two streams side by side, three or four in two rows.
        let mut grid = col().key("grid").grow(1.0).min_h(0.0).w_full().gap(10.0);
        for (index, pair) in watching.chunks(2).enumerate() {
            let mut line = row()
                .key(format!("grid-row-{index}"))
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .gap(10.0);
            for watch in pair {
                line = line.child(tile(s, t, watch, Mode::Grid));
            }
            if pair.len() == 1 && watching.len() > 2 {
                line = line.child(el().grow(1.0).min_w(0.0));
            }
            grid = grid.child(line);
        }
        area = area.child(grid);
    }
    let available = unwatched(s);
    if !available.is_empty() {
        let mut chips = row()
            .key("also-live")
            .w_full()
            .gap(8.0)
            .center_y()
            .flex_wrap()
            .child(faint(t, "Also live:"));
        for stream in available {
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
    let _ = k;
    area
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The focused or only stream, filling the stage.
    Stage,
    /// One of several equals.
    Grid,
    /// A small tile under the focused stream.
    Thumbnail,
}

fn tile(s: &App, t: &Theme, watch: &Watching, mode: Mode) -> El {
    let id = watch.stream.id.clone();
    let several = s.streaming.watching.len() > 1;
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
            el().w_full()
                .h_full()
                .pixels_fit(
                    s.streaming.surfaces.get(watch.slot).pixels(),
                    SurfaceFit::Fit,
                )
                .rotate(frame.rotation),
        ),
        None => video.child(
            col().gap(10.0).center().child(pulse(t.accent)).child(
                text(watch.phase.label())
                    .font(ty::SMALL)
                    .color(Color::WHITE.alpha(0.7)),
            ),
        ),
    };
    if mode != Mode::Stage && several {
        video = video
            .role("button")
            .label(format!("Focus {}", watch.stream.name))
            .help("Click to put this stream on the stage")
            .cursor(rune::Cursor::Pointer)
            .on_press_tx(send(Message::FocusStream(Some(id.clone()))));
    } else if mode == Mode::Stage && several {
        video = video
            .help("Double-click to show every stream")
            .on_double_press_tx(send(Message::FocusStream(None)));
    }
    if s.streaming.show_stats && mode != Mode::Thumbnail {
        video = video.child(stats(s, watch).absolute(10.0, 10.0));
    }
    let mut frame = col()
        .key(format!("tile-{id}"))
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(
            1.0,
            if receiving {
                t.hairline
            } else {
                t.fair.alpha(0.4)
            },
        )
        .clip_rounded(radius::CARD)
        .transition(Transition::opacity().combined(Transition::scale(0.98)))
        .motion(motion::SETTLE)
        .child(video);
    if mode == Mode::Thumbnail {
        frame = frame.child(
            row()
                .w_full()
                .px(8.0)
                .py(5.0)
                .gap(6.0)
                .center_y()
                .child(if receiving {
                    lit_dot(t.live)
                } else {
                    pulse(t.fair)
                })
                .child(
                    text(publisher_name(s, watch.stream.publisher))
                        .font(ty::SMALL)
                        .color(t.text)
                        .truncate_fade()
                        .grow(1.0)
                        .min_w(0.0),
                ),
        );
    } else {
        frame = frame.child(controls(s, t, watch, mode));
    }
    frame
}

/// Under a stream: who and what, its volume, focus and stop.
fn controls(s: &App, t: &Theme, watch: &Watching, mode: Mode) -> El {
    let id = watch.stream.id.clone();
    let publisher = watch.stream.publisher;
    let volume = s.streaming.volume(publisher);
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
        bar = bar
            .child(icon_button(
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
            ))
            .child(
                el().w(if mode == Mode::Stage { 120.0 } else { 80.0 })
                    .child(slider(
                        t,
                        "stream-volume",
                        "Stream volume",
                        i32::from(volume),
                        format!("{volume}%"),
                        Range::PERCENT,
                        move |s: &App| i32::from(s.streaming.volume(publisher)),
                        move |value| Message::StreamVolume(publisher, value.clamp(0, 100) as u8),
                    )),
            );
    }
    if s.streaming.watching.len() > 1 {
        bar = bar.child(if mode == Mode::Stage {
            icon_button(
                t,
                "unfocus",
                &icons::MINIMIZE,
                "Show every stream side by side",
                Variant::Ghost,
                Some(Message::FocusStream(None)),
            )
        } else {
            icon_button(
                t,
                "focus",
                &icons::MAXIMIZE,
                "Put this stream on the stage",
                Variant::Ghost,
                Some(Message::FocusStream(Some(id.clone()))),
            )
        });
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

/// Choosing what to share, its quality and audio; or, while sharing, who watches.
pub fn share_dialog(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if !s.streaming.share_open {
        return;
    }
    let sharing = &s.streaming.sharing;
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
                        |s: &App| s.streaming.sharing.quality.frame_rate as i32,
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
                |s: &App| s.streaming.sharing.quality.video_kbps as i32,
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
                        let next = !s.streaming.sharing.audio;
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
                    |s: &App| s.streaming.sharing.quality.audio_kbps as i32,
                    |value| Message::ShareAudioBitrate(value as u32),
                ))
                .child(faint(
                    t,
                    "Shares everything on this output, including notifications. Your microphone stays separate.",
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
        Some(s.streaming.sharing.quality.resolution.label()),
        "Resolution",
        true,
        224.0,
        |index| Message::ShareResolution(Resolution::ALL[index]),
        menus,
    )
}

/// The share dialog's pickers span its width.
const SHARE_PICKER: f32 = 480.0;

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
