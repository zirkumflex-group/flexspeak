//! Whether a watched stream is in trouble, and whose side it is on.
//!
//! The receiver turns its WebRTC counters into a [`Media`] reading once a second (see
//! [`Tracker`]). The view then lays that beside the TeamSpeak connection health of both ends
//! ([`attribute`]): stream video goes straight between the two clients, so the server links are
//! only a proxy for each side's network, and trouble neither of them explains is put between
//! the two.
// Only the Mac and Windows receivers read stream stats; elsewhere the tracker goes unused.
#![cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]

use crate::usage::Health;
use std::time::{Duration, Instant};

/// What the picture's own counters say, already held steady.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Media {
    #[default]
    Good,
    /// The connection dropped and is trying to recover.
    Reconnecting,
    /// Nothing at all has arrived for a while, though the connection hasn't noticed yet.
    Silent,
    /// The connection is alive but no video arrives: the streamer's picture stopped.
    Paused,
    /// Video arrives with loss or freezes.
    Choppy,
    /// Video arrives whole, but this computer drops frames or decodes too slowly.
    Decoding,
}

/// Running totals from one stats report. Only differences between reports are used.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Counters {
    pub video_packets: u64,
    pub packets_lost: i64,
    pub frames_received: u64,
    pub frames_decoded: u64,
    pub frames_dropped: u64,
    pub freezes: u32,
    pub decode_seconds: f64,
    /// Everything that arrived on the connection: media, control reports and keepalives.
    pub link_packets: u64,
}

/// How long a worse reading must last before it shows.
const SHOW_AFTER: Duration = Duration::from_secs(2);
/// How long the stream must read Good again before trouble clears.
const CLEAR_AFTER: Duration = Duration::from_secs(4);
/// How long without video, with the connection still alive, before the picture counts as paused.
const PAUSED_AFTER: Duration = Duration::from_secs(3);
/// Control reports and keepalives arrive every few seconds at most.
const LINK_QUIET: Duration = Duration::from_secs(6);
/// Loss above this share of the packets in a second reads as choppy.
const CHOPPY_LOSS: f64 = 0.03;
/// Dropping more than this share of received frames reads as a decoder that can't keep up.
const DROPPED_SHARE: f64 = 0.1;

/// Holds a stream's [`Media`] reading steady from one stats report to the next.
#[derive(Debug, Clone, Default)]
pub struct Tracker {
    last: Option<Counters>,
    last_video: Option<Instant>,
    last_link: Option<Instant>,
    shown: Media,
    /// A different reading and when it began.
    pending: Option<(Media, Instant)>,
}

impl Tracker {
    /// Reads one stats report. `disconnected` is the connection's own state, which shows at once.
    pub fn update(&mut self, now: Instant, counters: Counters, disconnected: bool) -> Media {
        let previous = self.last.replace(counters);
        let reading = match previous {
            None => {
                self.last_video = Some(now);
                self.last_link = Some(now);
                Media::Good
            }
            Some(previous) => self.read(now, previous, counters),
        };
        if disconnected {
            self.pending = None;
            self.shown = Media::Reconnecting;
            return self.shown;
        }
        // A recovered connection shows what it reads straight away.
        if reading == self.shown || self.shown == Media::Reconnecting {
            self.pending = None;
            self.shown = reading;
            return self.shown;
        }
        let hold = if reading == Media::Good {
            CLEAR_AFTER
        } else {
            SHOW_AFTER
        };
        let since = match self.pending {
            Some((pending, since)) if pending == reading => since,
            _ => {
                self.pending = Some((reading, now));
                now
            }
        };
        if now.saturating_duration_since(since) >= hold {
            self.shown = reading;
            self.pending = None;
        }
        self.shown
    }

    fn read(&mut self, now: Instant, previous: Counters, counters: Counters) -> Media {
        let video = counters
            .video_packets
            .saturating_sub(previous.video_packets);
        let link = counters.link_packets.saturating_sub(previous.link_packets);
        if video > 0 {
            self.last_video = Some(now);
        }
        if link > 0 || video > 0 {
            self.last_link = Some(now);
        }
        let since =
            |at: Option<Instant>| at.map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
        if since(self.last_link) >= LINK_QUIET {
            return Media::Silent;
        }
        if since(self.last_video) >= PAUSED_AFTER {
            return Media::Paused;
        }
        let lost = (counters.packets_lost - previous.packets_lost).max(0) as f64;
        let expected = video as f64 + lost;
        if expected > 0.0 && lost / expected >= CHOPPY_LOSS {
            return Media::Choppy;
        }
        let received = counters
            .frames_received
            .saturating_sub(previous.frames_received);
        let decoded = counters
            .frames_decoded
            .saturating_sub(previous.frames_decoded);
        let dropped = counters
            .frames_dropped
            .saturating_sub(previous.frames_dropped);
        let decode_time = (counters.decode_seconds - previous.decode_seconds).max(0.0);
        // Decoding a second's frames took most of that second.
        let slow = decoded > 0 && decode_time >= 0.8;
        if slow || (received >= 5 && dropped as f64 / received as f64 >= DROPPED_SHARE) {
            return Media::Decoding;
        }
        if counters.freezes > previous.freezes {
            return Media::Choppy;
        }
        Media::Good
    }
}

/// Whose side a stream's trouble is on, as the viewer is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// Your network: your own server connection is unstable as well.
    Yours,
    /// Your computer can't decode the stream fast enough.
    YourComputer,
    /// The streamer's network: their server connection is unstable as well.
    Streamers,
    /// The connection is alive but the streamer's picture stopped.
    StreamerPaused,
    /// Neither server connection explains it.
    Between,
}

/// A stream's trouble, if any, laid beside both ends' server connections. `reconnecting` tells
/// a dropped connection from one that only struggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Problem {
    pub trouble: Trouble,
    pub reconnecting: bool,
}

/// Puts a stream's trouble on a side. `ours` and `theirs` are the server connection health of
/// this client and of the streamer.
pub fn attribute(media: Media, ours: Health, theirs: Health) -> Option<Problem> {
    let network = |reconnecting| {
        let bad = |health| matches!(health, Health::Fair | Health::Poor);
        let rank = |health| match health {
            Health::Poor => 2,
            Health::Fair => 1,
            _ => 0,
        };
        let trouble = match (bad(ours), bad(theirs)) {
            (true, false) => Trouble::Yours,
            (false, true) => Trouble::Streamers,
            // Both struggle: the worse one is the likelier cause; a tie is yours, which you can
            // do something about.
            (true, true) if rank(theirs) > rank(ours) => Trouble::Streamers,
            (true, true) => Trouble::Yours,
            (false, false) => Trouble::Between,
        };
        Problem {
            trouble,
            reconnecting,
        }
    };
    match media {
        Media::Good => None,
        Media::Decoding => Some(Problem {
            trouble: Trouble::YourComputer,
            reconnecting: false,
        }),
        Media::Paused => Some(Problem {
            trouble: Trouble::StreamerPaused,
            reconnecting: false,
        }),
        Media::Reconnecting | Media::Silent => Some(network(true)),
        Media::Choppy => Some(network(false)),
    }
}

impl Problem {
    /// A few words for the corner of the picture.
    pub fn label(self) -> &'static str {
        match (self.trouble, self.reconnecting) {
            (Trouble::StreamerPaused, _) => "Stream paused",
            (Trouble::YourComputer, _) => "Your computer",
            (Trouble::Yours, false) => "Your connection",
            (Trouble::Yours, true) => "Reconnecting · your connection",
            (Trouble::Streamers, false) => "Streamer's connection",
            (Trouble::Streamers, true) => "Reconnecting · streamer's connection",
            (Trouble::Between, false) => "Connection trouble",
            (Trouble::Between, true) => "Reconnecting",
        }
    }

    /// What it means, for the tooltip.
    pub fn explanation(self) -> &'static str {
        match self.trouble {
            Trouble::StreamerPaused => {
                "The connection is fine, but no new picture is arriving. The streamer's window may be minimised or their capture stopped."
            }
            Trouble::YourComputer => {
                "Video arrives, but this computer can't decode it fast enough, so frames are dropped. Watching fewer streams or closing heavy apps helps."
            }
            Trouble::Yours => {
                "Your own connection to the server is unstable too, so the trouble is most likely on your side."
            }
            Trouble::Streamers => {
                "The streamer's connection to the server is unstable too, so the trouble is most likely on their side."
            }
            Trouble::Between => {
                "Both connections to the server look fine, so the trouble is on the way between you and the streamer."
            }
        }
    }

    /// Whether it is serious: the picture has stopped rather than stutters.
    pub fn serious(self) -> bool {
        self.reconnecting || self.trouble == Trouble::StreamerPaused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flowing(at: u64) -> Counters {
        Counters {
            video_packets: at * 100,
            frames_received: at * 30,
            frames_decoded: at * 30,
            decode_seconds: at as f64 * 0.1,
            link_packets: at * 110,
            ..Counters::default()
        }
    }

    fn run(
        tracker: &mut Tracker,
        start: Instant,
        seconds: std::ops::Range<u64>,
        each: impl Fn(u64) -> Counters,
    ) -> Media {
        let mut shown = Media::Good;
        for second in seconds {
            shown = tracker.update(start + Duration::from_secs(second), each(second), false);
        }
        shown
    }

    #[test]
    fn steady_video_reads_good() {
        let mut tracker = Tracker::default();
        assert_eq!(
            run(&mut tracker, Instant::now(), 0..10, flowing),
            Media::Good
        );
    }

    #[test]
    fn video_stopping_on_a_live_connection_reads_as_paused_after_a_hold() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        run(&mut tracker, start, 0..5, flowing);
        // Only control reports arrive from second 5 on.
        let paused = |second: u64| Counters {
            link_packets: 500 + second,
            ..flowing(4)
        };
        assert_eq!(
            run(&mut tracker, start, 5..8, paused),
            Media::Good,
            "still within the hold"
        );
        assert_eq!(run(&mut tracker, start, 8..11, paused), Media::Paused);
        // Video comes back: trouble clears only after it has stayed good.
        assert_eq!(run(&mut tracker, start, 11..13, flowing), Media::Paused);
        assert_eq!(run(&mut tracker, start, 13..20, flowing), Media::Good);
    }

    #[test]
    fn nothing_arriving_at_all_reads_as_silent() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        run(&mut tracker, start, 0..5, flowing);
        assert_eq!(
            run(&mut tracker, start, 5..15, |_| flowing(4)),
            Media::Silent
        );
    }

    #[test]
    fn loss_freezes_and_slow_decoding_are_told_apart() {
        let start = Instant::now();
        let mut lossy = Tracker::default();
        let lost = |second: u64| Counters {
            packets_lost: second as i64 * 10,
            ..flowing(second)
        };
        assert_eq!(run(&mut lossy, start, 0..6, lost), Media::Choppy);

        let mut frozen = Tracker::default();
        let freezes = |second: u64| Counters {
            freezes: second as u32,
            ..flowing(second)
        };
        assert_eq!(run(&mut frozen, start, 0..6, freezes), Media::Choppy);

        let mut slow = Tracker::default();
        let dropping = |second: u64| Counters {
            frames_decoded: second * 20,
            frames_dropped: second * 10,
            ..flowing(second)
        };
        assert_eq!(run(&mut slow, start, 0..6, dropping), Media::Decoding);
    }

    #[test]
    fn a_single_bad_second_does_not_show() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        run(&mut tracker, start, 0..5, flowing);
        let blip = |second: u64| Counters {
            packets_lost: if second == 5 { 50 } else { 0 },
            ..flowing(second)
        };
        assert_eq!(run(&mut tracker, start, 5..12, blip), Media::Good);
    }

    #[test]
    fn a_dropped_connection_shows_at_once_and_clears_when_it_recovers() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        run(&mut tracker, start, 0..3, flowing);
        assert_eq!(
            tracker.update(start + Duration::from_secs(3), flowing(3), true),
            Media::Reconnecting
        );
        assert_eq!(
            tracker.update(start + Duration::from_secs(4), flowing(4), false),
            Media::Good
        );
    }

    #[test]
    fn trouble_goes_to_the_side_whose_server_connection_struggles() {
        use Health::*;
        let side = |media, ours, theirs| attribute(media, ours, theirs).map(|p| p.trouble);
        assert_eq!(side(Media::Good, Poor, Poor), None);
        assert_eq!(side(Media::Choppy, Poor, Good), Some(Trouble::Yours));
        assert_eq!(side(Media::Choppy, Good, Fair), Some(Trouble::Streamers));
        assert_eq!(side(Media::Choppy, Fair, Poor), Some(Trouble::Streamers));
        assert_eq!(side(Media::Choppy, Poor, Poor), Some(Trouble::Yours));
        assert_eq!(side(Media::Choppy, Good, Unknown), Some(Trouble::Between));
        assert_eq!(
            side(Media::Paused, Poor, Poor),
            Some(Trouble::StreamerPaused)
        );
        assert_eq!(
            side(Media::Decoding, Good, Poor),
            Some(Trouble::YourComputer)
        );
        let dropped = attribute(Media::Silent, Good, Good).unwrap();
        assert!(dropped.reconnecting);
        assert_eq!(dropped.label(), "Reconnecting");
    }
}
