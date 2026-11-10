use crate::{
    audio::{self, PlaybackVolume},
    client::{self, Activity, ActivityKind, Channel, Command, Config, Event, Member, Snapshot},
    hub::{AudioHub, SessionId},
    streaming::{
        self, StreamInfo,
        broadcast::{self, Picture},
        publishing::{Access, Options, Quality, Resolution, ShareSource, SourceKind},
    },
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::mpsc;

/// Receives each event as JSON, on a background thread. The text is valid only during the call.
pub type Callback = extern "C" fn(*const c_char, *mut c_void);

struct Sink {
    callback: Callback,
    context: *mut c_void,
}

// The Swift side owns the context for the app's lifetime and accepts calls from any thread.
unsafe impl Send for Sink {}

impl Sink {
    fn send(&self, event: &Value) {
        if let Ok(text) = CString::new(event.to_string()) {
            (self.callback)(text.as_ptr(), self.context);
        }
    }
}

pub struct Session {
    commands: mpsc::Sender<Command>,
    surfaces: Arc<streaming::Surfaces>,
}

unsafe fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // Swift passes NUL-terminated strings that outlive this call.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// Starts a session, or reports why it could not through `callback` and returns null.
///
/// # Safety
/// The strings must be valid and NUL-terminated, or null. `context` must stay valid for as long
/// as events can arrive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flexspeak_connect(
    address: *const c_char,
    nickname: *const c_char,
    password: *const c_char,
    callback: Callback,
    context: *mut c_void,
) -> *mut Session {
    let sink = Sink { callback, context };
    let (address, nickname, password) = unsafe { (text(address), text(nickname), text(password)) };
    match start(address, nickname, password) {
        Ok((session, mut events)) => {
            std::thread::spawn(move || {
                while let Some(event) = events.blocking_recv() {
                    sink.send(&encode(event));
                }
            });
            Box::into_raw(Box::new(session))
        }
        Err(error) => {
            sink.send(&json!({ "type": "stopped", "error": format!("{error:#}") }));
            std::ptr::null_mut()
        }
    }
}

fn start(
    address: String,
    nickname: String,
    password: String,
) -> Result<(Session, mpsc::Receiver<Event>)> {
    let devices = audio::devices().context("Cannot list audio devices")?;
    let id = SessionId(1);
    let hub = Arc::new(AudioHub::default());
    hub.set_active(Some(id));
    let surfaces = Arc::new(streaming::Surfaces::default());
    let (commands, events) = client::start(
        Config {
            user_gains: Default::default(),
            id,
            hub,
            channel_health_visible: Arc::new(AtomicBool::new(false)),
            subscribe_all: Arc::new(AtomicBool::new(true)),
            streaming_visible: Arc::new(AtomicBool::new(true)),
            video_surfaces: surfaces.clone(),
            playback_volume: Arc::new(PlaybackVolume::default()),
            audio_processing: Default::default(),
            transmission: Default::default(),
            ptt: Arc::new(AtomicBool::new(false)),
            address,
            nickname,
            password,
            input: devices.input.context("This device has no microphone")?,
            output: devices.output.context("This device has no audio output")?,
        },
        false,
    )?;
    Ok((Session { commands, surfaces }, events))
}

/// Sends one JSON command to the session. Unknown commands are ignored.
///
/// # Safety
/// `session` must come from `flexspeak_connect` and not have been disconnected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flexspeak_command(session: *mut Session, command: *const c_char) {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return;
    };
    let command = unsafe { text(command) };
    let Some(command) = serde_json::from_str(&command).ok().and_then(decode) else {
        return;
    };
    let _ = session.commands.try_send(command);
}

/// Ends the session and frees it. Events stop after one final `stopped`.
///
/// # Safety
/// `session` must come from `flexspeak_connect`, and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flexspeak_disconnect(session: *mut Session) {
    if session.is_null() {
        return;
    }
    let session = unsafe { Box::from_raw(session) };
    let _ = session.commands.try_send(Command::Stop);
}

/// Copies the newest picture of a watched stream into `buffer` as RGBA. Returns false when no
/// new picture arrived since the last call.
///
/// # Safety
/// `session` must be live, `buffer` must hold `capacity` writable bytes, and the two sizes must
/// be valid to write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flexspeak_frame(
    session: *mut Session,
    slot: usize,
    buffer: *mut u8,
    capacity: usize,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    let Some(session) = (unsafe { session.as_ref() }) else {
        return false;
    };
    let target = unsafe { std::slice::from_raw_parts_mut(buffer, capacity) };
    let Some((w, h)) = session.surfaces.get(slot).copy_latest(target) else {
        return false;
    };
    unsafe {
        *width = w;
        *height = h;
    }
    true
}

/// Hands one NV12 screen picture from the broadcast extension to the running share.
///
/// # Safety
/// `y` and `uv` must be readable for `y_len` and `uv_len` bytes during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn flexspeak_push_screen(
    y: *const u8,
    y_len: usize,
    y_stride: u32,
    uv: *const u8,
    uv_len: usize,
    uv_stride: u32,
    width: u32,
    height: u32,
    rotation: u32,
) {
    if y.is_null() || uv.is_null() {
        return;
    }
    broadcast::deliver(
        Picture {
            y: unsafe { std::slice::from_raw_parts(y, y_len) },
            y_stride,
            uv: unsafe { std::slice::from_raw_parts(uv, uv_len) },
            uv_stride,
            width,
            height,
        },
        rotation,
    );
}

fn decode(value: Value) -> Option<Command> {
    let text = |key: &str| value[key].as_str().map(str::to_owned);
    let flag = |key: &str| value[key].as_bool();
    let id = |key: &str| value[key].as_u64();
    Some(match value["type"].as_str()? {
        "mute" => Command::Mute(flag("value")?),
        "deafen" => Command::Deafen(flag("value")?),
        "join" => Command::Join(id("channel")?, text("password").unwrap_or_default()),
        "poke" => Command::Poke(u16::try_from(id("client")?).ok()?, text("message")?),
        "move" => Command::Move(vec![u16::try_from(id("client")?).ok()?], id("channel")?),
        "refreshStreams" => Command::RefreshStreams,
        "watch" => Command::WatchStream(text("stream")?),
        "stopWatching" => Command::StopWatching(text("stream")),
        "startSharing" => Command::StartSharing(Options {
            source: ShareSource {
                kind: SourceKind::Screen(None),
                name: "iPhone screen".into(),
            },
            audio: None,
            quality: phone_quality(text("quality").as_deref()),
            access: if text("access").as_deref() == Some("public") {
                Access::Public
            } else {
                Access::Private
            },
        }),
        "stopSharing" => Command::StopSharing,
        "decideViewer" => Command::DecideViewer(u16::try_from(id("client")?).ok()?, flag("allow")?),
        _ => return None,
    })
}

/// Phones encode in software, so they share at a lower frame rate than the desktop presets.
fn phone_quality(name: Option<&str>) -> Quality {
    let (resolution, video_kbps) = if name == Some("saver") {
        (Resolution::P480, 800)
    } else {
        (Resolution::P720, 2000)
    };
    Quality {
        resolution,
        frame_rate: 15,
        video_kbps,
        audio_kbps: 64,
    }
}

fn member(member: &Member) -> Value {
    json!({
        "id": member.id,
        "name": member.name,
        "isSelf": member.is_self,
        "micMuted": member.mic_muted,
        "soundMuted": member.sound_muted,
        "canPoke": member.can_poke,
        "canMove": member.can_move,
    })
}

fn channel(channel: &Channel) -> Value {
    json!({
        "id": channel.id,
        "name": channel.name,
        "depth": channel.depth,
        "hasPassword": channel.has_password,
        "canJoin": channel.join_access.can_request(),
        "members": channel.members.iter().map(member).collect::<Vec<_>>(),
    })
}

fn snapshot(state: &Snapshot) -> Value {
    json!({
        "type": "state",
        "server": state.server,
        "serverVersion": state.server_version,
        "channels": state.channels.iter().map(channel).collect::<Vec<_>>(),
        "current": state.current.as_ref().map(|channel| channel.id),
        "canTalk": state.can_talk,
    })
}

fn activity(activity: &Activity) -> Option<String> {
    let name = &activity.name;
    let channel = &activity.channel;
    Some(match activity.kind {
        ActivityKind::Connected => format!("{name} connected"),
        ActivityKind::Disconnected => format!("{name} disconnected"),
        ActivityKind::LostConnection => format!("{name} lost connection"),
        ActivityKind::Moved if activity.is_self => format!("You moved to {channel}"),
        ActivityKind::Moved if activity.entered_yours => format!("{name} joined your channel"),
        ActivityKind::Moved if activity.left_yours => format!("{name} left your channel"),
        ActivityKind::Moved => return None,
        ActivityKind::KickedFromChannel => format!("{name} was kicked from the channel"),
        ActivityKind::KickedFromServer => format!("{name} was kicked from the server"),
        ActivityKind::Banned => format!("{name} was banned"),
    })
}

fn stream(stream: &StreamInfo) -> Value {
    json!({
        "id": stream.id,
        "publisher": stream.publisher,
        "name": stream.name,
        "viewers": stream.viewers,
        "hasAudio": stream.has_audio,
    })
}

fn encode(event: Event) -> Value {
    match event {
        Event::Status(text) => json!({ "type": "status", "text": text }),
        Event::Connected(audio) => json!({ "type": "connected", "text": audio }),
        Event::State(state) => snapshot(&state),
        Event::Activity(list) => json!({
            "type": "activity",
            "lines": list.iter().filter_map(activity).collect::<Vec<_>>(),
        }),
        Event::Talking(ids) => json!({ "type": "talking", "ids": ids }),
        Event::Transmitting(on) => json!({ "type": "transmitting", "value": on }),
        Event::Meter {
            level,
            sent,
            received,
            ..
        } => json!({
            "type": "meter",
            "level": level,
            "sent": sent,
            "received": received,
            "gaps": audio::PLAYBACK_GAPS.load(Ordering::Relaxed),
            "callbackFrames": audio::PLAYBACK_CALLBACK_FRAMES.load(Ordering::Relaxed),
        }),
        Event::Usage {
            network,
            connection,
        } => json!({
            "type": "usage",
            "upload": network.upload_per_second,
            "download": network.download_per_second,
            "uploaded": network.uploaded,
            "downloaded": network.downloaded,
            "ping": connection.ping_ms,
            "pingDeviation": connection.ping_deviation_ms,
            "lossIn": connection.packet_loss_in,
            "lossOut": connection.packet_loss_out,
        }),
        Event::Warning { source, detail } => json!({
            "type": "warning",
            "source": format!("{source:?}"),
            "text": detail,
        }),
        Event::Recovered(source) => json!({ "type": "recovered", "source": format!("{source:?}") }),
        Event::Controls { confirmed, pending } => json!({
            "type": "controls",
            "muted": confirmed.map(|state| state.muted),
            "deafened": confirmed.map(|state| state.deafened),
            "pending": pending,
        }),
        Event::ChannelPasswordRequired(id) => json!({ "type": "password", "channel": id }),
        Event::Poked { from, message } => json!({ "type": "poked", "from": from, "text": message }),
        Event::Done(source) => json!({ "type": "done", "source": format!("{source:?}") }),
        Event::Streaming(streaming::Event::Catalog(list)) => json!({
            "type": "catalog",
            "streams": list.iter().map(stream).collect::<Vec<_>>(),
        }),
        Event::Streaming(streaming::Event::Notice(text)) => {
            json!({ "type": "notice", "text": text })
        }
        Event::Streaming(streaming::Event::State {
            slot,
            stream: info,
            phase,
            detail,
            ..
        }) => json!({
            "type": "watch",
            "slot": slot,
            "stream": stream(&info),
            "active": phase.active(),
            "phase": phase.label(),
            "detail": detail,
        }),
        Event::Streaming(streaming::Event::Publishing(status)) => json!({
            "type": "sharing",
            "active": status.active,
            "live": status.live,
            "detail": status.detail,
            "videoFrames": status.video_frames,
            "requests": status.requests.iter()
                .map(|(id, name)| json!({ "id": id, "name": name }))
                .collect::<Vec<_>>(),
            "viewers": status.viewers.iter()
                .map(|(id, name, ready)| json!({ "id": id, "name": name, "ready": ready }))
                .collect::<Vec<_>>(),
        }),
        Event::Streaming(streaming::Event::Stats { stats, .. }) => json!({
            "type": "streamStats",
            "codec": stats.codec,
            "decoder": stats.decoder,
            "path": stats.ice_path,
            "fps": stats.fps,
            "width": stats.width,
            "height": stats.height,
            "decodedFrames": stats.decoded_frames,
        }),
        Event::Streaming(_) | Event::ClientUsage { .. } => json!({ "type": "ignored" }),
        Event::Stopped(error) => json!({
            "type": "stopped",
            "error": error.map(|(_, detail)| detail),
        }),
    }
}
