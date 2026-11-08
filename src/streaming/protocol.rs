use super::{MAX_SDP_BYTES, StreamInfo};
use anyhow::{Result, bail, ensure};
use serde_json::Value;
#[cfg(any(target_os = "macos", target_os = "windows", test))]
use serde_json::json;
use std::{collections::BTreeMap, str};
use tsproto_packets::{
    commands::{CommandItem, CommandParser},
    packets::{Direction, Flags, OutCommand, PacketType},
};

pub type Args = BTreeMap<String, String>;

pub fn parse(name: &str, content: &str) -> Result<Vec<Args>> {
    ensure!(
        content.len() <= 128 * 1024,
        "Stream command exceeds the size limit"
    );
    let (wire_name, parser) = CommandParser::new(content.as_bytes());
    ensure!(wire_name == name.as_bytes(), "Stream command name mismatch");
    let mut parts = vec![Args::new()];
    for item in parser {
        match item {
            CommandItem::Argument(arg) => {
                let key = str::from_utf8(arg.name())?;
                let value = arg.value().get_str()?;
                let part = parts.last_mut().unwrap();
                ensure!(
                    part.len() < 64 && key.len() <= 128,
                    "Too many stream fields"
                );
                ensure!(
                    part.insert(key.into(), value.into_owned()).is_none(),
                    "Duplicate stream field"
                );
            }
            CommandItem::NextCommand => {
                ensure!(parts.len() < 128, "Too many stream entries");
                parts.push(Args::new());
            }
        }
    }
    Ok(parts)
}

pub fn stream_info(args: &Args) -> Option<StreamInfo> {
    let id = args.get("id")?;
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return None;
    }
    let publisher = args
        .get("clid")?
        .parse::<u16>()
        .ok()
        .filter(|id| *id != 0)?;
    Some(StreamInfo {
        id: id.clone(),
        publisher,
        name: args.get("name").map_or_else(
            || "Screen share".into(),
            |name| name.chars().filter(|c| !c.is_control()).take(256).collect(),
        ),
        viewers: args
            .get("viewer")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0),
        has_audio: args.get("audio").is_some_and(|value| value == "1"),
        server_relayed: args.get("mode").is_some_and(|value| value == "2"),
    })
}

pub fn request_info(publisher: u16) -> OutCommand {
    let mut command = command("requeststreaminfo");
    command.write_arg("clid", &publisher);
    command
}

pub fn client_version(publisher: u16) -> OutCommand {
    let mut command = command("clientgetvariables");
    command.write_arg("clid", &publisher);
    command
}

pub fn join(info: &StreamInfo, leave: bool) -> OutCommand {
    let mut command = command("joinstreamrequest");
    command.write_arg("id", &info.id);
    command.write_arg("clid", &info.publisher);
    command.write_arg("msg", &"");
    command.write_arg("is_remove", &u8::from(leave));
    command
}

pub fn signal(info: &StreamInfo, payload: &Value) -> Result<OutCommand> {
    signal_to(&info.id, info.publisher, payload)
}

pub fn signal_to(id: &str, peer: u16, payload: &Value) -> Result<OutCommand> {
    let json = serde_json::to_string(payload)?;
    ensure!(
        json.len() <= MAX_SDP_BYTES + 1024,
        "Outgoing signaling exceeds the size limit"
    );
    let mut command = command("streamsignaling");
    command.write_arg("id", &id);
    command.write_arg("clid", &peer);
    command.write_arg("json", &json);
    Ok(command)
}

pub fn setup(
    name: &str,
    source: &super::publishing::SourceKind,
    audio: bool,
    quality: super::publishing::Quality,
    access: super::publishing::Access,
) -> OutCommand {
    let mut packet = command("setupstream");
    packet.write_arg("name", &name);
    packet.write_arg("type", &source.wire_type());
    // The server keeps this in 16 bits, so it must be kbit/s; bit/s wraps around.
    packet.write_arg("bitrate", &(quality.bitrate() / 1000));
    packet.write_arg("accessibility", &access.wire_value());
    packet.write_arg("mode", &1_u8);
    packet.write_arg("viewer_limit", &super::publishing::MAX_VIEWERS);
    packet.write_arg("audio", &u8::from(audio));
    packet
}

pub fn stop(id: &str) -> OutCommand {
    let mut packet = command("stopstream");
    packet.write_arg("id", &id);
    packet.write_arg("reason", &"");
    packet
}

pub fn admit(id: &str, peer: u16, offer: Option<&str>) -> Result<OutCommand> {
    if let Some(sdp) = offer {
        self::offer(sdp)?;
    }
    let mut packet = command("respondjoinstreamrequest");
    packet.write_arg("id", &id);
    packet.write_arg("clid", &peer);
    packet.write_arg("msg", &"");
    packet.write_arg("decision", &u8::from(offer.is_some()));
    packet.write_arg("offer", &offer.unwrap_or(""));
    Ok(packet)
}

fn command(name: &str) -> OutCommand {
    OutCommand::new(Direction::C2S, Flags::empty(), PacketType::Command, name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Offer(String),
    Answer(String),
    Candidate {
        sdp: String,
        mid: String,
        mline: i32,
    },
}

pub fn offer(sdp: &str) -> Result<Input> {
    ensure!(
        sdp.len() <= MAX_SDP_BYTES && sdp.starts_with("v=0") && !sdp.contains('\0'),
        "Invalid or oversized video offer"
    );
    Ok(Input::Offer(sdp.into()))
}

pub fn incoming_signal(value: &str) -> Result<Option<Input>> {
    ensure!(
        value.len() <= MAX_SDP_BYTES + 1024,
        "Signaling exceeds the size limit"
    );
    let value: Value = serde_json::from_str(value)?;
    let args = &value["args"];
    match value["cmd"].as_str() {
        Some("answer") => {
            let sdp = args
                .as_str()
                .or_else(|| args["answer"].as_str())
                .or_else(|| args["sdp"].as_str())
                .ok_or_else(|| anyhow::anyhow!("Answer has no SDP"))?;
            offer(sdp)?;
            Ok(Some(Input::Answer(sdp.into())))
        }
        Some("offer" | "reconnectOffer") => {
            let sdp = args
                .as_str()
                .or_else(|| args["offer"].as_str())
                .or_else(|| args["sdp"].as_str())
                .ok_or_else(|| anyhow::anyhow!("Offer has no SDP"))?;
            offer(sdp).map(Some)
        }
        Some("iceCandidate") => {
            let Some(sdp) = args["sdp"].as_str().filter(|s| !s.is_empty()) else {
                return Ok(None);
            };
            ensure!(
                sdp.len() <= 4096 && !sdp.chars().any(char::is_control),
                "Invalid ICE candidate"
            );
            let mid = args["mid"].as_str().unwrap_or("0");
            let mline = args["mLine"].as_i64().unwrap_or(0);
            ensure!(
                mid.len() <= 128 && !mid.chars().any(char::is_control) && (0..32).contains(&mline),
                "Invalid ICE media identifier"
            );
            Ok(Some(Input::Candidate {
                sdp: sdp.into(),
                mid: mid.into(),
                mline: mline as i32,
            }))
        }
        Some(_) => Ok(None),
        None => bail!("Signaling has no command"),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn answer(sdp: String) -> Value {
    json!({"cmd":"answer", "args":{"answer":sdp}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_camera_advertises_selected_bitrate_source_and_access() {
        use super::super::publishing::{Access, Quality, SourceKind};
        let packet = setup(
            "Camera",
            &SourceKind::Camera("camera-id".into()),
            false,
            Quality {
                video_kbps: 8200,
                ..Quality::default()
            },
            Access::Public,
        );
        let args = parse("setupstream", str::from_utf8(packet.0.content()).unwrap()).unwrap();
        assert_eq!(args[0]["type"], "1");
        assert_eq!(args[0]["accessibility"], "1");
        assert_eq!(args[0]["bitrate"], "8200");
        assert_eq!(args[0]["audio"], "0");
    }

    #[test]
    fn publishing_commands_preserve_ts6_wire_fields_and_escaping() {
        use super::super::publishing::{Access, Quality, SourceKind};
        let packet = setup(
            "Window | test",
            &SourceKind::Window(1),
            true,
            Quality::default(),
            Access::Private,
        );
        let args = parse("setupstream", str::from_utf8(packet.0.content()).unwrap()).unwrap();
        assert_eq!(args[0]["name"], "Window | test");
        for (key, value) in [
            ("type", "3"),
            ("audio", "1"),
            ("accessibility", "3"),
            ("mode", "1"),
            ("viewer_limit", "4"),
            ("bitrate", "2500"),
        ] {
            assert_eq!(args[0][key], value);
        }
        let sdp = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n";
        let packet = admit("stream-a", 7, Some(sdp)).unwrap();
        let args = parse(
            "respondjoinstreamrequest",
            str::from_utf8(packet.0.content()).unwrap(),
        )
        .unwrap();
        assert_eq!(args[0]["id"], "stream-a");
        assert_eq!(args[0]["clid"], "7");
        assert_eq!(args[0]["decision"], "1");
        assert_eq!(args[0]["offer"], sdp);
        assert!(admit("stream-a", 7, Some("not SDP")).is_err());
        let packet = admit("stream-a", 7, None).unwrap();
        let args = parse(
            "respondjoinstreamrequest",
            str::from_utf8(packet.0.content()).unwrap(),
        )
        .unwrap();
        assert_eq!(args[0]["decision"], "0");
        assert!(args[0]["offer"].is_empty());
    }
    #[test]
    fn ts_escaping_and_multipart_discovery_preserve_names_and_reject_ambiguous_fields() {
        let parts = parse("notifystreaminfo", r"notifystreaminfo id=first clid=7 name=Window\sA\pB audio=1|id=second clid=8 name=Other").unwrap();
        let info = stream_info(&parts[0]).unwrap();
        assert_eq!(info.name, "Window A|B");
        assert!(info.has_audio);
        assert_eq!(stream_info(&parts[1]).unwrap().publisher, 8);
        assert!(parse("notifystreaminfo", "notifystreaminfo id=a id=b").is_err());
        assert!(parse("notifystreaminfo", "notifyclientupdated clid=7").is_err());
        assert!(
            stream_info(&parse("notifystreaminfo", "notifystreaminfo id=a clid=70000").unwrap()[0])
                .is_none()
        );
    }
    #[test]
    fn signaling_uses_the_native_answer_shape_and_validates_peer_inputs() {
        assert!(matches!(
            incoming_signal(r#"{"cmd":"answer","args":{"answer":"v=0\r\n"}}"#).unwrap(),
            Some(Input::Answer(_))
        ));
        assert!(incoming_signal(r#"{"cmd":"answer","args":{"answer":"bad"}}"#).is_err());
        assert_eq!(answer("v=0\r\n".into())["args"]["answer"], "v=0\r\n");
        assert!(answer("v=0".into())["args"].get("sdp").is_none());
        let candidate = json!({"cmd":"iceCandidate", "args":{"sdp":"candidate:1 1 udp 1 127.0.0.1 5000 typ host", "mid":"video", "mLine":1}});
        assert!(matches!(
            incoming_signal(&candidate.to_string()).unwrap(),
            Some(Input::Candidate { mline: 1, .. })
        ));
        assert!(
            incoming_signal(r#"{"cmd":"iceCandidate","args":{"sdp":"x","mLine":-1}}"#).is_err()
        );
        assert!(offer(&"x".repeat(MAX_SDP_BYTES + 1)).is_err());
        assert!(offer("not an SDP").is_err());
        assert!(incoming_signal("bad json").is_err());
    }
}
