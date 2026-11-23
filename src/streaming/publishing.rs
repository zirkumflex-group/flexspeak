use super::{Event, MAX_SHARES, Surface, Surfaces, protocol, publish_media, signals::Backlog};
use crate::{
    alerts::Source,
    audio::DeviceChoice,
    client,
    command_flow::{Flow, Operation, Ticket},
};
use anyhow::{Result, anyhow, ensure};
use publish_media::PeerKey;
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tsclientlib::{ClientId, CommandError};

pub const MAX_VIEWERS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareSource {
    pub kind: SourceKind,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub enum SourceKind {
    Screen(Option<u64>),
    Window(u64),
    Camera(String),
}
impl SourceKind {
    pub fn wire_type(&self) -> u8 {
        match self {
            Self::Camera(_) => 1,
            Self::Screen(_) => 2,
            Self::Window(_) => 3,
        }
    }
    pub fn is_camera(&self) -> bool {
        matches!(self, Self::Camera(_))
    }
}

/// The share dialog's three kinds of source, one shown at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceGroup {
    Screen,
    Window,
    Camera,
}
impl SourceGroup {
    pub const ALL: [Self; 3] = [Self::Screen, Self::Window, Self::Camera];
    pub fn of(kind: &SourceKind) -> Self {
        match kind {
            SourceKind::Screen(_) => Self::Screen,
            SourceKind::Window(_) => Self::Window,
            SourceKind::Camera(_) => Self::Camera,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Screen => "Screen",
            Self::Window => "Window",
            Self::Camera => "Camera",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Public,
    #[default]
    Private,
}
impl Access {
    pub const ALL: [Self; 2] = [Self::Public, Self::Private];
    /// The word on the Privacy choice: public shares let channel members join by themselves.
    pub fn label(self) -> &'static str {
        match self {
            Self::Public => "Public",
            Self::Private => "Private",
        }
    }
    /// One line on what the choice means for viewers.
    pub fn help(self) -> &'static str {
        match self {
            Self::Public => "Anyone in the channel can watch without asking.",
            Self::Private => "Viewers ask, and you let each one in.",
        }
    }
    pub fn wire_value(self) -> u8 {
        match self {
            Self::Public => 1,
            Self::Private => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn options() -> Options {
        Options {
            source: ShareSource {
                kind: SourceKind::Screen(None),
                name: "Test screen".into(),
            },
            audio: None,
            voice_output: None,
            quality: Quality::default(),
            access: Access::default(),
            fast_capture: false,
            hardware_av1: false,
        }
    }
    fn camera() -> Options {
        Options {
            source: ShareSource {
                kind: SourceKind::Camera("camera".into()),
                name: "Camera".into(),
            },
            ..options()
        }
    }
    fn publisher() -> Publisher {
        let (events, _) = mpsc::channel(128);
        Publisher::new(
            events,
            Rc::new(RefCell::new(Flow::default())),
            Arc::new(Surfaces::default()),
        )
    }
    /// A share as `start` adds it, on a stand-in worker.
    fn add(
        publisher: &mut Publisher,
        options: Options,
    ) -> (
        u64,
        mpsc::Receiver<publish_media::Input>,
        mpsc::Sender<publish_media::Output>,
    ) {
        let (worker, commands, output, _) = publish_media::test_worker();
        publisher.next_key += 1;
        let key = publisher.next_key;
        publisher.shares.push(Share::new(key, worker, options));
        (key, commands, output)
    }
    /// One live share, key 1, with one channel member who may watch.
    fn fixture() -> (
        Publisher,
        mpsc::Receiver<publish_media::Input>,
        mpsc::Sender<publish_media::Output>,
    ) {
        let mut publisher = publisher();
        let (_, commands, output) = add(&mut publisher, options());
        publisher.shares[0].id = Some("ours".into());
        publisher.sync(ClientId(1), HashMap::from([(2, "Viewer".into())]));
        (publisher, commands, output)
    }
    fn share(publisher: &Publisher) -> &Share {
        &publisher.shares[0]
    }
    fn notify(publisher: &mut Publisher, name: &str, fields: &str) {
        let args = protocol::parse(name, &format!("{name} {fields}")).unwrap();
        publisher.notification(name, &args[0]);
    }
    #[test]
    fn dialog_starts_on_the_main_screen() {
        let source = |kind, name: &str| ShareSource {
            kind,
            name: name.into(),
        };
        let sources = [
            source(SourceKind::Screen(Some(0)), "Screen 1"),
            source(SourceKind::Screen(Some(1)), "Main screen"),
            source(SourceKind::Window(7), "Game"),
        ];
        let quick = [source(SourceKind::Screen(Some(1)), "Main screen")];
        assert_eq!(default_source(&sources, &quick), Some(sources[1].clone()));
        // Before the quick sources are known, the first screen.
        assert_eq!(default_source(&sources, &[]), Some(sources[0].clone()));
        // No screen at all: a lone source is still the obvious one.
        let camera = [source(SourceKind::Camera("c".into()), "Camera")];
        assert_eq!(default_source(&camera, &quick), Some(camera[0].clone()));
        assert_eq!(default_source(&[], &quick), None);
    }
    #[test]
    fn viewers_must_be_in_channel_and_explicitly_approved() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=other clid=2");
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=3");
        assert!(share(&publisher).requests.is_empty());
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        assert!(commands.try_recv().is_err());
        assert!(share(&publisher).viewers.is_empty());
        publisher.decide(1, 2, true).unwrap();
        let publish_media::Input::Add(key) = commands.try_recv().unwrap() else {
            panic!("Expected admitted viewer")
        };
        assert_eq!(key.client, 2);
        assert_eq!(share(&publisher).viewers.len(), 1);
        publisher.sync(ClientId(1), HashMap::new());
        assert!(share(&publisher).viewers.is_empty());
        assert!(
            matches!(commands.try_recv(),Ok(publish_media::Input::Remove(removed)) if removed == key)
        );
    }
    #[test]
    fn public_shares_admit_channel_members_and_fill_vacancies_without_approval() {
        let (mut publisher, mut commands, _output) = fixture();
        publisher.shares[0].options.access = Access::Public;
        publisher.link.members = (1..=6).map(|id| (id, format!("Viewer {id}"))).collect();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=1");
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=99");
        assert!(commands.try_recv().is_err());
        for id in 2..=6 {
            notify(
                &mut publisher,
                "notifyjoinstreamrequest",
                &format!("id=ours clid={id}"),
            );
        }
        assert_eq!(share(&publisher).viewers.len(), MAX_VIEWERS);
        assert_eq!(
            share(&publisher)
                .requests
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![6]
        );
        for _ in 0..MAX_VIEWERS {
            assert!(matches!(
                commands.try_recv(),
                Ok(publish_media::Input::Add(_))
            ));
        }
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        assert!(
            commands.try_recv().is_err(),
            "Repeated requests must not create duplicate peers"
        );
        notify(&mut publisher, "notifystreamclientleft", "id=ours clid=2");
        assert!(matches!(
            commands.try_recv(),
            Ok(publish_media::Input::Remove(_))
        ));
        assert!(
            matches!(commands.try_recv(), Ok(publish_media::Input::Add(key)) if key.client == 6)
        );
        assert_eq!(share(&publisher).viewers.len(), MAX_VIEWERS);
        assert!(share(&publisher).requests.is_empty());
    }

    #[test]
    fn quality_presets_are_valid_and_custom_settings_are_independent() {
        for preset in Preset::ALL {
            let quality = preset.quality();
            quality.validate().unwrap();
            assert_eq!(quality.preset(), Some(preset));
        }
        // Performance is light and keeps moving; Quality is the source up to 1440p and sharp.
        let performance = Preset::Performance.quality();
        assert_eq!(
            (
                performance.max_size(),
                performance.fps(),
                performance.struggle
            ),
            ((1280, 720), 30, Struggle::Smoothness)
        );
        let best = Preset::Quality.quality();
        assert_eq!(
            (best.max_size(), best.fps(), best.struggle),
            ((2560, 1440), 30, Struggle::Sharpness)
        );
        assert_eq!(Quality::default(), best, "Quality is the default");
        assert_eq!(Resolution::Source.max_size(), (7680, 4320));
        let mut quality = Quality {
            frame_rate: 24,
            resolution: Resolution::P2160,
            video_kbps: 12_000,
            audio_kbps: 256,
            struggle: Struggle::Smoothness,
        };
        assert_eq!(quality.preset(), None);
        assert_eq!(quality.max_size(), (3840, 2160));
        assert!((quality.upload_mbps() - 12.256).abs() < 1e-9);
        assert_eq!(quality.fps(), 24);
        assert_eq!(quality.bitrate(), 12_000_000);
        assert_eq!(quality.audio_bitrate(), 256_000);
        quality.validate().unwrap();
        quality.frame_rate = 0;
        assert!(quality.validate().is_err());
        quality.frame_rate = 60;
        quality.audio_kbps = 321;
        assert!(quality.validate().is_err());
        quality.audio_kbps = 128;
        quality.video_kbps = 20_001;
        assert!(quality.validate().is_err());
    }

    #[test]
    fn a_running_share_takes_audio_and_quality_changes_without_restarting() {
        let (mut publisher, mut commands, _) = fixture();
        let unchanged = publish_media::Live {
            audio: None,
            quality: Quality::default(),
        };
        publisher.configure(1, unchanged).unwrap();
        assert!(
            commands.try_recv().is_err(),
            "nothing changed, nothing sent"
        );
        let speakers = DeviceChoice {
            id: "speakers".into(),
            name: "Speakers".into(),
        };
        let live = publish_media::Live {
            audio: Some(speakers.clone()),
            quality: Preset::Performance.quality(),
        };
        publisher.configure(1, live.clone()).unwrap();
        assert!(matches!(
            commands.try_recv(),
            Ok(publish_media::Input::Configure(sent)) if sent == live
        ));
        let options = &share(&publisher).options;
        assert_eq!(options.audio, Some(speakers));
        assert_eq!(options.quality, Preset::Performance.quality());
        assert!(
            publisher
                .configure(
                    1,
                    publish_media::Live {
                        audio: None,
                        quality: Quality {
                            frame_rate: 0,
                            ..Quality::default()
                        },
                    }
                )
                .is_err(),
            "invalid quality is refused"
        );
        publisher.stop(None, "Stopped sharing");
        assert!(publisher.configure(1, live).is_err());
    }

    #[test]
    fn stale_peer_events_cannot_reopen_or_remove_a_new_attempt() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(1, 2, true).unwrap();
        let publish_media::Input::Add(first) = commands.try_recv().unwrap() else {
            panic!()
        };
        let Publisher { shares, link, .. } = &mut publisher;
        shares[0].remove_viewer(link, 2);
        commands.try_recv().unwrap();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(1, 2, true).unwrap();
        let publish_media::Input::Add(second) = commands.try_recv().unwrap() else {
            panic!()
        };
        assert_ne!(first, second);
        publisher
            .media_event(1, publish_media::Output::Offer(first, "v=0\r\n".into()))
            .unwrap();
        publisher
            .media_event(1, publish_media::Output::Connected(first))
            .unwrap();
        publisher
            .media_event(1, publish_media::Output::Left(first))
            .unwrap();
        assert_eq!(share(&publisher).viewers[&2].key, second);
        assert!(!share(&publisher).viewers[&2].ready);
        assert!(publisher.link.pending.is_empty());
    }
    fn candidate(index: usize) -> serde_json::Value {
        serde_json::json!({"cmd":"iceCandidate","args":{
            "sdp": format!("candidate:{index} 1 udp 1 192.0.2.1 {} typ host", 5000 + index),
            "mid": "0",
            "mLine": 0,
        }})
    }
    /// The commands carrying a viewer's candidate that the server has not answered.
    fn candidates_out(publisher: &Publisher) -> Vec<Ticket> {
        publisher
            .link
            .pending
            .iter()
            .filter_map(|(ticket, kind)| matches!(kind, Pending::Candidate(..)).then_some(*ticket))
            .collect()
    }
    /// The server took these commands.
    fn taken(publisher: &mut Publisher, tickets: Vec<Ticket>) {
        for ticket in tickets {
            publisher.link.flow.borrow_mut().cancel(ticket);
            assert!(publisher.result(ticket, &Ok(())));
        }
    }
    fn slow_down() -> std::result::Result<(), CommandError> {
        Err(CommandError {
            error: tsclientlib::TsError::ClientIsFlooding,
            missing_permission: None,
        })
    }
    /// One viewer, admitted, with the offer on its way.
    fn admitted(
        publisher: &mut Publisher,
        commands: &mut mpsc::Receiver<publish_media::Input>,
    ) -> PeerKey {
        notify(publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(1, 2, true).unwrap();
        let publish_media::Input::Add(key) = commands.try_recv().unwrap() else {
            panic!("Expected admitted viewer")
        };
        publisher
            .media_event(1, publish_media::Output::Offer(key, "v=0\r\n".into()))
            .unwrap();
        key
    }
    #[test]
    fn viewers_who_join_the_moment_a_share_starts_cannot_end_it() {
        let (mut publisher, mut commands, _output) = fixture();
        publisher.shares[0].options.access = Access::Public;
        publisher.link.members = (1..=4).map(|id| (id, format!("Viewer {id}"))).collect();
        // Two people who join streams by themselves ask as soon as this one appears, and
        // each connection gathers more candidates than the command queue holds.
        for id in [2, 3] {
            notify(
                &mut publisher,
                "notifyjoinstreamrequest",
                &format!("id=ours clid={id}"),
            );
        }
        let mut keys = Vec::new();
        while let Ok(publish_media::Input::Add(key)) = commands.try_recv() {
            keys.push(key);
        }
        assert_eq!(keys.len(), 2);
        for key in &keys {
            publisher
                .media_event(1, publish_media::Output::Offer(*key, "v=0\r\n".into()))
                .unwrap();
            for index in 0..40 {
                publisher
                    .media_event(1, publish_media::Output::Signal(*key, candidate(index)))
                    .unwrap();
            }
        }
        publisher.tick(true);
        assert!(publisher.active());
        assert_eq!(share(&publisher).viewers.len(), 2);
        let out = candidates_out(&publisher);
        assert_eq!(out.len(), 2, "One a viewer at a time");
        publisher.tick(true);
        assert_eq!(candidates_out(&publisher).len(), 2);
        assert!(publisher.link.flow.borrow().has_room());
        taken(&mut publisher, out);
        publisher.tick(true);
        assert_eq!(candidates_out(&publisher).len(), 2, "The next ones follow");
        // A viewer whose connection is up needs no more of them.
        publisher
            .media_event(1, publish_media::Output::Connected(keys[0]))
            .unwrap();
        let out = candidates_out(&publisher);
        taken(&mut publisher, out);
        publisher.tick(true);
        assert_eq!(candidates_out(&publisher).len(), 1);
        assert!(publisher.active());
    }
    #[test]
    fn a_full_command_queue_costs_a_viewer_their_turn_and_not_the_share() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(1, 2, true).unwrap();
        let publish_media::Input::Add(key) = commands.try_recv().unwrap() else {
            panic!("Expected admitted viewer")
        };
        while publisher
            .link
            .flow
            .borrow_mut()
            .enqueue(protocol::stop("other"), Operation::Chat)
            .is_ok()
        {}
        publisher
            .media_event(1, publish_media::Output::Offer(key, "v=0\r\n".into()))
            .unwrap();
        assert!(publisher.active());
        assert!(share(&publisher).viewers.is_empty());
        assert!(matches!(
            commands.try_recv(),
            Ok(publish_media::Input::Remove(removed)) if removed == key
        ));
    }
    #[test]
    fn a_server_asking_to_slow_down_keeps_the_share_and_the_candidate_goes_again() {
        let (mut publisher, mut commands, _output) = fixture();
        let key = admitted(&mut publisher, &mut commands);
        publisher
            .media_event(1, publish_media::Output::Signal(key, candidate(0)))
            .unwrap();
        publisher.tick(true);
        let out = candidates_out(&publisher);
        assert_eq!(out.len(), 1);
        publisher.link.flow.borrow_mut().cancel(out[0]);
        assert!(publisher.result(out[0], &slow_down()));
        assert!(publisher.active());
        assert_eq!(share(&publisher).viewers.len(), 1);
        publisher.tick(true);
        assert_eq!(candidates_out(&publisher).len(), 1, "It goes again");
        // A candidate the server never answered ends a viewer still connecting, but not
        // one who is watching already.
        publisher
            .media_event(1, publish_media::Output::Connected(key))
            .unwrap();
        let out = candidates_out(&publisher);
        assert!(publisher.timeout(out[0]));
        assert_eq!(share(&publisher).viewers.len(), 1);
        // An offer the server was too busy for costs that viewer their turn only.
        let offer = publisher
            .link
            .pending
            .iter()
            .find_map(|(ticket, kind)| matches!(kind, Pending::Peer(..)).then_some(*ticket))
            .unwrap();
        assert!(publisher.result(offer, &slow_down()));
        assert!(publisher.active());
        assert!(share(&publisher).viewers.is_empty());
    }
    #[test]
    fn denied_requests_stay_denied_and_requests_are_bounded() {
        let (mut publisher, mut commands, _output) = fixture();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        publisher.decide(1, 2, false).unwrap();
        notify(&mut publisher, "notifyjoinstreamrequest", "id=ours clid=2");
        assert!(share(&publisher).requests.is_empty());
        assert!(commands.try_recv().is_err());
        for id in 3..80 {
            publisher.link.members.insert(id, format!("Viewer {id}"));
            notify(
                &mut publisher,
                "notifyjoinstreamrequest",
                &format!("id=ours clid={id}"),
            );
        }
        assert_eq!(share(&publisher).requests.len(), 16);
        assert!(share(&publisher).viewers.is_empty());
    }
    #[test]
    fn stopping_cancels_capture_and_late_start_is_taken_down() {
        let (mut publisher, _commands, _output) = fixture();
        publisher.shares[0].id = None;
        publisher
            .media_event(
                1,
                publish_media::Output::Ready {
                    fast_capture: false,
                    hardware_av1: false,
                },
            )
            .unwrap();
        let ticket = share(&publisher).setup.unwrap();
        publisher.stop(None, "Stopped");
        assert!(!publisher.active());
        assert!(publisher.shares.is_empty());
        assert!(!publisher.link.pending.contains_key(&ticket));
        assert!(publisher.link.uncertain.is_empty(), "it was never sent");
        notify(&mut publisher, "notifystreamstarted", "id=late clid=1");
        assert!(
            publisher
                .link
                .pending
                .values()
                .any(|kind| matches!(kind, Pending::Stop))
        );
        let count = publisher.link.pending.len();
        publisher.link.own = None;
        notify(&mut publisher, "notifystreamstarted", "id=unknown");
        assert_eq!(publisher.link.pending.len(), count);
        let (worker, _, _, active) = publish_media::test_worker();
        drop(worker);
        assert!(!active.load(Ordering::Acquire));
    }
    #[test]
    fn stills_swap_channels_skip_padding_and_shrink_to_fit() {
        // Two BGRA pixels and four bytes of row padding.
        let data = [1, 2, 3, 9, 4, 5, 6, 9, 7, 7, 7, 7];
        let still = Still::from_bgra(&data, 12, 2, 1).unwrap();
        assert_eq!(still.rgba, [3, 2, 1, 255, 6, 5, 4, 255]);

        let large = vec![100_u8; 3840 * 4 * 2160];
        let still = Still::from_bgra(&large, 3840 * 4, 3840, 2160).unwrap();
        assert_eq!((still.width, still.height), (640, 360));
        assert_eq!(&still.rgba[..4], [100, 100, 100, 255]);

        assert!(Still::from_bgra(&data, 12, -2, 1).is_none());
        assert!(
            Still::from_bgra(&data, 4, 2, 1).is_none(),
            "stride too small"
        );
        assert!(
            Still::from_bgra(&data, 12, 2, 2).is_none(),
            "buffer too short"
        );
    }

    #[test]
    fn each_source_gets_one_picture_and_the_system_picker_none() {
        let mut ui = Ui::default();
        let camera = ShareSource {
            kind: SourceKind::Camera("c".into()),
            name: "Camera".into(),
        };
        let picker = ShareSource {
            kind: SourceKind::Screen(None),
            name: "Choose".into(),
        };
        assert!(!ui.begin_preview(&picker));
        assert!(ui.begin_preview(&camera));
        assert!(!ui.begin_preview(&camera), "only once");
        ui.finish_preview(&camera, None);
        assert!(matches!(
            ui.preview(&camera),
            Some(PreviewState::Unavailable)
        ));

        ui.previews.clear();
        ui.finish_preview(&camera, None);
        assert!(ui.preview(&camera).is_none(), "closed while being taken");
    }

    #[test]
    fn share_defaults_round_trip_and_only_apply_while_idle() {
        let defaults = ShareDefaults {
            quality: Preset::Performance.quality(),
            access: Access::Public,
            audio: false,
        };
        let json = serde_json::to_string(&defaults).unwrap();
        assert!(
            json.contains("\"p720\"")
                && json.contains("\"smoothness\"")
                && json.contains("\"public\""),
            "{json}"
        );
        assert_eq!(
            serde_json::from_str::<ShareDefaults>(&json).unwrap(),
            defaults
        );

        let mut ui = Ui::default();
        ui.apply(defaults);
        assert_eq!(ui.quality.preset(), Some(Preset::Performance));
        assert_eq!(ui.access, Access::Public);
        assert!(!ui.audio);

        ui.update(Status {
            key: 1,
            active: true,
            ..Status::default()
        });
        ui.apply(ShareDefaults::default());
        assert_eq!(
            ui.quality.preset(),
            Some(Preset::Quality),
            "for the next share"
        );
        ui.apply(defaults);
        ui.editing = Some(1);
        ui.apply(ShareDefaults::default());
        assert_eq!(
            ui.quality.preset(),
            Some(Preset::Performance),
            "not over the running share the dialog shows"
        );
    }

    #[test]
    fn choices_saved_with_the_old_presets_move_to_performance_or_quality() {
        let saved = |json: &str| {
            serde_json::from_str::<ShareDefaults>(json)
                .unwrap()
                .upgraded()
                .quality
                .preset()
        };
        let old = |resolution: &str, fps: u32, video: u32, audio: u32| {
            format!(
                r#"{{"quality":{{"resolution":"{resolution}","frame_rate":{fps},"video_kbps":{video},"audio_kbps":{audio}}}}}"#
            )
        };
        assert_eq!(
            saved(&old("p1080", 30, 4500, 128)),
            Some(Preset::Quality),
            "Sharp text"
        );
        for (resolution, fps, video, audio) in [
            ("p480", 30, 1200, 64),
            ("p720", 30, 2500, 128),
            ("p720", 60, 4000, 128),
        ] {
            assert_eq!(
                saved(&old(resolution, fps, video, audio)),
                Some(Preset::Performance),
                "{resolution} {fps}"
            );
        }
        // Nothing saved was the old default, Balanced: it starts at the new default.
        assert_eq!(saved("{}"), Some(Preset::Quality));
        // Adjusted values stay as they were, now kept sharp as before.
        let custom = serde_json::from_str::<ShareDefaults>(&old("p1080", 24, 6000, 128))
            .unwrap()
            .upgraded();
        assert_eq!(custom.quality.preset(), None);
        assert_eq!(custom.quality.frame_rate, 24);
        assert_eq!(custom.quality.struggle, Struggle::Sharpness);
    }

    #[test]
    fn ui_stop_bypasses_the_voice_command_queue() {
        let (mut publisher, _commands, _output) = fixture();
        let (events, mut heard) = mpsc::channel(128);
        publisher.link.events = events;
        let (_, _second, _camera_output) = add(&mut publisher, camera());
        let Publisher { shares, link, .. } = &mut publisher;
        shares[0].emit(link);
        shares[1].emit(link);
        let mut ui = Ui::default();
        while let Ok(client::Event::Streaming(Event::Publishing(status))) = heard.try_recv() {
            ui.update(status);
        }
        assert_eq!(ui.shares.len(), 2);
        ui.stop_capture(Some(1));
        assert!(share(&publisher).worker.as_ref().unwrap().stop.stopped());
        assert!(!publisher.shares[1].worker.as_ref().unwrap().stop.stopped());
        publisher.tick(false);
        assert!(publisher.active(), "the camera carries on");
        while let Ok(client::Event::Streaming(Event::Publishing(status))) = heard.try_recv() {
            ui.update(status);
        }
        assert_eq!(ui.shares.iter().map(|s| s.key).collect::<Vec<_>>(), [2]);
        assert!(ui.ended.contains("Stopped"));
    }
    #[test]
    fn unconfirmed_start_blocks_retry_and_old_timeouts_cannot_stop_new_share() {
        let (mut publisher, _commands, _output) = fixture();
        publisher.shares[0].id = None;
        publisher
            .media_event(
                1,
                publish_media::Output::Ready {
                    fast_capture: false,
                    hardware_av1: false,
                },
            )
            .unwrap();
        let ticket = share(&publisher).setup.unwrap();
        // A sent command can no longer be cancelled by removing it from the queue.
        publisher.link.flow.borrow_mut().cancel(ticket);
        publisher.timeout(ticket);
        assert!(!publisher.active());
        assert_eq!(publisher.link.uncertain, [ticket]);
        assert!(publisher.start(options()).is_err());
        publisher.reset();
        assert!(publisher.link.uncertain.is_empty());
        let old = publisher
            .link
            .send(
                protocol::setup(
                    "old",
                    &SourceKind::Screen(None),
                    false,
                    Quality::default(),
                    Access::Private,
                ),
                Pending::Setup(99),
                Operation::StreamStart,
            )
            .unwrap();
        publisher.timeout(old);
        assert!(publisher.link.uncertain.is_empty());
    }

    fn ready(publisher: &mut Publisher, key: u64) {
        publisher
            .media_event(
                key,
                publish_media::Output::Ready {
                    fast_capture: false,
                    hardware_av1: false,
                },
            )
            .unwrap();
    }

    #[test]
    fn shares_start_one_at_a_time_and_each_keeps_its_own_stream_and_viewers() {
        let mut publisher = publisher();
        let (screen, _screen_commands, _) = add(&mut publisher, options());
        let (camera, mut camera_commands, _) = add(&mut publisher, camera());
        publisher.sync(ClientId(1), HashMap::from([(2, "Viewer".into())]));
        ready(&mut publisher, camera);
        ready(&mut publisher, screen);
        let starting = |p: &Publisher| {
            p.shares
                .iter()
                .map(|s| s.setup.is_some())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            starting(&publisher),
            [false, true],
            "the screen waits until the camera's start is confirmed"
        );
        notify(&mut publisher, "notifystreamstarted", "id=cam clid=1");
        assert_eq!(publisher.shares[1].id.as_deref(), Some("cam"));
        assert_eq!(starting(&publisher), [true, false]);
        notify(&mut publisher, "notifystreamstarted", "id=cam clid=1");
        assert!(
            publisher.shares[0].id.is_none(),
            "a repeat is not the next start"
        );
        notify(&mut publisher, "notifystreamstarted", "id=scr clid=1");
        assert_eq!(publisher.shares[0].id.as_deref(), Some("scr"));
        notify(&mut publisher, "notifyjoinstreamrequest", "id=cam clid=2");
        assert!(publisher.shares[0].requests.is_empty());
        assert!(publisher.shares[1].requests.contains_key(&2));
        publisher.decide(camera, 2, true).unwrap();
        assert!(matches!(
            camera_commands.try_recv(),
            Ok(publish_media::Input::Add(key)) if key.client == 2
        ));
        assert!(
            publisher.decide(screen, 2, true).is_err(),
            "not asked there"
        );
        notify(&mut publisher, "notifystreamstopped", "id=cam");
        assert_eq!(publisher.shares.len(), 1);
        assert_eq!(publisher.shares[0].key, screen);
        assert!(publisher.active(), "the screen carries on");
        publisher.stop(Some(screen), "Stopped sharing");
        assert!(!publisher.active());
        assert!(
            publisher
                .link
                .pending
                .values()
                .any(|kind| matches!(kind, Pending::Stop)),
            "the server hears that the screen's stream ended"
        );
    }

    #[test]
    fn a_rejected_start_lets_the_next_share_ask() {
        let mut publisher = publisher();
        let (first, ..) = add(&mut publisher, options());
        let (second, ..) = add(&mut publisher, camera());
        publisher.sync(ClientId(1), HashMap::new());
        ready(&mut publisher, first);
        ready(&mut publisher, second);
        let ticket = publisher.shares[0].setup.unwrap();
        let refused = Err(CommandError {
            error: tsclientlib::TsError::PermissionsClientInsufficient,
            missing_permission: None,
        });
        assert!(publisher.result(ticket, &refused));
        assert_eq!(publisher.shares.len(), 1);
        assert_eq!(publisher.shares[0].key, second);
        assert!(publisher.shares[0].setup.is_some());
    }

    #[test]
    fn shares_are_bounded_and_a_source_runs_once() {
        let mut publisher = publisher();
        add(&mut publisher, options());
        publisher.shares[0].options.source.kind = SourceKind::Screen(Some(1));
        let same = Options {
            source: ShareSource {
                kind: SourceKind::Screen(Some(1)),
                name: "Screen".into(),
            },
            ..options()
        };
        let error = publisher.start(same).unwrap_err().to_string();
        assert!(error.contains("already sharing"), "{error}");
        add(&mut publisher, camera());
        add(&mut publisher, options());
        let error = publisher.start(options()).unwrap_err().to_string();
        assert!(error.contains("up to 3"), "{error}");
    }

    #[test]
    fn a_share_shows_its_own_picture_only_while_wanted_and_on_screen() {
        let (mut publisher, mut commands, _output) = fixture();
        assert!(!share(&publisher).preview, "a screen starts without");
        let (_, mut camera_commands, _camera_output) = add(&mut publisher, camera());
        assert!(publisher.shares[1].preview, "a camera starts with");
        publisher.set_preview(Some(1), true);
        assert!(
            share(&publisher).shown.is_none(),
            "not while the server is off screen"
        );
        publisher.tick(true);
        let slots: Vec<_> = publisher
            .shares
            .iter()
            .map(|share| share.shown.as_ref().map(|shown| shown.slot))
            .collect();
        assert_eq!(
            slots,
            [
                Some(super::super::MAX_WATCHES),
                Some(super::super::MAX_WATCHES + 1)
            ]
        );
        let Ok(publish_media::Input::Preview(Some((surface, generation)))) = commands.try_recv()
        else {
            panic!("the worker is told where to show it")
        };
        assert!(matches!(
            camera_commands.try_recv(),
            Ok(publish_media::Input::Preview(Some(_)))
        ));
        surface.publish(
            generation,
            super::super::Image {
                width: 2,
                height: 2,
                pixels: vec![0; 16],
            },
            0.0,
        );
        assert!(surface.latest().is_some());
        publisher.set_preview(Some(1), false);
        assert!(share(&publisher).shown.is_none());
        assert!(matches!(
            commands.try_recv(),
            Ok(publish_media::Input::Preview(None))
        ));
        assert!(surface.latest().is_none(), "its picture goes with it");
        publisher.tick(false);
        assert!(
            publisher.shares[1].shown.is_none(),
            "another server on screen takes the camera's off too"
        );
        assert!(publisher.shares[1].preview, "and brings it back later");
    }

    #[test]
    fn the_dialog_moves_off_a_source_that_runs_and_keeps_how_a_share_ended() {
        let source = |kind, name: &str| ShareSource {
            kind,
            name: name.into(),
        };
        let screen = source(SourceKind::Screen(Some(1)), "Screen");
        let mut ui = Ui {
            sources: vec![
                screen.clone(),
                source(SourceKind::Screen(Some(2)), "Other screen"),
                source(SourceKind::Camera("c".into()), "Camera"),
            ],
            selected: Some(screen.clone()),
            ..Ui::default()
        };
        ui.update(Status {
            key: 1,
            active: true,
            kind: Some(screen.kind.clone()),
            ..Status::default()
        });
        assert!(ui.in_use(&screen));
        ui.pick_free();
        assert_eq!(
            ui.selected,
            Some(ui.sources[2].clone()),
            "a camera beside the screen"
        );
        assert!(!ui.audio, "a camera shares no system sound");
        ui.editing = Some(1);
        ui.update(Status {
            key: 1,
            detail: "The server stopped sharing".into(),
            ..Status::default()
        });
        assert!(!ui.active());
        assert_eq!(ui.editing, None);
        assert_eq!(ui.ended, "The server stopped sharing");
    }
}
impl fmt::Display for ShareSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub source: ShareSource,
    pub audio: Option<DeviceChoice>,
    /// Where this app plays voice and sounds. When system audio is taken from the same output,
    /// the app's own sound is left out of it where the system allows.
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub voice_output: Option<DeviceChoice>,
    pub quality: Quality,
    pub access: Access,
    /// Settings > Streaming, on by default: capture screens and windows on the GPU without
    /// waiting for it (Windows only; see `streaming/wgc.rs`). Falls back to the standard
    /// capture, at the start or during the share.
    pub fast_capture: bool,
    /// Settings > Streaming: offer AV1 from the NVIDIA hardware encoder before VP8 (Windows,
    /// RTX 40 series and newer). Without such a GPU, VP8 is used as before.
    pub hardware_av1: bool,
}

/// The largest picture a share sends. A smaller source keeps its own size; nothing is
/// enlarged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    P480,
    #[default]
    P720,
    P1080,
    P1440,
    P2160,
    /// The source's own size, however large.
    Source,
}
impl Resolution {
    pub const ALL: [Self; 6] = [
        Self::P480,
        Self::P720,
        Self::P1080,
        Self::P1440,
        Self::P2160,
        Self::Source,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::P480 => "480p",
            Self::P720 => "720p",
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::P2160 => "2160p (4K)",
            Self::Source => "Source",
        }
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn max_size(self) -> (u32, u32) {
        match self {
            Self::P480 => (854, 480),
            Self::P720 => (1280, 720),
            Self::P1080 => (1920, 1080),
            Self::P1440 => (2560, 1440),
            Self::P2160 => (3840, 2160),
            // The largest picture a viewer accepts (see `rtc::present_frame`).
            Self::Source => (7680, 4320),
        }
    }
}

/// What gives way when the connection or the computer can't keep up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Struggle {
    /// Fewer frames, every one sharp: text stays readable.
    #[default]
    Sharpness,
    /// A softer picture that keeps moving.
    Smoothness,
}
impl Struggle {
    pub const ALL: [Self; 2] = [Self::Sharpness, Self::Smoothness];
    pub fn label(self) -> &'static str {
        match self {
            Self::Sharpness => "Keep it sharp",
            Self::Smoothness => "Keep it smooth",
        }
    }
}

/// Independent encoder ceilings. Presets just fill these values; changing one leaves the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Quality {
    pub resolution: Resolution,
    pub frame_rate: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    /// Saved before this existed as sharpness, which is what every share kept then.
    pub struggle: Struggle,
}
impl Default for Quality {
    fn default() -> Self {
        Preset::Quality.quality()
    }
}
impl Quality {
    /// Upload per viewer at most, in Mbit/s: video and audio. Each viewer gets their own.
    pub fn upload_mbps(self) -> f64 {
        f64::from(self.video_kbps + self.audio_kbps) / 1000.0
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn max_size(self) -> (u32, u32) {
        self.resolution.max_size()
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn fps(self) -> u32 {
        self.frame_rate
    }
    pub fn bitrate(self) -> u64 {
        u64::from(self.video_kbps) * 1000
    }
    #[cfg(any(target_os = "macos", target_os = "windows", test))]
    pub fn audio_bitrate(self) -> u64 {
        u64::from(self.audio_kbps) * 1000
    }
    pub fn validate(self) -> Result<()> {
        ensure!(
            (5..=60).contains(&self.frame_rate),
            "Frame rate must be 5–60 fps"
        );
        ensure!(
            (250..=20_000).contains(&self.video_kbps),
            "Video bitrate must be 250–20,000 kbps"
        );
        ensure!(
            (32..=320).contains(&self.audio_kbps),
            "Audio bitrate must be 32–320 kbps"
        );
        Ok(())
    }
    pub fn preset(self) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.quality() == self)
    }
}

/// The two ways to share without touching the details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// Light on the computer and the connection; stays smooth when either struggles.
    Performance,
    /// 1080p at 60 fps, for games and fast motion.
    Smooth,
    /// The source's own size up to 1440p, sharp enough for small text.
    Quality,
}
impl Preset {
    pub const ALL: [Self; 3] = [Self::Performance, Self::Smooth, Self::Quality];
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Smooth => "Smooth",
            Self::Quality => "Quality",
        }
    }
    /// One line on what the preset is for, for its tooltip.
    pub fn help(self) -> &'static str {
        match self {
            Self::Performance => "720p at 30 fps: light on your computer and connection",
            Self::Smooth => "1080p at 60 fps: fluid motion for games",
            Self::Quality => "Up to 1440p at 30 fps: sharp enough for small text",
        }
    }
    pub fn quality(self) -> Quality {
        // Generous for their size: a screen share at 30 fps is mostly text and sharp edges,
        // which VP8 needs bits for. Hardware AV1 makes the same bitrate look better still.
        let (resolution, frame_rate, video_kbps, struggle) = match self {
            Self::Performance => (Resolution::P720, 30, 3000, Struggle::Smoothness),
            // Twice the frames need nearly twice the bits to look the same.
            Self::Smooth => (Resolution::P1080, 60, 7000, Struggle::Smoothness),
            Self::Quality => (Resolution::P1440, 30, 8000, Struggle::Sharpness),
        };
        Quality {
            resolution,
            frame_rate,
            video_kbps,
            audio_kbps: 128,
            struggle,
        }
    }
}

/// The name a share is announced under. It says only what kind of share it is: a window's
/// title can hold private text (a browser tab, a document), so it never leaves this machine.
/// Viewers see who shares, and the publisher's own sidebar tile shows the source.
fn announced_name(source: &ShareSource) -> String {
    if source.kind.is_camera() {
        "Camera".into()
    } else {
        "Screen share".into()
    }
}

/// Whether a stream's name says only what kind of share it is, as older Flexspeak builds and
/// other clients announce, so the publisher's name should lead instead.
pub fn generic_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "" | "screen share" | "screenshare" | "screen" | "camera" | "stream" | "window"
    )
}

/// One share as the window knows it.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Which of your shares this is; unique on its server.
    pub key: u64,
    pub stop: Option<publish_media::Stop>,
    pub active: bool,
    pub live: bool,
    pub detail: String,
    /// Who may watch the running share. A quick share is public whatever the dialog says.
    pub access: Access,
    /// What the running share shows: a screen's, window's or camera's name.
    pub source: String,
    /// What kind of source that is, and which one.
    pub kind: Option<SourceKind>,
    /// Whether system audio goes out with it.
    pub system_audio: bool,
    pub quality: Quality,
    /// Whether you asked to see its own picture.
    pub preview: bool,
    /// The slot and generation its own picture shows on, while it does.
    pub shown: Option<(usize, u64)>,
    pub requests: Vec<(u16, String)>,
    pub viewers: Vec<(u16, String, bool)>,
    pub video_frames: u64,
    pub audio_frames: u64,
    pub audio_level: f32,
}

/// The source the share dialog starts on when none was picked: the main screen, so Go Live
/// works at once. `quick` names it (the quick share list starts with it); without that, the
/// first screen, which on a Mac is the system picker.
pub fn default_source(sources: &[ShareSource], quick: &[ShareSource]) -> Option<ShareSource> {
    let main = quick
        .iter()
        .find(|source| matches!(source.kind, SourceKind::Screen(_)));
    main.and_then(|main| sources.iter().find(|source| source.kind == main.kind))
        .or_else(|| {
            sources
                .iter()
                .find(|source| matches!(source.kind, SourceKind::Screen(_)))
        })
        .or(if sources.len() == 1 {
            sources.first()
        } else {
            None
        })
        .cloned()
}

/// What sharing starts with, kept between runs. Which source is left out: windows come and go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShareDefaults {
    pub quality: Quality,
    pub access: Access,
    pub audio: bool,
}
impl Default for ShareDefaults {
    fn default() -> Self {
        Self {
            quality: Quality::default(),
            access: Access::default(),
            audio: true,
        }
    }
}
impl ShareDefaults {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Choices saved with the four presets before Performance and Quality: Sharp text becomes
    /// Quality, the lighter three Performance, and anything adjusted stays as it was.
    pub fn upgraded(mut self) -> Self {
        let q = self.quality;
        let old = (q.resolution, q.frame_rate, q.video_kbps, q.audio_kbps);
        let preset = match old {
            (Resolution::P1080, 30, 4500, 128) => Some(Preset::Quality),
            (Resolution::P480, 30, 1200, 64)
            | (Resolution::P720, 30, 2500, 128)
            | (Resolution::P720, 60, 4000, 128) => Some(Preset::Performance),
            _ => None,
        };
        if let Some(preset) = preset.filter(|_| q.struggle == Struggle::default()) {
            self.quality = preset.quality();
        }
        self
    }
}

/// One picture of a source, small enough for the share dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Still {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows", test)),
    allow(dead_code)
)]
impl Still {
    const MAX_SIZE: (usize, usize) = (640, 360);

    /// Averages captured BGRA down to fit. `None` for a buffer that can't be this size.
    pub fn from_bgra(data: &[u8], stride: u32, width: i32, height: i32) -> Option<Self> {
        let (stride, width) = (stride as usize, usize::try_from(width).ok()?);
        let height = usize::try_from(height).ok()?;
        if !(1..=8192).contains(&width)
            || !(1..=8192).contains(&height)
            || stride < width * 4
            || data.len() < stride * height
        {
            return None;
        }
        let step = width
            .div_ceil(Self::MAX_SIZE.0)
            .max(height.div_ceil(Self::MAX_SIZE.1));
        let (out_width, out_height) = ((width / step).max(1), (height / step).max(1));
        let mut rgba = Vec::with_capacity(out_width * out_height * 4);
        for row in 0..out_height {
            for column in 0..out_width {
                let mut sum = [0_usize; 3];
                for y in row * step..((row + 1) * step).min(height) {
                    for x in column * step..((column + 1) * step).min(width) {
                        let pixel = &data[y * stride + x * 4..][..3];
                        sum.iter_mut()
                            .zip(pixel)
                            .for_each(|(s, p)| *s += *p as usize);
                    }
                }
                let count = (((row + 1) * step).min(height) - row * step)
                    * (((column + 1) * step).min(width) - column * step);
                rgba.extend([sum[2], sum[1], sum[0]].map(|s| (s / count) as u8));
                rgba.push(255);
            }
        }
        Some(Self {
            width: out_width as u32,
            height: out_height as u32,
            rgba,
        })
    }
}

pub enum PreviewState {
    Taking,
    Ready(rune::Pixels),
    Unavailable,
}

pub struct SourcePreview {
    pub source: ShareSource,
    pub state: PreviewState,
}

/// The share dialog's choices and your running shares, for the window.
pub struct Ui {
    pub sources: Vec<ShareSource>,
    pub selected: Option<ShareSource>,
    pub loading: bool,
    pub audio: bool,
    pub audio_device: Option<DeviceChoice>,
    pub quality: Quality,
    pub access: Access,
    /// Every running share, the oldest first.
    pub shares: Vec<Status>,
    /// The running share the dialog shows; `None` while it starts another.
    pub editing: Option<u64>,
    /// How the last share to end ended, for the dialog that starts the next one.
    pub ended: String,
    /// One still per source chosen since the dialog opened.
    pub previews: Vec<SourcePreview>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            sources: vec![],
            selected: None,
            loading: false,
            audio: true,
            audio_device: None,
            quality: Quality::default(),
            access: Access::default(),
            shares: Vec::new(),
            editing: None,
            ended: String::new(),
            previews: Vec::new(),
        }
    }
}
impl Ui {
    /// Whether any share runs.
    pub fn active(&self) -> bool {
        !self.shares.is_empty()
    }
    /// Whether any share is live on the server.
    pub fn live(&self) -> bool {
        self.shares.iter().any(|share| share.live)
    }
    pub fn share(&self, key: u64) -> Option<&Status> {
        self.shares.iter().find(|share| share.key == key)
    }
    /// The running share the dialog shows, if it shows one.
    pub fn editing_share(&self) -> Option<&Status> {
        self.editing.and_then(|key| self.share(key))
    }
    /// Whether no other share can start until one stops.
    pub fn full(&self) -> bool {
        self.shares.len() >= MAX_SHARES
    }
    /// Whether a running share already shows this source. The system picker asks each time,
    /// so it never counts.
    pub fn in_use(&self, source: &ShareSource) -> bool {
        source.kind != SourceKind::Screen(None)
            && self
                .shares
                .iter()
                .any(|share| share.kind.as_ref() == Some(&source.kind))
    }
    /// Takes in a share's news. A share that ended leaves only how it ended.
    pub fn update(&mut self, status: Status) {
        let index = self.shares.iter().position(|share| share.key == status.key);
        if !status.active {
            if let Some(index) = index {
                self.shares.remove(index);
            }
            if self.editing == Some(status.key) {
                self.editing = None;
            }
            if !status.detail.is_empty() {
                self.ended = status.detail;
            }
            return;
        }
        match index {
            Some(index) => self.shares[index] = status,
            None => self.shares.push(status),
        }
    }
    /// Moves the dialog off a source that already runs: to a camera when one is free, since
    /// a screen and a camera is the usual pair, then to anything free of the same kind, then
    /// to anything free at all.
    pub fn pick_free(&mut self) {
        let Some(selected) = self.selected.clone().filter(|source| self.in_use(source)) else {
            return;
        };
        let group = SourceGroup::of(&selected.kind);
        let free = |wanted: Option<SourceGroup>| {
            self.sources
                .iter()
                .find(|source| {
                    !self.in_use(source)
                        && wanted.is_none_or(|group| SourceGroup::of(&source.kind) == group)
                })
                .cloned()
        };
        let next = free(Some(SourceGroup::Camera))
            .or_else(|| free(Some(group)))
            .or_else(|| free(None));
        if next.as_ref().is_some_and(|source| source.kind.is_camera()) {
            self.audio = false;
        }
        self.selected = next;
    }
    /// Starts from what was last used. A chosen camera keeps sharing without system sound.
    pub fn apply(&mut self, defaults: ShareDefaults) {
        if self.editing_share().is_some() {
            return;
        }
        self.quality = defaults.quality;
        self.access = defaults.access;
        if !self.selected.as_ref().is_some_and(|s| s.kind.is_camera()) {
            self.audio = defaults.audio;
        }
    }

    pub fn preview(&self, source: &ShareSource) -> Option<&PreviewState> {
        let found = self.previews.iter().find(|p| &p.source == source);
        found.map(|preview| &preview.state)
    }

    /// Whether this source still needs its one picture. Marks it as being taken.
    pub fn begin_preview(&mut self, source: &ShareSource) -> bool {
        if source.kind == SourceKind::Screen(None) || self.preview(source).is_some() {
            return false;
        }
        self.previews.push(SourcePreview {
            source: source.clone(),
            state: PreviewState::Taking,
        });
        true
    }

    /// Stores a finished picture, unless the dialog closed while it was being taken.
    pub fn finish_preview(&mut self, source: &ShareSource, still: Option<Still>) {
        let Some(preview) = self.previews.iter_mut().find(|p| &p.source == source) else {
            return;
        };
        preview.state = match still {
            Some(still) => {
                let pixels = rune::Pixels::new();
                pixels.set(still.width, still.height, still.rgba);
                PreviewState::Ready(pixels)
            }
            None => PreviewState::Unavailable,
        };
    }

    /// Stops one share's capture at once, or with `None` every one's, ahead of the commands
    /// the voice connection has queued.
    pub fn stop_capture(&self, key: Option<u64>) {
        for share in &self.shares {
            if key.is_none_or(|key| key == share.key)
                && let Some(stop) = &share.stop
            {
                stop.stop();
            }
        }
    }
}

/// What a command sent for sharing was for, with the share it belongs to.
#[derive(Clone, Copy)]
enum Pending {
    Setup(u64),
    Stop,
    Peer(u64, PeerKey),
    /// One of a viewer's connection candidates.
    Candidate(u64, PeerKey),
}

struct Viewer {
    name: String,
    ready: bool,
    key: PeerKey,
    /// Connection candidates still to reach this viewer.
    candidates: Backlog,
}

/// What every share goes through: the window, the server's command queue, and who is in the
/// channel to watch.
struct Link {
    events: mpsc::Sender<client::Event>,
    flow: Rc<RefCell<Flow>>,
    pending: HashMap<Ticket, Pending>,
    members: HashMap<u16, String>,
    own: Option<u16>,
    next_peer: u64,
    /// Starts the server may still confirm although their share was stopped. The stream each
    /// brings is taken down when it shows up; until then no other share starts, so a late
    /// confirmation can't be taken for a new share's.
    uncertain: Vec<Ticket>,
}

impl Link {
    fn send(
        &mut self,
        packet: tsproto_packets::packets::OutCommand,
        kind: Pending,
        operation: Operation,
    ) -> Result<Ticket> {
        ensure!(
            self.pending.len() < 64,
            "Too many pending publishing commands"
        );
        let ticket = self.flow.borrow_mut().enqueue(packet, operation)?;
        self.pending.insert(ticket, kind);
        Ok(ticket)
    }
    fn warn(&self, detail: String) {
        let _ = self.events.try_send(client::Event::Warning {
            source: Source::StreamPublish,
            detail,
        });
    }
    /// Drops the queued commands `matches` picks, sent or not.
    fn cancel(&mut self, matches: impl Fn(&Pending) -> bool) {
        let tickets: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(ticket, kind)| matches(kind).then_some(*ticket))
            .collect();
        for ticket in tickets {
            self.pending.remove(&ticket);
            self.flow.borrow_mut().cancel(ticket);
        }
    }
}

/// A share's own picture, on a stage slot of its own (see [`super::SLOTS`]).
struct Shown {
    surface: Arc<Surface>,
    slot: usize,
    generation: u64,
}

/// One running share: its capture, its stream on the server and who watches it.
struct Share {
    /// Unique among this server's shares, so the window can tell them apart before the server
    /// names the stream.
    key: u64,
    worker: Option<publish_media::Publisher>,
    options: Options,
    id: Option<String>,
    setup: Option<Ticket>,
    /// Capture is running; the server is asked to start the stream once no other share's start
    /// is waiting for it.
    ready: bool,
    requests: BTreeMap<u16, String>,
    viewers: BTreeMap<u16, Viewer>,
    denied: HashSet<u16>,
    started: Instant,
    status: Status,
    /// Whether fast capture is the one running, once capture started.
    fast_capture: bool,
    /// Whether hardware AV1 is offered to viewers, once capture started.
    hardware_av1: bool,
    /// A change to the running share the worker was too busy to take yet.
    unsent: Option<publish_media::Live>,
    /// Whether you asked to see this share's own picture.
    preview: bool,
    /// Where it shows, while it does: only on the server the window shows.
    shown: Option<Shown>,
}

impl Share {
    fn new(key: u64, worker: publish_media::Publisher, options: Options) -> Self {
        Self {
            key,
            worker: Some(worker),
            // You can't see your camera any other way; a screen you see already.
            preview: options.source.kind.is_camera(),
            options,
            id: None,
            setup: None,
            ready: false,
            requests: BTreeMap::new(),
            viewers: BTreeMap::new(),
            denied: HashSet::new(),
            started: Instant::now(),
            status: Status {
                detail: "Starting capture; allow access if prompted…".into(),
                ..Status::default()
            },
            fast_capture: false,
            hardware_av1: false,
            unsent: None,
            shown: None,
        }
    }
    fn active(&self) -> bool {
        self.worker.is_some()
    }
    /// The server was asked to start this stream and hasn't said yet.
    fn confirming(&self) -> bool {
        self.setup.is_some() && self.id.is_none()
    }
    /// The running share in a few words: who may watch, then the capture and encoding
    /// options that were asked for and whether they run ("Public · Hardware AV1 · Fast capture").
    fn running_note(&self) -> String {
        let mut parts = vec![self.options.access.label()];
        match (self.options.hardware_av1, self.hardware_av1) {
            (false, _) => {}
            (true, true) => parts.push("Hardware AV1"),
            (true, false) => parts.push("VP8, AV1 unavailable"),
        }
        match (self.options.fast_capture, self.fast_capture) {
            (false, _) => {}
            (true, true) => parts.push("Fast capture"),
            (true, false) => parts.push("Standard capture"),
        }
        parts.join(" · ")
    }
    fn emit(&mut self, link: &Link) {
        let status = &mut self.status;
        status.key = self.key;
        status.stop = self.worker.as_ref().map(|worker| worker.stop.clone());
        status.active = self.worker.is_some();
        status.live = self.id.is_some() && self.worker.is_some();
        status.access = self.options.access;
        status.source = self.options.source.name.clone();
        status.kind = Some(self.options.source.kind.clone());
        status.system_audio = self.options.audio.is_some();
        status.quality = self.options.quality;
        status.preview = self.preview;
        status.shown = self
            .shown
            .as_ref()
            .map(|shown| (shown.slot, shown.generation));
        status.requests = self
            .requests
            .iter()
            .map(|(id, name)| (*id, name.clone()))
            .collect();
        status.viewers = self
            .viewers
            .iter()
            .map(|(id, viewer)| (*id, viewer.name.clone(), viewer.ready))
            .collect();
        let _ = link
            .events
            .try_send(client::Event::Streaming(Event::Publishing(status.clone())));
    }
    fn stop(&mut self, link: &mut Link, detail: &str) {
        self.worker = None;
        self.unsent = None;
        self.ready = false;
        self.requests.clear();
        self.viewers.clear();
        self.denied.clear();
        let key = self.key;
        link.cancel(|kind| {
            matches!(kind, Pending::Peer(owner, _) | Pending::Candidate(owner, _) if *owner == key)
        });
        if let Some(ticket) = self.setup.take() {
            if link.flow.borrow_mut().cancel(ticket) {
                link.pending.remove(&ticket);
            } else if self.id.is_none() {
                link.uncertain.push(ticket);
            }
        }
        if let Some(id) = self.id.take()
            && let Err(error) = link.send(protocol::stop(&id), Pending::Stop, Operation::StreamStop)
        {
            link.warn(format!(
                "Capture stopped, but the server could not be notified: {error}"
            ));
        }
        self.hide();
        self.status.detail = detail.into();
        self.status.audio_level = 0.0;
        self.emit(link);
    }
    fn fail(&mut self, link: &mut Link, detail: &str) {
        self.stop(link, detail);
        link.warn(detail.into());
    }
    /// Takes the share's own picture off the stage.
    fn hide(&mut self) {
        if let Some(shown) = self.shown.take() {
            shown.surface.end(shown.generation);
            let _ = self.command(publish_media::Input::Preview(None));
        }
    }
    /// Changes system audio and quality while sharing, without starting again. Source and
    /// access stay as they started.
    fn configure(&mut self, live: publish_media::Live) -> Result<()> {
        live.quality.validate()?;
        ensure!(self.worker.is_some(), "Sharing has stopped");
        if self.options.audio == live.audio && self.options.quality == live.quality {
            return Ok(());
        }
        self.options.audio = live.audio.clone();
        self.options.quality = live.quality;
        // A slider sends many changes; only the newest matters, and a busy worker gets it later.
        self.unsent = Some(live);
        self.send_configuration();
        Ok(())
    }
    fn send_configuration(&mut self) {
        let (Some(worker), Some(live)) = (&self.worker, self.unsent.take()) else {
            return;
        };
        match worker.input.try_send(publish_media::Input::Configure(live)) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(publish_media::Input::Configure(live))) => {
                self.unsent = Some(live);
            }
            Err(_) => {}
        }
    }
    fn command(&self, command: publish_media::Input) -> Result<()> {
        self.worker
            .as_ref()
            .ok_or_else(|| anyhow!("Sharing has stopped"))?
            .input
            .try_send(command)
            .map_err(|_| anyhow!("The publisher is busy or stopped"))
    }
    fn decide(&mut self, link: &mut Link, peer: u16, allow: bool) -> Result<()> {
        let name = self
            .requests
            .get(&peer)
            .cloned()
            .ok_or_else(|| anyhow!("This viewer is no longer waiting"))?;
        ensure!(
            link.members.contains_key(&peer),
            "The viewer left your channel"
        );
        let id = self
            .id
            .clone()
            .ok_or_else(|| anyhow!("Sharing has stopped"))?;
        if allow {
            ensure!(
                self.viewers.len() < MAX_VIEWERS,
                "This share already has four viewers"
            );
            link.next_peer += 1;
            let key = PeerKey {
                client: peer,
                serial: link.next_peer,
            };
            self.command(publish_media::Input::Add(key))?;
            self.viewers.insert(
                peer,
                Viewer {
                    name,
                    ready: false,
                    key,
                    candidates: Backlog::default(),
                },
            );
        } else {
            // Remember a refusal locally; repeated server requests must not reopen it.
            ensure!(
                self.denied.len() < 128,
                "Too many denied requests; restart sharing to clear them"
            );
            self.denied.insert(peer);
            link.send(
                protocol::admit(&id, peer, None)?,
                Pending::Peer(
                    self.key,
                    PeerKey {
                        client: peer,
                        serial: 0,
                    },
                ),
                Operation::StreamAdmit,
            )?;
        }
        self.requests.remove(&peer);
        self.emit(link);
        Ok(())
    }
    fn remove_viewer(&mut self, link: &mut Link, peer: u16) {
        self.requests.remove(&peer);
        if let Some(viewer) = self.viewers.remove(&peer)
            && self
                .command(publish_media::Input::Remove(viewer.key))
                .is_err()
            && self.active()
        {
            self.fail(link, "Could not stop a departing viewer; sharing stopped");
            return;
        }
        let key = self.key;
        link.cancel(|kind| {
            matches!(kind, Pending::Peer(owner, p) | Pending::Candidate(owner, p)
                if *owner == key && p.client == peer)
        });
        self.admit_waiting(link);
        self.emit(link);
    }
    /// Public shares also fill newly freed slots, so a full share never needs manual approval.
    fn admit_waiting(&mut self, link: &mut Link) {
        if self.options.access != Access::Public || !self.active() {
            return;
        }
        let available = MAX_VIEWERS.saturating_sub(self.viewers.len());
        let waiting: Vec<_> = self.requests.keys().copied().take(available).collect();
        for peer in waiting {
            if let Err(error) = self.decide(link, peer, true) {
                link.warn(format!("Could not admit viewer: {error}"));
                break;
            }
        }
    }
    /// A notification about this share's stream, by its ID.
    fn notification(&mut self, link: &mut Link, name: &str, args: &protocol::Args) {
        let peer = args.get("clid").and_then(|v| v.parse::<u16>().ok());
        match name {
            "notifystreamstopped" => {
                self.id = None;
                self.stop(link, "The server stopped sharing");
            }
            "notifystreamclientleft" => {
                if let Some(peer) = peer {
                    self.remove_viewer(link, peer);
                }
            }
            "notifyjoinstreamrequest" => {
                if let Some(peer) =
                    peer.filter(|id| link.members.contains_key(id) && Some(*id) != link.own)
                {
                    if args.get("is_remove").is_some_and(|v| v == "1") {
                        self.remove_viewer(link, peer);
                    } else if !self.denied.contains(&peer)
                        && !self.viewers.contains_key(&peer)
                        && self.requests.len() < 16
                    {
                        self.requests.insert(peer, link.members[&peer].clone());
                        self.admit_waiting(link);
                        self.emit(link);
                    }
                }
            }
            "notifystreamsignaling" => {
                if let Some(peer) =
                    peer.filter(|id| self.viewers.contains_key(id) && link.members.contains_key(id))
                {
                    let signal =
                        protocol::incoming_signal(args.get("json").map_or("", String::as_str));
                    if let (Ok(Some(protocol::Input::Answer(sdp))), Some(viewer)) =
                        (&signal, self.viewers.get_mut(&peer))
                    {
                        viewer.candidates.answered(sdp);
                    }
                    let result = signal.and_then(|signal| match signal {
                        Some(
                            input
                            @ (protocol::Input::Answer(_) | protocol::Input::Candidate { .. }),
                        ) => self
                            .command(publish_media::Input::Signal(self.viewers[&peer].key, input)),
                        _ => Ok(()),
                    });
                    if let Err(error) = result {
                        self.remove_viewer(link, peer);
                        link.warn(format!("Viewer negotiation failed: {error}"));
                    }
                }
            }
            _ => {}
        }
    }
    /// Whether a viewer's command still belongs to the viewer this share has now.
    fn current(&self, peer: PeerKey) -> bool {
        peer.serial == 0
            || self
                .viewers
                .get(&peer.client)
                .is_some_and(|v| v.key == peer)
    }
    /// Asks the server to start the stream.
    fn send_setup(&mut self, link: &mut Link) -> Result<()> {
        let packet = protocol::setup(
            &announced_name(&self.options.source),
            &self.options.source.kind,
            self.options.audio.is_some(),
            self.options.quality,
            self.options.access,
        );
        self.setup = Some(link.send(packet, Pending::Setup(self.key), Operation::StreamStart)?);
        self.status.detail = "Capture ready; waiting for the server…".into();
        self.emit(link);
        Ok(())
    }
    /// The server started the stream under `id`.
    fn started(&mut self, link: &Link, id: String) {
        self.id = Some(id);
        self.setup = None;
        self.status.detail = self.running_note();
        let _ = link
            .events
            .try_send(client::Event::Recovered(Source::StreamPublish));
        self.emit(link);
    }
    fn tick(&mut self, link: &mut Link) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.stop.stopped())
        {
            self.stop(link, "Stopped sharing");
            return;
        }
        self.send_configuration();
        if self.active() && self.id.is_none() && self.started.elapsed() > Duration::from_secs(60) {
            self.fail(
                link,
                "Screen sharing did not start. Check capture permission and try again.",
            );
            return;
        }
        for _ in 0..32 {
            let event = match self.worker.as_mut().map(|w| w.output.try_recv()) {
                Some(Ok(event)) => event,
                Some(Err(mpsc::error::TryRecvError::Disconnected)) => {
                    self.fail(link, "The screen publisher stopped");
                    break;
                }
                _ => break,
            };
            if let Err(error) = self.media_event(link, event) {
                self.fail(link, &error.to_string());
                return;
            }
        }
        self.send_candidates(link);
    }
    /// Hands each viewer's next connection candidate to the command queue, once the one
    /// before it has gone out. A full queue only makes them wait.
    fn send_candidates(&mut self, link: &mut Link) {
        let Some(id) = self.id.as_deref() else {
            return;
        };
        let now = Instant::now();
        for (client, viewer) in &mut self.viewers {
            let Some(next) = viewer.candidates.next(&link.flow.borrow(), now) else {
                continue;
            };
            let Ok(packet) = protocol::signal_to(id, *client, next.value()) else {
                continue;
            };
            match link.send(
                packet,
                Pending::Candidate(self.key, viewer.key),
                Operation::StreamCandidate,
            ) {
                Ok(ticket) => viewer.candidates.sent(ticket, next),
                Err(_) => {
                    viewer.candidates.put_back(next);
                    break;
                }
            }
        }
    }
    fn media_event(&mut self, link: &mut Link, event: publish_media::Output) -> Result<()> {
        match event {
            publish_media::Output::Ready {
                fast_capture,
                hardware_av1,
            } => {
                self.fast_capture = fast_capture;
                self.hardware_av1 = hardware_av1;
                self.ready = true;
                self.status.detail = "Capture ready; waiting for the server…".into();
                self.emit(link);
            }
            publish_media::Output::Offer(peer, sdp) => {
                if self.current(peer) {
                    let id = self
                        .id
                        .as_deref()
                        .ok_or_else(|| anyhow!("Sharing has stopped"))?;
                    let sent = protocol::admit(id, peer.client, Some(&sdp)).and_then(|packet| {
                        link.send(
                            packet,
                            Pending::Peer(self.key, peer),
                            Operation::StreamAdmit,
                        )
                    });
                    // An offer that can't go out costs that viewer their turn, never the
                    // share and the people already watching it.
                    if let Err(error) = sent {
                        let name = self
                            .viewers
                            .get(&peer.client)
                            .map_or_else(|| "a viewer".to_owned(), |viewer| viewer.name.clone());
                        self.remove_viewer(link, peer.client);
                        link.warn(format!("Could not let {name} watch: {error}"));
                    }
                }
            }
            publish_media::Output::Signal(peer, value) => {
                if let Some(viewer) = self.viewers.get_mut(&peer.client).filter(|v| v.key == peer) {
                    viewer.candidates.push(value);
                }
            }
            publish_media::Output::Connected(peer) => {
                if let Some(viewer) = self.viewers.get_mut(&peer.client).filter(|v| v.key == peer) {
                    viewer.ready = true;
                    viewer.candidates.connected();
                    let (gathered, sent) = viewer.candidates.counts();
                    tracing::info!(
                        target: "flexspeak::stream",
                        viewer = peer.client,
                        gathered,
                        sent,
                        "viewer connected"
                    );
                }
                self.emit(link);
            }
            publish_media::Output::Left(peer) => {
                if self.current(peer) {
                    self.remove_viewer(link, peer.client);
                }
            }
            publish_media::Output::Stats {
                video,
                audio,
                level,
            } => {
                self.status.video_frames = video;
                self.status.audio_frames = audio;
                self.status.audio_level = level;
                self.emit(link);
            }
            publish_media::Output::Warning(detail) => link.warn(detail),
            publish_media::Output::StandardCapture(detail) => {
                self.fast_capture = false;
                let _ = link.events.try_send(client::Event::Warning {
                    source: Source::StreamCapture,
                    detail,
                });
                self.emit(link);
            }
            publish_media::Output::Failed(error) => return Err(anyhow!(error)),
        }
        Ok(())
    }
}

/// Your shares on one server: up to [`MAX_SHARES`] at once, such as a screen and a camera,
/// each its own stream with its own viewers.
pub struct Publisher {
    link: Link,
    shares: Vec<Share>,
    next_key: u64,
    surfaces: Arc<Surfaces>,
    /// Whether this server is the one on screen; only then do shares show their own picture.
    foreground: bool,
}

impl Publisher {
    pub fn new(
        events: mpsc::Sender<client::Event>,
        flow: Rc<RefCell<Flow>>,
        surfaces: Arc<Surfaces>,
    ) -> Self {
        Self {
            link: Link {
                events,
                flow,
                pending: HashMap::new(),
                members: HashMap::new(),
                own: None,
                next_peer: 0,
                uncertain: Vec::new(),
            },
            shares: Vec::new(),
            next_key: 0,
            surfaces,
            foreground: false,
        }
    }
    pub fn active(&self) -> bool {
        self.shares.iter().any(Share::active)
    }
    fn index(&self, key: u64) -> Option<usize> {
        self.shares.iter().position(|share| share.key == key)
    }
    /// Forgets stopped shares, then asks the server for the next start and shows or hides
    /// each share's picture.
    fn settle(&mut self) {
        self.shares.retain(Share::active);
        self.send_setups();
        self.shares.retain(Share::active);
        self.update_previews();
    }
    /// One start at a time: the server says which client started a stream, not which request
    /// it answers, so the next share asks only once the one before it is confirmed.
    fn send_setups(&mut self) {
        if !self.link.uncertain.is_empty()
            || self
                .shares
                .iter()
                .any(|share| share.active() && share.confirming())
        {
            return;
        }
        while let Some(share) = self.shares.iter_mut().find(|share| {
            share.active() && share.ready && share.setup.is_none() && share.id.is_none()
        }) {
            match share.send_setup(&mut self.link) {
                Ok(()) => break,
                Err(error) => share.fail(&mut self.link, &error.to_string()),
            }
        }
    }
    /// Puts each share's own picture on a free slot while you want it and the server is on
    /// screen, and takes it off otherwise.
    fn update_previews(&mut self) {
        for index in 0..self.shares.len() {
            let share = &self.shares[index];
            let wanted = self.foreground && share.preview && share.active();
            if !wanted {
                if share.shown.is_some() {
                    let share = &mut self.shares[index];
                    share.hide();
                    share.emit(&self.link);
                }
                continue;
            }
            if share.shown.is_some() {
                continue;
            }
            let used: Vec<usize> = self
                .shares
                .iter()
                .filter_map(|share| share.shown.as_ref().map(|shown| shown.slot))
                .collect();
            let Some(slot) = (super::MAX_WATCHES..super::SLOTS).find(|slot| !used.contains(slot))
            else {
                continue;
            };
            let surface = self.surfaces.get(slot).clone();
            let generation = surface.begin();
            let share = &mut self.shares[index];
            if share
                .command(publish_media::Input::Preview(Some((
                    surface.clone(),
                    generation,
                ))))
                .is_ok()
            {
                share.shown = Some(Shown {
                    surface,
                    slot,
                    generation,
                });
                share.emit(&self.link);
            } else {
                // The worker is busy; the next tick tries again.
                surface.end(generation);
            }
        }
    }
    #[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
    /// Starts a stand-in share; it runs for as long as the returned value is held.
    pub fn pretend_sharing(&mut self) -> impl Sized + use<> {
        let (worker, commands, output, active) = publish_media::test_worker();
        self.next_key += 1;
        let options = Options {
            source: ShareSource {
                kind: SourceKind::Screen(None),
                name: "Screen".into(),
            },
            audio: None,
            voice_output: None,
            quality: Quality::default(),
            access: Access::default(),
            fast_capture: false,
            hardware_av1: false,
        };
        self.shares.push(Share::new(self.next_key, worker, options));
        (commands, output, active)
    }
    pub fn sync(&mut self, own: ClientId, members: HashMap<u16, String>) {
        self.link.own = Some(own.0);
        self.link.members = members;
        for share in &mut self.shares {
            let gone: Vec<_> = share
                .viewers
                .keys()
                .chain(share.requests.keys())
                .copied()
                .filter(|id| !self.link.members.contains_key(id))
                .collect();
            for id in gone {
                share.remove_viewer(&mut self.link, id);
            }
        }
        self.settle();
    }
    pub fn start(&mut self, options: Options) -> Result<()> {
        ensure!(
            self.shares.len() < MAX_SHARES,
            "You can share up to {MAX_SHARES} streams at once. Stop one first."
        );
        // The system picker asks what to share each time, so it may be asked again.
        ensure!(
            options.source.kind == SourceKind::Screen(None)
                || !self
                    .shares
                    .iter()
                    .any(|share| share.options.source.kind == options.source.kind),
            "You are already sharing {}",
            options.source.name
        );
        ensure!(
            self.link.uncertain.is_empty(),
            "The previous start is still unconfirmed. Reconnect before starting another share."
        );
        ensure!(
            !self.link.flow.borrow().cooling_down(Instant::now()),
            "Wait for the server cooldown before sharing"
        );
        options.quality.validate()?;
        let worker = publish_media::start(options.clone())?;
        self.next_key += 1;
        let mut share = Share::new(self.next_key, worker, options);
        share.emit(&self.link);
        self.shares.push(share);
        self.settle();
        Ok(())
    }
    /// Stops one share, or with `None` every one.
    pub fn stop(&mut self, key: Option<u64>, detail: &str) {
        for share in &mut self.shares {
            if key.is_none_or(|key| key == share.key) && share.active() {
                share.stop(&mut self.link, detail);
            }
        }
        self.settle();
    }
    pub fn reset(&mut self) {
        for mut share in std::mem::take(&mut self.shares) {
            share.worker = None;
            share.hide();
            share.status = Status::default();
            share.emit(&self.link);
        }
        for ticket in self.link.pending.keys() {
            self.link.flow.borrow_mut().cancel(*ticket);
        }
        self.link.pending.clear();
        self.link.uncertain.clear();
    }
    /// Changes one share's system audio and quality while it runs.
    pub fn configure(&mut self, key: u64, live: publish_media::Live) -> Result<()> {
        let index = self
            .index(key)
            .ok_or_else(|| anyhow!("Sharing has stopped"))?;
        let share = &mut self.shares[index];
        share.configure(live)?;
        share.emit(&self.link);
        Ok(())
    }
    pub fn decide(&mut self, key: u64, peer: u16, allow: bool) -> Result<()> {
        let index = self
            .index(key)
            .ok_or_else(|| anyhow!("Sharing has stopped"))?;
        let result = self.shares[index].decide(&mut self.link, peer, allow);
        self.settle();
        result
    }
    /// Shows or hides one share's own picture, or with `None` every one's.
    pub fn set_preview(&mut self, key: Option<u64>, on: bool) {
        for share in &mut self.shares {
            if key.is_none_or(|key| key == share.key) && share.preview != on {
                share.preview = on;
                share.emit(&self.link);
            }
        }
        self.update_previews();
    }
    pub fn notification(&mut self, name: &str, args: &protocol::Args) {
        if name == "notifystreamstarted"
            && self.link.own.is_some()
            && args.get("clid").and_then(|v| v.parse().ok()) == self.link.own
        {
            let Some(id) = args.get("id").filter(|id| {
                !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
            }) else {
                return;
            };
            if self
                .shares
                .iter()
                .any(|share| share.id.as_deref() == Some(id.as_str()))
            {
                return;
            }
            let waiting = self
                .shares
                .iter()
                .position(|share| share.active() && share.confirming());
            match waiting {
                Some(index) => self.shares[index].started(&self.link, id.clone()),
                // A stream none of the running shares asked for: a start whose share was
                // stopped before the server confirmed it.
                None => {
                    if !self.link.uncertain.is_empty() {
                        self.link.uncertain.remove(0);
                    }
                    if let Err(error) =
                        self.link
                            .send(protocol::stop(id), Pending::Stop, Operation::StreamStop)
                    {
                        self.link.warn(error.to_string());
                    }
                }
            }
            self.settle();
            return;
        }
        let Some(index) = self
            .shares
            .iter()
            .position(|share| share.id.is_some() && share.id.as_ref() == args.get("id"))
        else {
            return;
        };
        self.shares[index].notification(&mut self.link, name, args);
        self.settle();
    }
    pub fn result(
        &mut self,
        ticket: Ticket,
        result: &std::result::Result<(), CommandError>,
    ) -> bool {
        let Some(kind) = self.link.pending.remove(&ticket) else {
            return false;
        };
        let flooding = |error: &CommandError| error.error == tsclientlib::TsError::ClientIsFlooding;
        match kind {
            Pending::Stop => {
                if let Err(error) = result {
                    self.link.warn(format!(
                        "Capture stopped; the server rejected stop sharing: {}",
                        error.error
                    ));
                }
            }
            Pending::Setup(key) => match self.index(key) {
                // Its share was stopped while the server decided; a refusal means no stream.
                None => {
                    if result.is_err() {
                        self.link.uncertain.retain(|uncertain| *uncertain != ticket);
                    }
                }
                Some(index) => {
                    if let Err(error) = result {
                        let share = &mut self.shares[index];
                        share.setup = None;
                        if flooding(error) {
                            share.stop(
                                &mut self.link,
                                "Sharing stopped while the server limits requests",
                            );
                        } else {
                            share.fail(
                                &mut self.link,
                                &format!("The server rejected sharing: {}", error.error),
                            );
                        }
                    }
                }
            },
            Pending::Peer(key, peer) => {
                if let Some(index) = self.index(key)
                    && self.shares[index].current(peer)
                    && let Err(error) = result
                {
                    // The viewer loses their turn; the share and its other viewers carry
                    // on. A server asking to slow down has its own notice.
                    self.shares[index].remove_viewer(&mut self.link, peer.client);
                    if !flooding(error) {
                        self.link
                            .warn(format!("Viewer signaling rejected: {}", error.error));
                    }
                }
            }
            Pending::Candidate(key, peer) => {
                if let Some(index) = self.index(key)
                    && let Some(viewer) = self.shares[index]
                        .viewers
                        .get_mut(&peer.client)
                        .filter(|viewer| viewer.key == peer)
                {
                    // One the server was too busy for goes again after the wait.
                    let again = result.as_ref().is_err_and(flooding);
                    viewer.candidates.reply(ticket, again);
                    if let Err(error) = result
                        && !again
                        && !viewer.ready
                    {
                        self.shares[index].remove_viewer(&mut self.link, peer.client);
                        self.link
                            .warn(format!("Viewer signaling rejected: {}", error.error));
                    }
                }
            }
        }
        self.settle();
        true
    }
    pub fn timeout(&mut self, ticket: Ticket) -> bool {
        let Some(kind) = self.link.pending.remove(&ticket) else {
            return false;
        };
        match kind {
            Pending::Stop => self
                .link
                .warn("Capture stopped; the server did not confirm stop sharing".into()),
            // Stopping finds the start sent, so it stays uncertain until its stream shows up.
            Pending::Setup(key) => {
                if let Some(index) = self.index(key) {
                    self.shares[index].fail(
                        &mut self.link,
                        "The server did not confirm sharing. Reconnect before retrying.",
                    );
                }
            }
            Pending::Peer(key, peer) => {
                if let Some(index) = self.index(key)
                    && self.shares[index].current(peer)
                {
                    self.shares[index].remove_viewer(&mut self.link, peer.client);
                    self.link.warn("Viewer signaling timed out".into());
                }
            }
            Pending::Candidate(key, peer) => {
                if let Some(index) = self.index(key)
                    && let Some(viewer) = self.shares[index]
                        .viewers
                        .get_mut(&peer.client)
                        .filter(|viewer| viewer.key == peer)
                {
                    viewer.candidates.reply(ticket, false);
                    if !viewer.ready {
                        self.shares[index].remove_viewer(&mut self.link, peer.client);
                        self.link.warn("Viewer signaling timed out".into());
                    }
                }
            }
        }
        self.settle();
        true
    }
    /// `foreground`: the server is the one on screen, so shares may show their own picture.
    pub fn tick(&mut self, foreground: bool) {
        self.foreground = foreground;
        for share in &mut self.shares {
            share.tick(&mut self.link);
        }
        self.settle();
    }
    #[cfg(test)]
    fn media_event(&mut self, key: u64, event: publish_media::Output) -> Result<()> {
        let index = self.index(key).ok_or_else(|| anyhow!("No such share"))?;
        self.shares[index].media_event(&mut self.link, event)?;
        self.settle();
        Ok(())
    }
}
