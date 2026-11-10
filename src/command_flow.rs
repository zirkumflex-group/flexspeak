//! Bounded, paced TeamSpeak control commands. Voice packets bypass this scheduler.
use anyhow::{Result, ensure};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tsclientlib::{CommandError, Connection, MessageHandle, TsError, prelude::*};
use tsproto_packets::packets::OutCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticket(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Controls,
    Subscribe,
    Join,
    Statistics,
    Discovery,
    StreamInfo,
    StreamJoin,
    StreamSignal,
    StreamLeave,
    StreamStart,
    StreamStop,
    StreamAdmit,
    PublisherInfo,
    Poke,
    Chat,
    Move,
    Moderate,
    Groups,
}
impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Controls => "mute/deafen update",
            Self::Subscribe => "channel subscription",
            Self::Join => "channel join",
            Self::Statistics => "connection statistics",
            Self::Discovery => "stream discovery",
            Self::StreamInfo => "stream details",
            Self::StreamJoin => "stream viewing request",
            Self::StreamSignal => "video negotiation",
            Self::StreamLeave => "stop watching",
            Self::StreamStart => "start sharing",
            Self::StreamStop => "stop sharing",
            Self::StreamAdmit => "viewer admission",
            Self::PublisherInfo => "publisher information",
            Self::Poke => "poke",
            Self::Chat => "message",
            Self::Move => "client move",
            Self::Moderate => "kick or ban",
            Self::Groups => "group change",
        }
    }
    pub fn optional(self) -> bool {
        matches!(
            self,
            Self::Statistics | Self::Discovery | Self::PublisherInfo
        )
    }
    /// Whether Flexspeak sends this again by itself after the server refuses it, so a refusal
    /// needs no one's attention.
    pub fn retried(self) -> bool {
        matches!(self, Self::Controls | Self::Subscribe) || self.optional()
    }
    fn priority(self) -> u8 {
        match self {
            Self::Controls | Self::Join | Self::Subscribe | Self::StreamStop => 0,
            _ if self.optional() => 2,
            _ => 1,
        }
    }
}
struct Queued {
    ticket: Ticket,
    operation: Operation,
    command: OutCommand,
}
struct Pending {
    ticket: Ticket,
    operation: Operation,
    sent: Instant,
}
pub struct Reply {
    pub ticket: Ticket,
    pub operation: Operation,
    pub result: std::result::Result<(), CommandError>,
    pub flooded: bool,
    pub recovered: bool,
}

#[derive(Default)]
pub struct Flow {
    next: u64,
    queue: VecDeque<Queued>,
    pending: HashMap<MessageHandle, Pending>,
    last_send: Option<Instant>,
    last_optional: Option<Instant>,
    cooldown: Option<Instant>,
    backoff: u64,
    pace_ms: u64,
    probe: Option<Ticket>,
}
impl Flow {
    pub fn enqueue(&mut self, command: OutCommand, operation: Operation) -> Result<Ticket> {
        let limit = if operation.priority() == 0 { 64 } else { 56 };
        ensure!(
            self.queue.len() + self.pending.len() < limit,
            "Too many pending server commands; wait before trying again"
        );
        self.next += 1;
        let ticket = Ticket(self.next);
        self.queue.push_back(Queued {
            ticket,
            operation,
            command,
        });
        Ok(ticket)
    }
    pub fn cancel(&mut self, ticket: Ticket) -> bool {
        let before = self.queue.len();
        self.queue.retain(|item| item.ticket != ticket);
        before != self.queue.len()
    }
    pub fn cooling_down(&self, now: Instant) -> bool {
        self.cooldown.is_some_and(|until| now < until)
    }
    fn optional_due(&self, now: Instant) -> bool {
        self.last_optional
            .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(1))
            && self
                .last_send
                .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_secs(1))
    }
    pub fn optional_ready(&self, operation: Operation, now: Instant) -> bool {
        // One queued request per optional producer lets FIFO scheduling give each a turn.
        // Requiring an empty queue lets the first producer starve all the others.
        operation.optional()
            && self
                .queue
                .iter()
                .all(|item| item.operation.optional() && item.operation != operation)
            && self.pending.len() < 8
            && !self.cooling_down(now)
            && self.probe.is_none()
            && self.optional_due(now)
    }
    fn next_ready(&self, now: Instant) -> Option<usize> {
        if self.cooling_down(now)
            || self.probe.is_some()
            || self.pending.len() >= 8
            || self.last_send.is_some_and(|at| {
                now.saturating_duration_since(at) < Duration::from_millis(self.pace_ms.max(250))
            })
        {
            return None;
        }
        self.queue
            .iter()
            .enumerate()
            .filter(|(_, item)| !item.operation.optional() || self.optional_due(now))
            .min_by_key(|(_, item)| item.operation.priority())
            .map(|(index, _)| index)
    }
    pub fn flush(&mut self, connection: &mut Connection, now: Instant) -> Result<()> {
        let Some(index) = self.next_ready(now) else {
            return Ok(());
        };
        let item = self.queue.remove(index).unwrap();
        let handle = item.command.send_with_result(connection)?;
        if self.cooldown.is_some() {
            self.probe = Some(item.ticket);
        }
        self.last_send = Some(now);
        if item.operation.optional() {
            self.last_optional = Some(now);
        }
        self.pending.insert(
            handle,
            Pending {
                ticket: item.ticket,
                operation: item.operation,
                sent: now,
            },
        );
        Ok(())
    }
    fn feedback(&mut self, ticket: Ticket, flooded: bool, success: bool, now: Instant) -> bool {
        let was_probe = self.probe == Some(ticket);
        if was_probe {
            self.probe = None;
        }
        if flooded {
            if !self.cooling_down(now) {
                self.pace_ms = (self.pace_ms.max(250) * 2).min(2000);
                self.backoff = if self.backoff == 0 {
                    10
                } else {
                    (self.backoff * 2).min(60)
                };
                self.cooldown = Some(now + Duration::from_secs(self.backoff));
            }
            false
        } else if was_probe && success {
            self.cooldown = None;
            self.backoff = 0;
            true
        } else {
            false
        }
    }
    pub fn result(
        &mut self,
        handle: MessageHandle,
        result: std::result::Result<(), CommandError>,
        now: Instant,
    ) -> Option<Reply> {
        let item = self.pending.remove(&handle)?;
        let flooded = result
            .as_ref()
            .is_err_and(|error| error.error == TsError::ClientIsFlooding);
        let recovered = self.feedback(item.ticket, flooded, result.is_ok(), now);
        Some(Reply {
            ticket: item.ticket,
            operation: item.operation,
            result,
            flooded,
            recovered,
        })
    }
    pub fn expired(&mut self, now: Instant) -> Vec<(Ticket, Operation)> {
        let mut expired = Vec::new();
        self.pending.retain(|_, item| {
            let keep = now.saturating_duration_since(item.sent) < Duration::from_secs(10);
            if !keep {
                expired.push((item.ticket, item.operation));
            }
            keep
        });
        if expired
            .iter()
            .any(|(ticket, _)| Some(*ticket) == self.probe)
        {
            self.probe = None;
        }
        expired
    }
    pub fn flood_detail(&self, operation: Operation, now: Instant) -> String {
        format!(
            "The server asked Flexspeak to slow down, so the {} wasn't sent. Try again in about {} seconds.",
            operation.label(),
            self.wait_seconds(now)
        )
    }
    pub fn wait_seconds(&self, now: Instant) -> u64 {
        self.cooldown.map_or(0, |until| {
            until
                .saturating_duration_since(now)
                .as_secs()
                .saturating_add(1)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioState {
    pub muted: bool,
    pub deafened: bool,
}
pub struct Controls {
    pub desired: AudioState,
    pub confirmed: Option<AudioState>,
    pub pending: Option<(Ticket, AudioState)>,
    changed: Instant,
    blocked: bool,
}
impl Controls {
    pub fn new(now: Instant) -> Self {
        Self {
            desired: AudioState {
                muted: true,
                deafened: false,
            },
            confirmed: None,
            pending: None,
            changed: now - Duration::from_secs(1),
            blocked: false,
        }
    }
    pub fn changed(&mut self, flow: &mut Flow, now: Instant) {
        self.changed = now;
        self.blocked = false;
        if self.pending.is_some_and(|(ticket, _)| flow.cancel(ticket)) {
            self.pending = None;
        }
    }
    pub fn needs_send(&self, now: Instant) -> bool {
        self.pending.is_none()
            && !self.blocked
            && self.confirmed != Some(self.desired)
            && now.saturating_duration_since(self.changed) >= Duration::from_millis(150)
    }
    pub fn capture_allowed(&self) -> bool {
        !self.desired.muted && !self.desired.deafened && self.confirmed == Some(self.desired)
    }
    pub fn reply(&mut self, ticket: Ticket, success: bool, retry: bool) -> bool {
        let Some((pending, sent)) = self.pending else {
            return false;
        };
        if pending != ticket {
            return false;
        }
        self.pending = None;
        if success {
            self.confirmed = Some(sent);
        } else {
            self.blocked = !retry && self.desired == sent;
        }
        true
    }
    pub fn reconnect(&mut self, now: Instant) {
        self.confirmed = None;
        self.pending = None;
        self.blocked = false;
        self.changed = now - Duration::from_secs(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command() -> OutCommand {
        use tsproto_packets::packets::{Direction, Flags, PacketType};
        OutCommand::new(Direction::C2S, Flags::empty(), PacketType::Command, "test")
    }
    #[test]
    fn queued_work_is_paced_prioritized_and_reserves_space_for_controls() {
        let now = Instant::now();
        let mut flow = Flow::default();
        let info = flow.enqueue(command(), Operation::Statistics).unwrap();
        assert!(
            !flow.optional_ready(Operation::Statistics, now),
            "Do not pile up requests from the same optional producer"
        );
        assert_eq!(
            flow.next_ready(now),
            Some(0),
            "An admitted optional command must dispatch"
        );
        let control = flow.enqueue(command(), Operation::Controls).unwrap();
        assert_eq!(flow.queue[flow.next_ready(now).unwrap()].ticket, control);
        flow.last_send = Some(now);
        assert!(flow.next_ready(now + Duration::from_millis(249)).is_none());
        assert_eq!(flow.next_ready(now + Duration::from_millis(250)), Some(1));
        flow.cancel(control);
        assert!(flow.next_ready(now + Duration::from_millis(500)).is_none());
        assert_eq!(flow.next_ready(now + Duration::from_secs(1)), Some(0));
        flow.cancel(info);
        for _ in 0..56 {
            flow.enqueue(command(), Operation::StreamSignal).unwrap();
        }
        assert!(flow.enqueue(command(), Operation::Discovery).is_err());
        for _ in 0..8 {
            flow.enqueue(command(), Operation::Controls).unwrap();
        }
        assert!(flow.enqueue(command(), Operation::Controls).is_err());
    }
    #[test]
    fn only_requests_flexspeak_resends_itself_count_as_retried() {
        for operation in [
            Operation::Controls,
            Operation::Subscribe,
            Operation::Statistics,
            Operation::Discovery,
            Operation::PublisherInfo,
        ] {
            assert!(operation.retried(), "{operation:?}");
        }
        for operation in [
            Operation::Join,
            Operation::Poke,
            Operation::Chat,
            Operation::Move,
            Operation::Moderate,
            Operation::Groups,
            Operation::StreamJoin,
            Operation::StreamStart,
        ] {
            assert!(!operation.retried(), "{operation:?}");
        }
    }

    #[test]
    fn real_reply_tracking_and_expiry_never_mistake_an_old_ack_for_recovery() {
        let now = Instant::now();
        let mut flow = Flow::default();
        flow.pending.insert(
            MessageHandle(10),
            Pending {
                ticket: Ticket(1),
                operation: Operation::Controls,
                sent: now,
            },
        );
        let reply = flow
            .result(
                MessageHandle(10),
                Err(CommandError {
                    error: TsError::ClientIsFlooding,
                    missing_permission: None,
                }),
                now,
            )
            .unwrap();
        assert!(reply.flooded && !reply.recovered);
        assert_eq!(reply.operation, Operation::Controls);
        flow.pending.insert(
            MessageHandle(11),
            Pending {
                ticket: Ticket(2),
                operation: Operation::Statistics,
                sent: now,
            },
        );
        assert!(
            !flow
                .result(MessageHandle(11), Ok(()), now + Duration::from_secs(1))
                .unwrap()
                .recovered
        );
        flow.probe = Some(Ticket(3));
        flow.pending.insert(
            MessageHandle(12),
            Pending {
                ticket: Ticket(3),
                operation: Operation::Statistics,
                sent: now + Duration::from_secs(10),
            },
        );
        assert_eq!(
            flow.expired(now + Duration::from_secs(20)),
            vec![(Ticket(3), Operation::Statistics)]
        );
        assert!(flow.probe.is_none() && flow.cooldown.is_some());
        assert!(
            flow.result(MessageHandle(12), Ok(()), now + Duration::from_secs(21))
                .is_none()
        );
        assert_eq!(
            flow.pace_ms, 500,
            "Flood feedback reduces the ongoing command rate"
        );
    }
    #[test]
    fn returning_to_confirmed_state_cancels_unsent_toggles() {
        let now = Instant::now();
        let mut flow = Flow::default();
        let mut controls = Controls::new(now);
        controls.confirmed = Some(controls.desired);
        controls.desired.muted = false;
        let ticket = flow.enqueue(command(), Operation::Controls).unwrap();
        controls.pending = Some((ticket, controls.desired));
        controls.desired.muted = true;
        controls.changed(&mut flow, now);
        assert!(flow.queue.is_empty() && controls.pending.is_none());
        assert!(!controls.needs_send(now + Duration::from_secs(1)));
        assert!(!controls.capture_allowed());
    }
    #[test]
    fn cooldown_waits_for_one_new_probe_and_ignores_old_successes() {
        let now = Instant::now();
        let mut flow = Flow::default();
        assert!(!flow.feedback(Ticket(1), true, false, now));
        assert!(flow.cooling_down(now + Duration::from_secs(9)));
        flow.feedback(Ticket(2), true, false, now + Duration::from_secs(1));
        assert!(!flow.cooling_down(now + Duration::from_secs(10)));
        assert!(!flow.feedback(Ticket(3), false, true, now + Duration::from_secs(11)));
        assert!(flow.cooldown.is_some());
        flow.probe = Some(Ticket(4));
        assert!(!flow.optional_ready(Operation::Statistics, now + Duration::from_secs(12)));
        flow.feedback(Ticket(4), true, false, now + Duration::from_secs(12));
        assert!(flow.cooling_down(now + Duration::from_secs(31)));
        flow.probe = Some(Ticket(5));
        assert!(flow.feedback(Ticket(5), false, true, now + Duration::from_secs(33)));
        assert!(flow.cooldown.is_none());
    }
    #[test]
    fn toggles_coalesce_and_old_acknowledgements_cannot_unmute_newer_intent() {
        let now = Instant::now();
        let mut flow = Flow::default();
        let mut controls = Controls::new(now);
        controls.confirmed = Some(controls.desired);
        controls.desired.muted = false;
        controls.changed(&mut flow, now);
        assert!(!controls.needs_send(now));
        assert!(!controls.capture_allowed());
        controls.pending = Some((Ticket(1), controls.desired));
        controls.desired.muted = true;
        controls.changed(&mut flow, now + Duration::from_millis(50));
        controls.reply(Ticket(1), true, false);
        assert!(!controls.capture_allowed());
        assert!(controls.needs_send(now + Duration::from_millis(200)));
        controls.pending = Some((Ticket(2), controls.desired));
        controls.reply(Ticket(2), false, true);
        assert!(controls.needs_send(now + Duration::from_secs(1)));
    }
}
