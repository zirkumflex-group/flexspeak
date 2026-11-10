use super::{Stats, Surface, protocol::Input};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", target_os = "ios")),
    allow(dead_code)
)]
pub enum Output {
    Signal(Value),
    TransportConnected,
    Playing,
    Stats(Stats),
    Failed(String),
}

pub struct Receiver {
    pub input: mpsc::Sender<Input>,
    pub output: mpsc::Receiver<Output>,
    cancel: watch::Sender<bool>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "ios")))]
pub fn start(_: String, _: Vec<Input>, _: u64, _: Arc<Surface>) -> Result<Receiver> {
    anyhow::bail!(
        "The experimental native video receiver is currently available on Mac and Windows."
    )
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "ios"))]
pub fn start(
    offer: String,
    candidates: Vec<Input>,
    generation: u64,
    surface: Arc<Surface>,
) -> Result<Receiver> {
    start_with_servers(
        offer,
        candidates,
        generation,
        surface,
        vec![
            "stun:turn.teamspeak.com:3478".into(),
            "stun:turn2.teamspeak.com:3478".into(),
        ],
    )
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "ios"))]
pub(super) fn start_with_servers(
    offer: String,
    candidates: Vec<Input>,
    generation: u64,
    surface: Arc<Surface>,
    servers: Vec<String>,
) -> Result<Receiver> {
    let (input, commands) = mpsc::channel(32);
    let (events, output) = mpsc::channel(64);
    let (cancel, cancelled) = watch::channel(false);
    std::thread::Builder::new()
        .name("stream-receiver".into())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(native::receive(
                    (offer, candidates),
                    generation,
                    surface.clone(),
                    commands,
                    cancelled,
                    &events,
                    servers,
                ))
            })();
            surface.end(generation);
            if let Err(error) = result {
                let _ = events.try_send(Output::Failed(error.to_string()));
            }
        })?;
    Ok(Receiver {
        input,
        output,
        cancel,
    })
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "ios"))]
mod native {
    use super::*;
    use crate::streaming::{MAX_CANDIDATES, protocol};
    use anyhow::{anyhow, bail, ensure};
    use futures::StreamExt;
    use libwebrtc::{
        MediaType,
        audio_stream::native::NativeAudioStream,
        ice_candidate::IceCandidate,
        media_stream_track::MediaStreamTrack,
        peer_connection::{AnswerOptions, PeerConnection, PeerConnectionState},
        peer_connection_factory::{
            ContinualGatheringPolicy, IceServer, PeerConnectionFactory, RtcConfiguration,
            native::PeerConnectionFactoryExt,
        },
        rtp_transceiver::{RtpTransceiverDirection, RtpTransceiverInit},
        session_description::{SdpType, SessionDescription},
        stats::{IceCandidatePairState, RtcStats},
        video_frame::{BoxVideoFrame, VideoFormatType, native::VideoFrameBufferExt},
        video_stream::native::NativeVideoStream,
    };
    use serde_json::json;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };

    enum Callback {
        Candidate(Value),
        State(PeerConnectionState),
        Track(MediaStreamTrack),
    }

    fn send(output: &mpsc::Sender<Output>, event: Output) -> Result<()> {
        output
            .try_send(event)
            .map_err(|_| anyhow!("The video event queue is unavailable"))
    }

    async fn accept_offer(
        pc: &PeerConnection,
        sdp: &str,
        candidates: &mut Vec<Input>,
        output: &mpsc::Sender<Output>,
    ) -> Result<()> {
        protocol::offer(sdp)?;
        ensure!(
            sdp.lines().any(|line| line.starts_with("m=video ")),
            "The publisher did not offer video"
        );
        let offer = SessionDescription::parse(sdp, SdpType::Offer)
            .map_err(|_| anyhow!("The video offer could not be parsed"))?;
        pc.set_remote_description(offer)
            .await
            .map_err(|_| anyhow!("The video offer could not be accepted"))?;
        for candidate in candidates.drain(..) {
            add_candidate(pc, candidate).await;
        }
        let answer = pc
            .create_answer(AnswerOptions::default())
            .await
            .map_err(|_| anyhow!("Could not create a video answer"))?;
        let wire = answer.to_string();
        // This binding cannot safely inspect an empty sender track; validate the generated SDP.
        ensure!(
            !wire
                .lines()
                .any(|line| matches!(line, "a=sendrecv" | "a=sendonly")),
            "A stream receiver must not send local media"
        );
        pc.set_local_description(answer)
            .await
            .map_err(|_| anyhow!("Could not apply the video answer"))?;
        send(output, Output::Signal(protocol::answer(wire)))
    }

    async fn add_candidate(pc: &PeerConnection, candidate: Input) {
        if let Input::Candidate { sdp, mid, mline } = candidate
            && let Ok(candidate) = IceCandidate::parse(&mid, mline, &sdp)
        {
            // Unusable candidates are normal; ICE only needs one working pair.
            let _ = pc.add_ice_candidate(candidate).await;
        }
    }

    pub(super) async fn receive(
        (offer, mut pending): (String, Vec<Input>),
        generation: u64,
        surface: Arc<Surface>,
        mut input: mpsc::Receiver<Input>,
        mut cancel: watch::Receiver<bool>,
        output: &mpsc::Sender<Output>,
        servers: Vec<String>,
    ) -> Result<()> {
        let factory = PeerConnectionFactory::default();
        factory.set_adm_recording_enabled(false);
        factory.set_adm_playout_enabled(false);
        let mut config = RtcConfiguration::default();
        config.continual_gathering_policy = ContinualGatheringPolicy::GatherOnce;
        config.ice_servers = if servers.is_empty() {
            vec![]
        } else {
            vec![IceServer {
                urls: servers,
                username: String::new(),
                password: String::new(),
            }]
        };
        let pc = factory
            .create_peer_connection(config)
            .map_err(|_| anyhow!("Could not initialize the native video receiver"))?;
        let (callbacks, mut callback_rx) = mpsc::channel(64);
        let overflow = Arc::new(AtomicBool::new(false));
        {
            let tx = callbacks.clone();
            let full = overflow.clone();
            pc.on_ice_candidate(Some(Box::new(move |candidate| {
                let event = Callback::Candidate(json!({"cmd":"iceCandidate", "args":{"sdp":candidate.candidate(), "mid":candidate.sdp_mid(), "mLine":candidate.sdp_mline_index()}}));
                if tx.try_send(event).is_err() { full.store(true, Ordering::Relaxed); }
            })));
        }
        {
            let tx = callbacks.clone();
            let full = overflow.clone();
            pc.on_connection_state_change(Some(Box::new(move |state| {
                if tx.try_send(Callback::State(state)).is_err() {
                    full.store(true, Ordering::Relaxed);
                }
            })));
        }
        {
            let tx = callbacks;
            let full = overflow.clone();
            pc.on_track(Some(Box::new(move |event| {
                if tx.try_send(Callback::Track(event.track)).is_err() {
                    full.store(true, Ordering::Relaxed);
                }
            })));
        }
        let mut video: Option<NativeVideoStream> = None;
        let mut audio: Option<NativeAudioStream> = None;
        let result = async {
            for media in [MediaType::Video, MediaType::Audio] {
                pc.add_transceiver_for_media(media, RtpTransceiverInit { direction: RtpTransceiverDirection::RecvOnly, stream_ids: vec![], send_encodings: vec![] })
                    .map_err(|_| anyhow!("Could not create a receive-only media track"))?;
            }
            let accept = tokio::time::timeout(Duration::from_secs(15), accept_offer(&pc, &offer, &mut pending, output));
            tokio::select! {
                _ = cancel.changed() => return Ok(()),
                result = accept => result.map_err(|_| anyhow!("Video negotiation timed out"))??,
            }
            let started = Instant::now();
            let mut first_frame = false;
            let mut last_presented = Instant::now() - Duration::from_secs(1);
            let mut candidate_count = 0;
            let mut stats_tick = tokio::time::interval(Duration::from_secs(1));
            stats_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                if *cancel.borrow() { break; }
                ensure!(!overflow.load(Ordering::Relaxed), "Native video signaling overflowed its queue");
                tokio::select! {
                    _ = cancel.changed() => break,
                    message = input.recv() => match message {
                        Some(Input::Offer(sdp)) => {
                            let negotiation = tokio::time::timeout(Duration::from_secs(15), accept_offer(&pc, &sdp, &mut pending, output));
                            tokio::select! { _ = cancel.changed() => break, result = negotiation => result.map_err(|_| anyhow!("Video renegotiation timed out"))?? }
                        },
                        Some(candidate) => {
                            candidate_count += 1;
                            ensure!(candidate_count <= MAX_CANDIDATES, "Too many video connection candidates");
                            add_candidate(&pc, candidate).await;
                        },
                        None => break,
                    },
                    event = callback_rx.recv() => match event {
                        Some(Callback::Candidate(value)) => send(output, Output::Signal(value))?,
                        Some(Callback::State(PeerConnectionState::Connected)) => send(output, Output::TransportConnected)?,
                        Some(Callback::State(PeerConnectionState::Disconnected | PeerConnectionState::Failed | PeerConnectionState::Closed)) => bail!("The video connection was interrupted. Watch again to reconnect."),
                        Some(Callback::Track(MediaStreamTrack::Video(track))) => {
                            if let Some(stream) = &mut video { stream.close(); }
                            video = Some(NativeVideoStream::new(track));
                        },
                        Some(Callback::Track(MediaStreamTrack::Audio(track))) => {
                            if let Some(stream) = &mut audio { stream.close(); }
                            audio = Some(NativeAudioStream::new(track,48000,2));
                        },
                        Some(Callback::State(_)) => {},
                        None => break,
                    },
                    frame = async { match &mut video { Some(stream) => stream.next().await, None => std::future::pending().await } } => {
                        let Some(frame) = frame else { bail!("The video track ended"); };
                        if last_presented.elapsed() < Duration::from_millis(33) { continue; }
                        present_frame(frame, generation, &surface)?;
                        last_presented = Instant::now();
                        if !first_frame { first_frame = true; send(output, Output::Playing)?; }
                    },
                    frame = async { match &mut audio { Some(stream) => stream.next().await, None => std::future::pending().await } } => {
                        let Some(frame) = frame else { audio = None; continue; };
                        ensure!(frame.sample_rate == 48000 && frame.num_channels == 2,"Unexpected stream audio format");
                        surface.publish_audio(generation,&frame.data);
                    },
                    _ = stats_tick.tick() => {
                        ensure!(first_frame || started.elapsed() < Duration::from_secs(30), "No decoded video arrived within 30 seconds");
                        if let Ok(Ok(stats)) = tokio::time::timeout(Duration::from_secs(2), pc.get_stats()).await {
                            let _ = output.try_send(Output::Stats(summarize_stats(&stats)));
                        }
                    }
                }
            }
            Ok(())
        }.await;
        if let Some(mut stream) = video {
            stream.close();
        }
        if let Some(mut stream) = audio {
            stream.close();
        }
        pc.on_track(None);
        pc.on_ice_candidate(None);
        pc.on_connection_state_change(None);
        pc.close();
        drop(pc);
        drop(factory);
        result
    }

    fn present_frame(frame: BoxVideoFrame, generation: u64, surface: &Surface) -> Result<()> {
        let width = frame.buffer.width();
        let height = frame.buffer.height();
        ensure!(
            width >= 2 && height >= 2 && width <= 7680 && height <= 4320,
            "Unsupported video frame dimensions"
        );
        let scale = (1280.0 / width as f64).min(720.0 / height as f64).min(1.0);
        let display_width = ((width as f64 * scale) as u32).max(2) & !1;
        let display_height = ((height as f64 * scale) as u32).max(2) & !1;
        let mut buffer = frame.buffer.to_i420();
        if display_width != width || display_height != height {
            buffer = buffer.scale(display_width as i32, display_height as i32);
        }
        let mut rgba = vec![0; display_width as usize * display_height as usize * 4];
        // libyuv ABGR is RGBA byte order on the supported little-endian targets.
        buffer.to_argb(
            VideoFormatType::ABGR,
            &mut rgba,
            display_width * 4,
            display_width as i32,
            display_height as i32,
        );
        surface.publish(
            generation,
            crate::streaming::Image {
                width: display_width,
                height: display_height,
                pixels: rgba,
            },
            (frame.rotation as i32 as f32).to_radians(),
        );
        Ok(())
    }

    fn summarize_stats(stats: &[RtcStats]) -> Stats {
        let mut summary = Stats::default();
        let inbound = stats.iter().find_map(|stat| match stat {
            RtcStats::InboundRtp(stat) if stat.stream.kind == "video" => Some(stat),
            _ => None,
        });
        if let Some(inbound) = inbound {
            summary.packets = inbound.received.packets_received;
            summary.bytes = inbound.inbound.bytes_received;
            summary.decoded_frames = inbound.inbound.frames_decoded as u64;
            summary.fps = inbound.inbound.frames_per_second;
            summary.width = inbound.inbound.frame_width;
            summary.height = inbound.inbound.frame_height;
            summary.decoder = inbound
                .inbound
                .decoder_implementation
                .chars()
                .take(128)
                .collect();
            if let Some(codec) = stats.iter().find_map(|stat| match stat {
                RtcStats::Codec(codec) if codec.rtc.id == inbound.stream.codec_id => Some(codec),
                _ => None,
            }) {
                summary.codec = codec.codec.mime_type.clone();
                summary.codec_parameters = codec.codec.sdp_fmtp_line.chars().take(256).collect();
            }
        }
        let pair = stats.iter().find_map(|stat| match stat {
            RtcStats::CandidatePair(pair)
                if pair.candidate_pair.nominated
                    && pair.candidate_pair.state == Some(IceCandidatePairState::Succeeded) =>
            {
                Some(&pair.candidate_pair)
            }
            _ => None,
        });
        if let Some(pair) = pair {
            let local = stats.iter().find_map(|stat| match stat {
                RtcStats::LocalCandidate(c) if c.rtc.id == pair.local_candidate_id => {
                    Some(&c.local_candidate)
                }
                _ => None,
            });
            let remote = stats.iter().find_map(|stat| match stat {
                RtcStats::RemoteCandidate(c) if c.rtc.id == pair.remote_candidate_id => {
                    Some(&c.remote_candidate)
                }
                _ => None,
            });
            if let (Some(local), Some(remote)) = (local, remote) {
                let label = |kind: Option<libwebrtc::stats::IceCandidateType>| {
                    kind.map_or_else(|| "Unknown".into(), |kind| format!("{kind:?}"))
                };
                summary.ice_path = format!(
                    "{} ↔ {} · {}",
                    label(local.candidate_type),
                    label(remote.candidate_type),
                    local.protocol.to_uppercase()
                );
            }
        }
        summary
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use libwebrtc::{
            peer_connection::OfferOptions,
            video_frame::{I420Buffer, VideoFrame, VideoRotation},
            video_source::{VideoResolution, native::NativeVideoSource},
        };

        struct ClosePeer(PeerConnection);
        impl Drop for ClosePeer {
            fn drop(&mut self) {
                self.0.close();
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        #[ignore = "Native codec/UDP integration; synthetic frames only, no capture devices or external STUN"]
        async fn native_receiver_decodes_synthetic_video_and_stops() -> Result<()> {
            let factory = PeerConnectionFactory::default();
            factory.set_adm_recording_enabled(false);
            factory.set_adm_playout_enabled(false);
            let pc = factory.create_peer_connection(RtcConfiguration::default())?;
            let _close = ClosePeer(pc.clone());
            let source = NativeVideoSource::new(
                VideoResolution {
                    width: 320,
                    height: 180,
                },
                true,
            );
            let track = factory.create_video_track("synthetic-video", source.clone());
            pc.add_transceiver(
                MediaStreamTrack::Video(track),
                RtpTransceiverInit {
                    direction: RtpTransceiverDirection::SendOnly,
                    stream_ids: vec!["synthetic".into()],
                    send_encodings: vec![],
                },
            )?;
            let (candidates, mut candidate_rx) = mpsc::channel(32);
            pc.on_ice_candidate(Some(Box::new(move |candidate| {
                let _ = candidates.try_send(Input::Candidate {
                    sdp: candidate.candidate(),
                    mid: candidate.sdp_mid(),
                    mline: candidate.sdp_mline_index(),
                });
            })));
            let offer = pc.create_offer(OfferOptions::default()).await?;
            let sdp = offer.to_string();
            pc.set_local_description(offer).await?;
            let surface = Arc::new(Surface::default());
            let generation = surface.begin();
            let mut receiver =
                super::super::start_with_servers(sdp, vec![], generation, surface.clone(), vec![])?;
            let start = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_millis(67));
            let mut answered = false;
            let mut pending = Vec::new();
            let mut colors = std::collections::HashSet::new();
            let mut delivered = 0;
            let mut decoded = 0;
            let mut final_stats = Stats::default();
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    tokio::select! {
                        candidate = candidate_rx.recv() => if let Some(candidate) = candidate { receiver.input.send(candidate).await?; },
                        event = receiver.output.recv() => match event {
                            Some(Output::Signal(value)) if value["cmd"] == "answer" => {
                                let sdp = value["args"]["answer"].as_str().unwrap();
                                ensure!(sdp.contains("a=recvonly"), "Receiver did not negotiate receive-only video");
                                pc.set_remote_description(SessionDescription::parse(sdp, SdpType::Answer)?).await?;
                                answered = true;
                                for candidate in pending.drain(..) { add_candidate(&pc, candidate).await; }
                            },
                            Some(Output::Signal(value)) => {
                                if let Some(candidate) = protocol::incoming_signal(&value.to_string())? {
                                    if answered { add_candidate(&pc, candidate).await; } else { pending.push(candidate); }
                                }
                            },
                            Some(Output::Stats(stats)) => { decoded = stats.decoded_frames; final_stats = stats; },
                            Some(Output::Failed(error)) => bail!("{error}"),
                            Some(_) => {},
                            None => bail!("Receiver exited before video was validated"),
                        },
                        _ = tick.tick() => {
                            let mut buffer = I420Buffer::new(320, 180);
                            let (y, u, v) = buffer.data_mut();
                            y.fill(50 + (start.elapsed().as_millis() / 100 % 120) as u8);
                            u.fill(90); v.fill(240);
                            let mut frame = VideoFrame::new(VideoRotation::VideoRotation0, buffer);
                            frame.timestamp_us = start.elapsed().as_micros() as i64;
                            source.capture_frame(&frame);
                            if let Some(crate::streaming::Image { width, height, pixels }) = surface.take() {
                                ensure!((width, height) == (320, 180), "Unexpected decoded dimensions");
                                let offset = ((height / 2 * width + width / 2) * 4) as usize;
                                let data: &[u8] = pixels.as_ref();
                                if data[offset] > data[offset + 2].saturating_add(40) {
                                    ensure!(data[offset + 3] == 255, "Incorrect RGBA alpha/channel order");
                                    colors.insert(data[offset]); delivered += 1;
                                }
                            }
                        }
                    }
                    if delivered >= 5 && decoded >= 5 && colors.len() >= 2 { break; }
                }
                Ok::<_, anyhow::Error>(())
            }).await.map_err(|_| anyhow!("Synthetic native receiver timed out"))??;
            println!(
                "Native video proof: {} · {}x{} · {} RTP packets · {} decoded frames · {} RGBA frames · {} distinct red levels · ICE {}",
                final_stats.codec,
                final_stats.width,
                final_stats.height,
                final_stats.packets,
                decoded,
                delivered,
                colors.len(),
                final_stats.ice_path
            );
            receiver.cancel.send(true)?;
            tokio::time::timeout(Duration::from_secs(5), async {
                while receiver.output.recv().await.is_some() {}
            })
            .await?;
            assert!(surface.take().is_none());
            pc.close();
            Ok(())
        }
    }
}
