use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::prelude::*;
use num_traits::ToPrimitive;
use tokio::io::ReadBuf;
use tokio::net::UdpSocket;
use tracing::{info_span, Span};
use tsproto_packets::packets::*;
use tsproto_types::crypto::EccKeyPubP256;

use crate::packet_codec::PacketCodec;
use crate::resend::{PacketId, PartialPacketId, Resender, ResenderState};
use crate::{Error, Result, MAX_UDP_PACKET_LENGTH, UDP_SINK_CAPACITY};

/// The needed functions, this can be used to abstract from the underlying
/// transport and allows simulation.
pub trait Socket {
	fn poll_recv_from(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<io::Result<SocketAddr>>;
	fn poll_send_to(
		&self,
		cx: &mut Context,
		buf: &[u8],
		target: SocketAddr,
	) -> Poll<io::Result<usize>>;
	fn local_addr(&self) -> io::Result<SocketAddr>;
}

/// A cache for the key and nonce for a generation id.
/// This has to be stored for each packet type.
#[derive(Debug)]
pub struct CachedKey {
	pub generation_id: u32,
	pub key: [u8; 16],
	pub nonce: [u8; 16],
}

/// Data that has to be stored for a connection when it is connected.
pub struct ConnectedParams {
	/// The client id of this connection.
	pub c_id: u16,
	/// If voice packets should be encrypted
	pub voice_encryption: bool,

	/// The public key of the other side.
	pub public_key: EccKeyPubP256,
	/// The iv used to encrypt and decrypt packets.
	pub shared_iv: [u8; 64],
	/// The mac used for unencrypted packets.
	pub shared_mac: [u8; 8],
	/// Cached key and nonce per packet type and for server to client (without
	/// client id inside the packet) and client to server communication.
	pub key_cache: [[CachedKey; 2]; 8],
}

/// An event that originates from a tsproto raw connection.
#[derive(Debug)]
pub enum Event<'a> {
	ReceiveUdpPacket(&'a InUdpPacket<'a>),
	ReceivePacket(&'a InPacket<'a>),
	SendUdpPacket(&'a OutUdpPacket),
	SendPacket(&'a OutPacket),
}

/// An item that originates from a tsproto raw event stream.
///
/// The disconnected event is signaled by returning `None` from the stream.
#[derive(Debug)]
pub enum StreamItem {
	Command(InCommandBuf),
	Audio(InAudioBuf),
	C2SInit(InC2SInitBuf),
	S2CInit(InS2CInitBuf),
	/// All packets with an id less or equal to this id were acknowledged.
	AckPacket(PacketId),
	/// The network statistics were updated.
	NetworkStatsUpdated,
	Error(Error),
}

type EventListener = Box<dyn for<'a> Fn(&'a Event<'a>) + Send>;

/// Represents a currently alive connection.
pub struct Connection {
	pub is_client: bool,
	pub span: Span,
	/// The parameters of this connection, if it is already established.
	pub params: Option<ConnectedParams>,
	/// The address of the other side, where packets are coming from and going
	/// to.
	pub address: SocketAddr,

	pub resender: Resender,
	pub codec: PacketCodec,
	pub udp_socket: Box<dyn Socket + Send>,
	udp_buffer: Vec<u8>,

	/// A buffer of packets that should be returned from the stream.
	///
	/// If a new udp packet is received and we already received the following
	/// ids, we can get multiple packets back at once. As we can only return one
	/// from the stream, the rest is stored here.
	pub(crate) stream_items: VecDeque<StreamItem>,

	/// The queue of non-command packets that should be sent.
	///
	/// These packets are not influenced by congestion control.
	/// If it gets too long, we don't poll from the `udp_socket` anymore.
	acks_to_send: VecDeque<OutUdpPacket>,

	pub event_listeners: Vec<EventListener>,
}

impl Socket for UdpSocket {
	fn poll_recv_from(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<io::Result<SocketAddr>> {
		self.poll_recv_from(cx, buf)
	}

	fn poll_send_to(
		&self,
		cx: &mut Context,
		buf: &[u8],
		target: SocketAddr,
	) -> Poll<io::Result<usize>> {
		self.poll_send_to(cx, buf, target)
	}

	fn local_addr(&self) -> io::Result<SocketAddr> {
		self.local_addr()
	}
}

/// How long sends may keep failing before the connection counts as broken. A shorter refusal
/// costs a few lost packets, which the resender sends again.
pub const SEND_FAILURE_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

/// At most this many refused receives are skipped in one poll, so a socket that keeps failing
/// can't spin.
const MAX_SKIPPED_RECEIVES: usize = 64;

/// Whether the network refused one packet without the socket being broken: Windows briefly out
/// of buffers or UDP ports (WSAENOBUFS, os error 10055), a route that is coming back, or, on a
/// receive, an ICMP "port unreachable" reported for an earlier packet (WSAECONNRESET).
pub fn is_transient(error: &io::Error) -> bool {
	use io::ErrorKind::*;
	if matches!(
		error.kind(),
		WouldBlock
			| Interrupted
			| NetworkUnreachable
			| HostUnreachable
			| NetworkDown
			| ConnectionReset
			| ConnectionRefused
	) {
		return true;
	}
	#[cfg(windows)]
	const NO_BUFFERS: i32 = 10055;
	#[cfg(not(windows))]
	const NO_BUFFERS: i32 = 105;
	error.raw_os_error() == Some(NO_BUFFERS)
}

/// Wraps the socket of a connection so the network refusing a packet now and then does not end
/// the connection: a refused send counts as a lost packet, a refused receive is skipped. Only
/// sends that keep failing for [`SEND_FAILURE_GRACE`] return their error.
struct Forgiving {
	inner: Box<dyn Socket + Send>,
	/// When the current run of refused sends started.
	failing_since: std::sync::Mutex<Option<std::time::Instant>>,
}

impl Forgiving {
	fn new(inner: Box<dyn Socket + Send>) -> Self {
		Self { inner, failing_since: Default::default() }
	}

	/// What to do with a send the network refused at `now`: `None` drops the packet, `Some`
	/// gives up because sends have been failing too long.
	fn refused(&self, error: io::Error, now: std::time::Instant) -> Option<io::Error> {
		let mut failing_since = self.failing_since.lock().unwrap();
		let since = *failing_since.get_or_insert_with(|| {
			tracing::warn!(%error, "Network refused a packet; counting it as lost");
			now
		});
		if now.saturating_duration_since(since) >= SEND_FAILURE_GRACE {
			Some(error)
		} else {
			tracing::trace!(%error, "Dropped a packet the network refused");
			None
		}
	}

	fn sent(&self) {
		if let Some(since) = self.failing_since.lock().unwrap().take() {
			tracing::info!(after = ?since.elapsed(), "Network accepts packets again");
		}
	}
}

impl Socket for Forgiving {
	fn poll_recv_from(&self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<io::Result<SocketAddr>> {
		for _ in 0..MAX_SKIPPED_RECEIVES {
			match self.inner.poll_recv_from(cx, buf) {
				Poll::Ready(Err(error)) if is_transient(&error) => {
					tracing::debug!(%error, "Skipped a refused receive");
				}
				result => return result,
			}
		}
		cx.waker().wake_by_ref();
		Poll::Pending
	}

	fn poll_send_to(
		&self,
		cx: &mut Context,
		buf: &[u8],
		target: SocketAddr,
	) -> Poll<io::Result<usize>> {
		match self.inner.poll_send_to(cx, buf, target) {
			Poll::Ready(Ok(size)) => {
				self.sent();
				Poll::Ready(Ok(size))
			}
			Poll::Ready(Err(error)) if is_transient(&error) => {
				match self.refused(error, std::time::Instant::now()) {
					Some(error) => Poll::Ready(Err(error)),
					None => Poll::Ready(Ok(buf.len())),
				}
			}
			other => other,
		}
	}

	fn local_addr(&self) -> io::Result<SocketAddr> {
		self.inner.local_addr()
	}
}

impl Default for CachedKey {
	fn default() -> Self {
		CachedKey { generation_id: u32::MAX, key: [0; 16], nonce: [0; 16] }
	}
}

impl ConnectedParams {
	/// Fills the parameters for a connection with their default state.
	pub fn new(public_key: EccKeyPubP256, shared_iv: [u8; 64], shared_mac: [u8; 8]) -> Self {
		Self {
			c_id: 0,
			voice_encryption: true,
			public_key,
			shared_iv,
			shared_mac,
			key_cache: Default::default(),
		}
	}
}

impl Connection {
	pub fn new(is_client: bool, address: SocketAddr, udp_socket: Box<dyn Socket + Send>) -> Self {
		let span = info_span!("connection", local_addr = %udp_socket.local_addr().unwrap(),
			remote_addr = %address);
		let udp_socket: Box<dyn Socket + Send> = Box::new(Forgiving::new(udp_socket));

		let mut res = Self {
			is_client,
			span,
			params: None,
			address,
			resender: Default::default(),
			codec: Default::default(),
			udp_socket,
			udp_buffer: Default::default(),

			stream_items: Default::default(),
			acks_to_send: Default::default(),
			event_listeners: Default::default(),
		};
		if is_client {
			// The first command is sent as part of the C2SInit::Init4 packet
			// so it does not get registered automatically.
			res.codec.outgoing_p_ids[PacketType::Command.to_usize().unwrap()] =
				PartialPacketId { generation_id: 0, packet_id: 1 };
		} else {
			res.codec.incoming_p_ids[PacketType::Command.to_usize().unwrap()] =
				PartialPacketId { generation_id: 0, packet_id: 1 };
		}
		res
	}

	/// Check if a given id is in the receive window.
	///
	/// Returns
	/// 1. If the packet id is inside the receive window
	/// 1. The generation of the packet
	/// 1. The minimum accepted packet id
	/// 1. The maximum accepted packet id
	pub(crate) fn in_receive_window(&self, p_type: PacketType, p_id: u16) -> (bool, u32, u16, u16) {
		if p_type == PacketType::Init {
			return (true, 0, 0, 0);
		}
		let type_i = p_type.to_usize().unwrap();
		// Receive window is the next half of ids
		let cur_next = self.codec.incoming_p_ids[type_i].packet_id;
		let (limit, next_gen) = cur_next.overflowing_add(u16::MAX / 2);
		let gen = self.codec.incoming_p_ids[type_i].generation_id;
		let in_recv_win = (!next_gen && p_id >= cur_next && p_id < limit)
			|| (next_gen && (p_id >= cur_next || p_id < limit));
		let gen_id = if in_recv_win {
			if next_gen && p_id < limit {
				gen + 1
			} else {
				gen
			}
		} else if p_id < cur_next {
			gen
		} else {
			gen - 1
		};

		(in_recv_win, gen_id, cur_next, limit)
	}

	pub fn send_event(&self, event: &Event) {
		for l in &self.event_listeners {
			l(event)
		}
	}

	/// Kept for callers; the receive buffer is no longer handed out, so a
	/// returned buffer is simply dropped.
	pub fn hand_back_buffer(&mut self, _buffer: Vec<u8>) {}

	fn poll_send_acks(&mut self, cx: &mut Context) -> Result<()> {
		// Poll acks_to_send
		while let Some(packet) = self.acks_to_send.front() {
			match self.poll_send_udp_packet(cx, packet) {
				Poll::Ready(Ok(())) => {
					self.resender.handle_loss_outgoing(packet);
				}
				Poll::Ready(Err(e)) => return Err(e),
				Poll::Pending => break,
			}
			self.acks_to_send.pop_front();
		}
		Ok(())
	}

	fn poll_incoming_udp_packet(&mut self, cx: &mut Context) -> Poll<Result<StreamItem>> {
		if self.acks_to_send.len() >= UDP_SINK_CAPACITY {
			return Poll::Pending;
		}

		loop {
			// Poll udp_socket
			if self.udp_buffer.len() != MAX_UDP_PACKET_LENGTH {
				self.udp_buffer.resize(MAX_UDP_PACKET_LENGTH, 0);
			}

			let mut read_buf = ReadBuf::new(&mut self.udp_buffer);
			match self.udp_socket.poll_recv_from(cx, &mut read_buf) {
				Poll::Ready(Ok(addr)) => {
					// Keep the receive buffer and hand on an exact-size copy, so a
					// packet held in a queue does not pin a whole receive buffer.
					let udp_buffer = read_buf.filled().to_vec();
					match self.handle_udp_packet(cx, udp_buffer, addr) {
						Ok(()) => {
							if let Some(item) = self.stream_items.pop_front() {
								return Poll::Ready(Ok(item));
							}
						}
						Err(e) => {
							return Poll::Ready(Err(e));
						}
					}
				}
				// Udp socket closed
				Poll::Ready(Err(e)) => return Poll::Ready(Err(Error::Network(e))),
				Poll::Pending => return Poll::Pending,
			}
		}
	}

	fn handle_udp_packet(
		&mut self,
		cx: &mut Context,
		udp_buffer: Vec<u8>,
		addr: SocketAddr,
	) -> Result<()> {
		let _span = self.span.clone().entered();
		if addr != self.address {
			self.stream_items.push_back(StreamItem::Error(Error::WrongAddress));
			return Ok(());
		}

		let dir = if self.is_client { Direction::S2C } else { Direction::C2S };
		let packet = InUdpPacket(match InPacket::try_new(dir, &udp_buffer) {
			Ok(r) => r,
			Err(e) => {
				self.stream_items.push_back(StreamItem::Error(Error::PacketParse("udp", e)));
				return Ok(());
			}
		});
		let event = Event::ReceiveUdpPacket(&packet);
		self.send_event(&event);

		self.resender.received_packet();
		PacketCodec::handle_udp_packet(self, cx, udp_buffer)?;

		Ok(())
	}

	/// Try to send an ack packet.
	///
	/// If it does not work, add it to the ack queue.
	pub(crate) fn send_ack_packet(&mut self, cx: &mut Context, packet: OutPacket) -> Result<()> {
		self.send_event(&Event::SendPacket(&packet));
		let mut udp_packets = PacketCodec::encode_packet(self, packet)?;
		assert_eq!(
			udp_packets.len(),
			1,
			"Encoding an ack packet should only yield a single packet"
		);
		let packet = udp_packets.pop().unwrap();

		match self.poll_send_udp_packet(cx, &packet) {
			Poll::Ready(r) => {
				if r.is_ok() {
					self.resender.handle_loss_outgoing(&packet);
				}
				r
			}
			Poll::Pending => {
				self.acks_to_send.push_back(packet);
				Ok(())
			}
		}
	}

	/// Add a packet to the send queue.
	///
	/// This function buffers indefinitely, to prevent using a large amount of
	/// memory, check `is_send_queue_full` first and only send a packet if this
	/// function returns `false`.
	///
	/// When the `PacketId` which is returned by this function is acknowledged,
	/// the packet was successfully received by the other side of the
	/// connection.
	pub fn send_packet(&mut self, packet: OutPacket) -> Result<PacketId> {
		self.send_event(&Event::SendPacket(&packet));
		let udp_packets = PacketCodec::encode_packet(self, packet)?;

		let id = udp_packets.last().unwrap().into();
		for p in udp_packets {
			self.send_udp_packet(p);
		}
		Ok(id)
	}

	/// Add an udp packet to the send queue.
	pub fn send_udp_packet(&mut self, packet: OutUdpPacket) {
		let _span = self.span.clone().entered();
		match packet.packet_type() {
			PacketType::Init | PacketType::Command | PacketType::CommandLow => {
				Resender::send_packet(self, packet);
			}
			_ => self.acks_to_send.push_back(packet),
		}
	}

	pub fn poll_send_udp_packet(
		&self,
		cx: &mut Context,
		packet: &OutUdpPacket,
	) -> Poll<Result<()>> {
		Self::static_poll_send_udp_packet(
			&*self.udp_socket,
			self.address,
			&self.event_listeners,
			cx,
			packet,
		)
	}

	/// Remember to add the size of the sent packet to the stats in the resender.
	pub fn static_poll_send_udp_packet(
		udp_socket: &dyn Socket,
		address: SocketAddr,
		event_listeners: &[EventListener],
		cx: &mut Context,
		packet: &OutUdpPacket,
	) -> Poll<Result<()>> {
		let data = packet.data().data();
		match udp_socket.poll_send_to(cx, data, address).map_err(Error::Network)? {
			Poll::Pending => Poll::Pending,
			Poll::Ready(size) => {
				let event = Event::SendUdpPacket(packet);
				for l in event_listeners {
					l(&event)
				}

				if size != data.len() {
					Poll::Ready(Err(Error::Network(std::io::Error::new(
						std::io::ErrorKind::Other,
						"Failed to send whole udp packet",
					))))
				} else {
					Poll::Ready(Ok(()))
				}
			}
		}
	}

	pub fn is_send_queue_full(&self) -> bool {
		self.resender.is_full()
	}
	pub fn is_send_queue_empty(&self) -> bool {
		self.resender.is_empty()
	}
}

/// Pull for events.
///
/// `Ok(StreamItem::Error)` is recoverable, `Err()` is not.
///
/// Polling does a few things in round robin fashion:
/// 1. Check for new udp packets
/// 2. Use the resender to resend packets if necessary
/// 3. Use the resender to send ping packets if necessary
impl Stream for Connection {
	type Item = Result<StreamItem>;
	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
		let _span = self.span.clone().entered();
		if let Err(e) = self.poll_send_acks(cx) {
			return Poll::Ready(Some(Err(e)));
		}

		if self.resender.get_state() == ResenderState::Disconnected {
			// Send all ack packets and return `None` afterwards
			if self.acks_to_send.is_empty() {
				return Poll::Ready(None);
			}
		}

		// Use the resender to resend packets
		match Resender::poll_resend(&mut self, cx) {
			Ok(()) => {}
			Err(e) => return Poll::Ready(Some(Err(e))),
		}

		// Use the resender to send pings
		match Resender::poll_ping(&mut self, cx) {
			Ok(()) => {}
			Err(e) => return Poll::Ready(Some(Err(e))),
		}

		// Return existing stream_items
		if let Some(item) = self.stream_items.pop_front() {
			return Poll::Ready(Some(Ok(item)));
		}

		// Check for new udp packets
		match self.poll_incoming_udp_packet(cx) {
			Poll::Ready(r) => return Poll::Ready(Some(r)),
			Poll::Pending => {}
		}

		Poll::Pending
	}
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;
	use std::time::{Duration, Instant};

	use futures::task::noop_waker_ref;

	use super::*;

	/// Answers each send with the next result queued, and each receive with the next error.
	#[derive(Default)]
	struct Scripted {
		sends: RefCell<VecDeque<io::Result<usize>>>,
		receives: RefCell<VecDeque<io::Error>>,
	}
	// Only ever used on one thread in these tests.
	unsafe impl Send for Scripted {}

	impl Socket for Scripted {
		fn poll_recv_from(&self, _: &mut Context, _: &mut ReadBuf) -> Poll<io::Result<SocketAddr>> {
			match self.receives.borrow_mut().pop_front() {
				Some(error) => Poll::Ready(Err(error)),
				None => Poll::Ready(Ok("127.0.0.1:9987".parse().unwrap())),
			}
		}
		fn poll_send_to(
			&self,
			_: &mut Context,
			_: &[u8],
			_: SocketAddr,
		) -> Poll<io::Result<usize>> {
			Poll::Ready(self.sends.borrow_mut().pop_front().unwrap_or(Ok(4)))
		}
		fn local_addr(&self) -> io::Result<SocketAddr> {
			Ok("127.0.0.1:1".parse().unwrap())
		}
	}

	fn no_buffers() -> io::Error {
		#[cfg(windows)]
		return io::Error::from_raw_os_error(10055);
		#[cfg(not(windows))]
		return io::Error::from_raw_os_error(105);
	}

	fn send(socket: &Forgiving) -> Poll<io::Result<usize>> {
		let mut cx = Context::from_waker(noop_waker_ref());
		socket.poll_send_to(&mut cx, &[0; 4], "127.0.0.1:9987".parse().unwrap())
	}

	#[test]
	fn transient_errors() {
		assert!(is_transient(&no_buffers()));
		assert!(is_transient(&io::Error::from(io::ErrorKind::ConnectionReset)));
		assert!(!is_transient(&io::Error::from(io::ErrorKind::PermissionDenied)));
		assert!(!is_transient(&io::Error::from(io::ErrorKind::InvalidInput)));
	}

	#[test]
	fn a_refused_send_counts_as_lost() {
		let inner = Scripted::default();
		inner.sends.borrow_mut().extend([Err(no_buffers()), Err(no_buffers())]);
		let socket = Forgiving::new(Box::new(inner));
		assert!(matches!(send(&socket), Poll::Ready(Ok(4))));
		assert!(matches!(send(&socket), Poll::Ready(Ok(4))));
		assert!(socket.failing_since.lock().unwrap().is_some());
		// A send that goes through ends the run.
		assert!(matches!(send(&socket), Poll::Ready(Ok(4))));
		assert!(socket.failing_since.lock().unwrap().is_none());
	}

	#[test]
	fn sends_failing_past_the_grace_end_the_connection() {
		let socket = Forgiving::new(Box::new(Scripted::default()));
		let start = Instant::now();
		assert!(socket.refused(no_buffers(), start).is_none());
		assert!(socket.refused(no_buffers(), start + Duration::from_secs(4)).is_none());
		assert!(socket.refused(no_buffers(), start + SEND_FAILURE_GRACE).is_some());
	}

	#[test]
	fn other_send_errors_pass_through() {
		let inner = Scripted::default();
		inner.sends.borrow_mut().push_back(Err(io::ErrorKind::PermissionDenied.into()));
		let socket = Forgiving::new(Box::new(inner));
		assert!(matches!(send(&socket), Poll::Ready(Err(_))));
	}

	#[test]
	fn a_refused_receive_is_skipped() {
		let inner = Scripted::default();
		inner.receives.borrow_mut().push_back(io::ErrorKind::ConnectionReset.into());
		let socket = Forgiving::new(Box::new(inner));
		let mut cx = Context::from_waker(noop_waker_ref());
		let mut data = [0; 8];
		let mut buf = ReadBuf::new(&mut data);
		assert!(matches!(socket.poll_recv_from(&mut cx, &mut buf), Poll::Ready(Ok(_))));
	}
}
