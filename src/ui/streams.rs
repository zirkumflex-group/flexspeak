//! The connected right pane: share your screen and watch streams in the current channel.

use rune::{Color, Cx, El, SurfaceFit, Transition, col, el, row, text};
use rune_kit::{Checkbox, Kit};

use super::theme::{Theme, radius, size, ty};
use super::widgets::{
    Variant, button, card, faint, group, group_note, group_title, heading, lit_dot, meter, muted,
    picker, pulse, scroll_page, title, well,
};
use super::{dispatch, icons};
use crate::{App, Message, streaming, usage};

pub fn view(s: &App, _cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let connected = s.connected && !s.loopback && !s.stopping;
    let supported = connected && s.state.server_version.starts_with("6.");
    let mut content = col()
        .w_full()
        .max_w(860.0)
        .padding(size::PAGE)
        .gap(10.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
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
                )
                .child(button(
                    t,
                    "refresh-streams",
                    "Refresh",
                    Some(&icons::REFRESH),
                    Variant::Secondary,
                    supported.then_some(Message::RefreshStreams),
                )),
        );
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
    if supported && streaming::AVAILABLE {
        content = content.child(share_card(s, t, k, layer));
    }
    let active_stream = s.streaming.stream.as_ref().filter(|_| s.streaming.active());
    if let Some(stream) = active_stream {
        content = content.child(player(s, t, stream));
    }
    let available: Vec<_> = s
        .streaming
        .catalog
        .iter()
        .filter(|stream| active_stream.is_none_or(|active| active.id != stream.id))
        .collect();
    if !available.is_empty() || active_stream.is_none() {
        content = content.child(group_title(t, "In this channel"));
    }
    let mut rows = Vec::new();
    if available.is_empty() && active_stream.is_none() {
        rows.push(
            col()
                .w_full()
                .padding(20.0)
                .center()
                .child(muted(t, "No one in this channel is sharing.")),
        );
    }
    for stream in available {
        let publisher = s
            .state
            .current_members()
            .iter()
            .find(|member| member.id == stream.publisher)
            .map_or("Publisher", |member| member.name.as_str());
        let can_watch = supported
            && streaming::AVAILABLE
            && !stream.server_relayed
            && !s.streaming.active()
            && !s.streaming.sharing.status.active;
        rows.push(
            row()
                .key(format!("stream-{}", stream.id))
                .w_full()
                .transition(Transition::opacity())
                .child(
                    row()
                        .w_full()
                        .px(size::CARD_PAD)
                        .py(10.0)
                        .gap(12.0)
                        .center_y()
                        .child(lit_dot(t.live))
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(2.0)
                                .child(
                                    text(stream.name.clone())
                                        .font(ty::BODY_STRONG)
                                        .color(t.text),
                                )
                                .child(faint(
                                    t,
                                    format!(
                                        "{publisher} · {} viewers{}",
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
                            if stream.server_relayed {
                                "Server relay unavailable"
                            } else {
                                "Watch"
                            },
                            None,
                            Variant::Secondary,
                            can_watch.then(|| Message::WatchStream(stream.id.clone())),
                        )),
                ),
        );
    }
    if !rows.is_empty() {
        content = content.child(group(t, rows));
    }
    content = content.child(group_note(
        t,
        "Leaving this view, switching channels or entering performance mode stops watching and sharing.",
    ));
    scroll_page(k, "streams-scroll", content)
}

fn share_card(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let sharing = &s.streaming.sharing;
    let status = &sharing.status;
    let mut controls = card(t).key("share").gap(10.0).child(
        row()
            .gap(10.0)
            .center_y()
            .children(status.active.then(|| lit_dot(t.live)))
            .child(heading(
                t,
                if status.active {
                    "You are sharing"
                } else {
                    "Share your screen"
                },
            )),
    );
    if status.active {
        controls = controls
            .border(1.0, t.live.alpha(0.45))
            .child(muted(t, status.detail.clone()))
            .child(
                row()
                    .gap(size::GAP)
                    .center_y()
                    .child(button(
                        t,
                        "stop-sharing",
                        "Stop Sharing",
                        Some(&icons::STOP),
                        Variant::Danger,
                        Some(Message::StopSharing),
                    ))
                    .child(faint(
                        t,
                        format!(
                            "{} / {} viewers · {} video frames · {} audio blocks",
                            status.viewers.len(),
                            streaming::publishing::MAX_VIEWERS,
                            status.video_frames,
                            status.audio_frames
                        ),
                    )),
            )
            .child(meter(t, "share-audio", status.audio_level.min(1.0), t.live));
        for (id, name, ready) in &status.viewers {
            controls = controls.child(
                row()
                    .key(format!("viewer-{id}"))
                    .gap(8.0)
                    .center_y()
                    .transition(Transition::opacity())
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
            controls = controls.child(
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
                            .then_some(Message::DecideViewer(*id, true)),
                    ))
                    .child(button(
                        t,
                        "deny",
                        "Deny",
                        None,
                        Variant::Secondary,
                        Some(Message::DecideViewer(*id, false)),
                    )),
            );
        }
        return controls;
    }
    let names: Vec<String> = sharing
        .sources
        .iter()
        .map(|source| source.name.clone())
        .collect();
    controls = controls
        .child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .flex_wrap()
                .child(picker(
                    k,
                    s,
                    "share-source",
                    names,
                    sharing.selected.as_ref().map(|source| source.name.as_str()),
                    "Choose a screen or window…",
                    true,
                    size::FORM_CONTROL,
                    {
                        let sources = sharing.sources.clone();
                        move |index| Message::ShareSource(sources[index].clone())
                    },
                    layer,
                ))
                .child(button(
                    t,
                    "choose-source",
                    if sharing.loading {
                        "Loading…"
                    } else {
                        "Find Sources"
                    },
                    None,
                    Variant::Secondary,
                    (!sharing.loading).then_some(Message::RefreshShareSources),
                )),
        )
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
        controls = controls
            .child(picker(
                k,
                s,
                "share-output",
                outputs,
                current.as_deref(),
                "System audio output",
                true,
                size::FORM_CONTROL,
                {
                    let outputs = s.devices.outputs.clone();
                    move |index| Message::ShareAudioOutput(outputs[index].clone())
                },
                layer,
            ))
            .child(faint(
                t,
                "Shares everything on this output, including notifications. Your microphone stays separate.",
            ));
    }
    controls = controls.child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .flex_wrap()
            .child(
                faint(t, "Up to 720p at 30 fps. You approve each viewer.")
                    .grow(1.0)
                    .min_w(0.0),
            )
            .child(button(
                t,
                "start-sharing",
                "Start Sharing",
                None,
                Variant::Primary,
                sharing.selected.is_some().then_some(Message::StartSharing),
            )),
    );
    if !status.detail.is_empty() {
        controls = controls.child(muted(t, status.detail.clone()));
    }
    controls
}

fn player(s: &App, t: &Theme, stream: &streaming::StreamInfo) -> El {
    let phase = s.streaming.phase.unwrap_or(streaming::Phase::Stopped);
    let receiving = phase == streaming::Phase::Receiving;
    let mut player = card(t)
        .key("player")
        .transition(Transition::opacity().combined(Transition::scale(0.98)))
        .child(
            row()
                .w_full()
                .gap(12.0)
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
                        .gap(3.0)
                        .child(heading(t, stream.name.clone()))
                        .child(faint(t, phase.label())),
                )
                .child(button(
                    t,
                    "stop-watching",
                    "Stop Watching",
                    Some(&icons::STOP),
                    Variant::Danger,
                    Some(Message::StopWatching),
                )),
        );
    if !s.streaming.detail.is_empty() {
        player = player.child(muted(t, s.streaming.detail.clone()));
    }
    let screen = if let Some(frame) = &s.streaming.frame {
        el().key("video")
            .w_full()
            .h(420.0)
            .radius(radius::CARD - 4.0)
            .clip_rounded(radius::CARD - 4.0)
            .fill(Color::BLACK)
            .child(
                el().w_full()
                    .h_full()
                    .pixels_fit(s.streaming.surface.pixels(), SurfaceFit::Fit)
                    .rotate(frame.rotation),
            )
    } else {
        well(t)
            .key("video-waiting")
            .h(260.0)
            .center()
            .gap(10.0)
            .child(pulse(t.accent))
            .child(muted(t, "Waiting for decoded video…"))
    };
    player = player.child(screen.transition(Transition::opacity()));
    let stats = &s.streaming.stats;
    if stats.packets > 0 || s.streaming.ui_updates > 0 {
        let mut details = well(t).padding(12.0).gap(4.0).child(
            text(format!(
                "{} · {}×{} · {:.1} decoded FPS",
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
            .color(t.text),
        );
        details = details
            .child(faint(
                t,
                format!(
                    "{} RTP packets · {} decoded frames · {} UI updates · {} received",
                    stats.packets,
                    stats.decoded_frames,
                    s.streaming.ui_updates,
                    usage::format_bytes(stats.bytes as f64)
                ),
            ))
            .child(faint(
                t,
                format!("Decoder: {} · ICE: {}", stats.decoder, stats.ice_path),
            ));
        if !stats.codec_parameters.is_empty() {
            details = details.child(faint(t, stats.codec_parameters.clone()));
        }
        if !stats.publisher_version.is_empty() {
            details = details.child(faint(t, format!("Publisher: {}", stats.publisher_version)));
        }
        player = player.child(details);
    }
    player
}
