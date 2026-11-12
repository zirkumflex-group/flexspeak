# Streaming notification hook

This is the `tsclientlib` crate from ReSpeak/tsclientlib revision
`ee3bc6f45a7137db7793ba5593a321df400d53e5`. Its protocol dependencies remain
pinned to that revision. Original MIT/Apache notices are included.

Local changes:

- Replace sibling path dependencies with the original pinned Git dependencies.
- Newer support crates than upstream pins: base64 0.23 and itertools 0.15.
- `tsproto` is the patched copy in `vendor/tsproto`, from the same revision; see its
  `FLEXSPEAK-PATCH.md`.
- Expose TS6 stream notifications and client streaming-flag updates through
  `StreamItem::StreamCommand` and the synchronous equivalent. UTF-8 payloads are
  capped at 128 KiB. Ordinary client updates still pass through bookkeeping.
- Streaming discovery reads the raw optional flag, avoiding changes to generated
  protocol declarations or unrelated handshake/crypto behavior.
- Keep the stable upstream formatting options and allow five pre-existing Clippy
  lint categories only in this vendored crate; application lints remain strict.

The extension approach follows the MIT/Apache-licensed
[Moepchi unknown-command hook](https://github.com/Moepchi/tsclientlib/commit/e9dd82e8035bb6a5248b2a8dac01cfa2e8726cef).
Only this narrow hook is adapted; the remainder of that fork is not imported.
