//! Connection candidates waiting to go to the other end of a stream.
//!
//! Each candidate is a server command of its own, and the server takes only a few commands a
//! second. A computer with several network adapters gathers dozens for one connection, so
//! sending them as they come fills the command queue, more so when several viewers join at
//! once. They wait here instead: one per connection is in the queue at a time, the likeliest
//! to work first, and none once the connection is up.
use crate::command_flow::{Flow, Ticket};
use serde_json::Value;
use std::time::Instant;

/// The most that wait for one connection. Past it the least useful are left out.
const LIMIT: usize = 48;

/// One candidate, as the other end reads it.
pub struct Candidate {
    /// Lower goes first.
    rank: u8,
    /// The order they were gathered in, which settles equal ranks.
    order: u64,
    value: Value,
}
impl Candidate {
    pub fn value(&self) -> &Value {
        &self.value
    }
}

#[derive(Default)]
pub struct Backlog {
    waiting: Vec<Candidate>,
    /// Handed to the command queue, and not answered by the server yet.
    sent: Vec<(Ticket, Candidate)>,
    /// Media sections that share the first one's connection, so theirs are not needed.
    spare: Vec<String>,
    /// The connection is up: nothing more needs to go.
    connected: bool,
    gathered: u64,
    delivered: u64,
}

impl Backlog {
    pub fn push(&mut self, value: Value) {
        self.gathered += 1;
        let section = value["args"]["mid"].as_str().unwrap_or("");
        if self.connected || self.spare.iter().any(|spare| spare == section) {
            return;
        }
        self.waiting.push(Candidate {
            rank: rank(&value),
            order: self.gathered,
            value,
        });
        if self.waiting.len() > LIMIT
            && let Some(worst) = (0..self.waiting.len())
                .max_by_key(|index| (self.waiting[*index].rank, self.waiting[*index].order))
        {
            self.waiting.swap_remove(worst);
        }
    }

    /// The next to send, once the one before it has left the command queue and the queue
    /// has room. Give it back with [`Self::sent`] or [`Self::put_back`].
    pub fn next(&mut self, flow: &Flow, now: Instant) -> Option<Candidate> {
        if self.waiting.is_empty()
            || !flow.has_room()
            || flow.cooling_down(now)
            || self.sent.iter().any(|(ticket, _)| flow.queued(*ticket))
        {
            return None;
        }
        let best = (0..self.waiting.len())
            .min_by_key(|index| (self.waiting[*index].rank, self.waiting[*index].order))?;
        Some(self.waiting.swap_remove(best))
    }
    pub fn sent(&mut self, ticket: Ticket, candidate: Candidate) {
        self.sent.push((ticket, candidate));
    }
    pub fn put_back(&mut self, candidate: Candidate) {
        if !self.connected {
            self.waiting.push(candidate);
        }
    }

    /// The server answered the command that carried one. `again`: it did not take it, or the
    /// command never left, so it goes once more. Whether the command was one of these.
    pub fn reply(&mut self, ticket: Ticket, again: bool) -> bool {
        let Some(index) = self.sent.iter().position(|(sent, _)| *sent == ticket) else {
            return false;
        };
        let (_, candidate) = self.sent.swap_remove(index);
        if again {
            self.put_back(candidate);
        } else {
            self.delivered += 1;
        }
        true
    }

    /// The other end answered the offer. Media sections it bundles onto the first one's
    /// connection need no candidates of their own.
    pub fn answered(&mut self, sdp: &str) {
        let Some(bundle) = sdp
            .lines()
            .find_map(|line| line.trim().strip_prefix("a=group:BUNDLE "))
        else {
            return;
        };
        self.spare = bundle
            .split_ascii_whitespace()
            .skip(1)
            .map(str::to_owned)
            .collect();
        let spare = &self.spare;
        self.waiting.retain(|candidate| {
            let section = candidate.value["args"]["mid"].as_str().unwrap_or("");
            !spare.iter().any(|spare| spare == section)
        });
    }

    /// The connection is up, so the rest are not needed.
    pub fn connected(&mut self) {
        self.connected = true;
        self.waiting.clear();
    }

    /// Whether the connection is up.
    pub fn up(&self) -> bool {
        self.connected
    }

    /// How many were gathered and how many the server took, for the log.
    pub fn counts(&self) -> (u64, u64) {
        (self.gathered, self.delivered)
    }

    #[cfg(test)]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

/// Which go first: a candidate reads `candidate:<id> <part> <transport> <priority> <address>
/// <port> typ <kind> …`. The first media section comes before the others, which share its
/// connection whenever the other end bundles. UDP comes before TCP, which only helps where
/// UDP is blocked. An address seen from outside comes before a local one, which only works
/// on the same network.
fn rank(value: &Value) -> u8 {
    let args = &value["args"];
    let later = u8::from(args["mLine"].as_i64().unwrap_or(0) > 0);
    let mut fields = args["sdp"].as_str().unwrap_or("").split_ascii_whitespace();
    let transport = fields.nth(2).unwrap_or("udp");
    let kind = fields.nth(4).unwrap_or("host");
    if transport.eq_ignore_ascii_case("udp") {
        2 * later + u8::from(kind == "host")
    } else {
        4 + later
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_flow::Operation;
    use serde_json::json;
    use tsproto_packets::packets::{Direction, Flags, OutCommand, PacketType};

    fn candidate(transport: &str, kind: &str, section: i64) -> Value {
        json!({"cmd":"iceCandidate","args":{
            "sdp": format!("candidate:1 1 {transport} 1 192.0.2.1 5000 typ {kind}"),
            "mid": section.to_string(),
            "mLine": section,
        }})
    }
    fn command() -> OutCommand {
        OutCommand::new(Direction::C2S, Flags::empty(), PacketType::Command, "test")
    }
    fn described(candidate: &Candidate) -> String {
        let args = &candidate.value()["args"];
        let mut fields = args["sdp"].as_str().unwrap().split(' ');
        let transport = fields.nth(2).unwrap();
        format!("{transport} {} {}", fields.nth(4).unwrap(), args["mLine"])
    }

    #[test]
    fn the_likeliest_to_work_go_first_and_one_is_queued_at_a_time() {
        let now = Instant::now();
        let mut flow = Flow::default();
        let mut backlog = Backlog::default();
        for (transport, kind, section) in [
            ("tcp", "host", 1),
            ("udp", "host", 1),
            ("tcp", "host", 0),
            ("udp", "host", 0),
            ("udp", "srflx", 1),
            ("udp", "srflx", 0),
        ] {
            backlog.push(candidate(transport, kind, section));
        }
        let mut order = Vec::new();
        while let Some(next) = backlog.next(&flow, now) {
            order.push(described(&next));
            let ticket = flow.enqueue(command(), Operation::StreamCandidate).unwrap();
            backlog.sent(ticket, next);
            assert!(
                backlog.next(&flow, now).is_none(),
                "The one before it still waits in the queue"
            );
            assert!(flow.cancel(ticket));
        }
        assert_eq!(
            order,
            [
                "udp srflx 0",
                "udp host 0",
                "udp srflx 1",
                "udp host 1",
                "tcp host 0",
                "tcp host 1"
            ]
        );
    }

    #[test]
    fn a_refused_one_goes_again_and_a_taken_one_counts() {
        let now = Instant::now();
        let mut flow = Flow::default();
        let mut backlog = Backlog::default();
        backlog.push(candidate("udp", "host", 0));
        let next = backlog.next(&flow, now).unwrap();
        let ticket = flow.enqueue(command(), Operation::StreamCandidate).unwrap();
        backlog.sent(ticket, next);
        flow.cancel(ticket);
        assert!(backlog.reply(ticket, true));
        assert!(!backlog.reply(ticket, true), "Answered once");
        assert_eq!(backlog.counts(), (1, 0));
        let next = backlog.next(&flow, now).unwrap();
        let ticket = flow.enqueue(command(), Operation::StreamCandidate).unwrap();
        backlog.sent(ticket, next);
        assert!(backlog.reply(ticket, false));
        assert_eq!(backlog.counts(), (1, 1));
        assert_eq!(backlog.waiting(), 0);
    }

    #[test]
    fn a_full_queue_and_a_connection_that_is_up_send_nothing() {
        let now = Instant::now();
        let mut flow = Flow::default();
        let mut backlog = Backlog::default();
        backlog.push(candidate("udp", "host", 0));
        let mut filling = Vec::new();
        while flow.has_room() {
            filling.push(flow.enqueue(command(), Operation::Chat).unwrap());
        }
        assert!(backlog.next(&flow, now).is_none());
        assert!(
            flow.enqueue(command(), Operation::StreamStart).is_ok(),
            "Candidates leave room for what you ask for"
        );
        for ticket in filling {
            flow.cancel(ticket);
        }
        backlog.connected();
        assert!(backlog.next(&flow, now).is_none());
        backlog.push(candidate("udp", "srflx", 0));
        assert_eq!(backlog.waiting(), 0);
        assert_eq!(backlog.counts(), (2, 0));
    }

    #[test]
    fn bundled_sections_need_no_candidates_and_the_least_useful_are_left_out() {
        let mut backlog = Backlog::default();
        backlog.push(candidate("udp", "host", 0));
        backlog.push(candidate("udp", "host", 1));
        backlog.answered("v=0\r\na=group:BUNDLE 0 1\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n");
        assert_eq!(backlog.waiting(), 1);
        backlog.push(candidate("udp", "srflx", 1));
        assert_eq!(backlog.waiting(), 1);

        // An answer that keeps the sections apart drops nothing.
        let mut apart = Backlog::default();
        apart.push(candidate("udp", "host", 1));
        apart.answered("v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\n");
        assert_eq!(apart.waiting(), 1);

        let mut full = Backlog::default();
        for _ in 0..LIMIT {
            full.push(candidate("tcp", "host", 0));
        }
        full.push(candidate("udp", "srflx", 0));
        assert_eq!(full.waiting(), LIMIT);
        let flow = Flow::default();
        let first = full.next(&flow, Instant::now()).unwrap();
        assert_eq!(described(&first), "udp srflx 0");
    }
}
