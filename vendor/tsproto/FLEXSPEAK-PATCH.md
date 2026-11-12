# Incoming loss count

This is the `tsproto` crate from ReSpeak/tsclientlib revision
`ee3bc6f45a7137db7793ba5593a321df400d53e5`. Its sibling crates `tsproto-packets` and
`tsproto-types` remain pinned to that revision. Original MIT/Apache notices are included.

Local changes:

- Replace sibling path dependencies with the original pinned Git dependencies, and leave out
  the benches and examples with their dependencies.
- `src/packet_codec.rs`: a voice packet or an ack that arrives late no longer moves the next
  expected packet id back. Upstream accepts such a packet, which is right, but then expects
  its successor again. That successor had already arrived, so the packet after it looked like
  one that skipped an id. Each pair of packets that swapped on the way was counted as one
  lost packet: a music bot whose packets swapped about 1.5 times a second read as 2.5%
  incoming loss, with spikes to 14%, while every packet arrived.
- `src/lib.rs`: allow five pre-existing Clippy lint categories in this vendored crate only, and
  apply the stable formatting options; application lints remain strict.
- `src/resend.rs`: a test-only reader for the incoming voice counters.
- `src/client.rs`: tests for voice in order, swapped pairs and a packet that is missing.

The tests are not part of the app's CI. Run them here with `cargo test --lib`.
