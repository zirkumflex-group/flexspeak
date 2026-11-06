# Errors and server command pacing

The native Rune error dialog (rune-kit `Dialog`) replaces persistent error blocks for connection/audio failures, server command rejection, join failures, settings load/save/reload, shortcut registration, trace recording/export and streaming failures. Return presses Close and Escape dismisses it; the window behind it is inert while it is open.

## Error lifetime

- Close, Enter and Escape dismiss the current incident. The same incident stays suppressed until its operation recovers or the user explicitly retries that operation.
- There is one incident per error source, at most 15, with details bounded to 4,096 characters. Repeated reports update the incident/count instead of opening additional dialogs. Different sources wait behind the current dialog.
- Recovery clears only the corresponding source. An unrelated successful command cannot clear a flood error. A failed server update cannot be presented as acknowledged.
- Incorrect channel passwords stay inside the existing password prompt; other queued errors wait until that prompt closes.
- Covered widgets do not receive mouse, scroll, keyboard or input-method events. Their widget state is retained, restoring the previous input focus after dismissal. The error dialog's keyboard action is Close (Enter/Escape); Tab cannot move into the covered form. Audio, protocol work and global mute/deafen shortcuts continue.
- Missing/stale peer telemetry and unsupported-platform capabilities remain status/tooltip information, not repeated modal incidents.

## Audio buffer glitches

CPAL's `ErrorKind::Xrun` reports a buffer underrun/overrun while the audio stream continues. On Windows, WASAPI emits it for a capture-buffer discontinuity. Previously, either audio callback queued every notification as a device failure, causing the next audio tick to stop the voice session or local microphone test and show **Audio unavailable**.

Input and output callbacks now exclude `Xrun` from the fatal-error queue. Repeated glitches cannot fill that queue or crowd out a later device failure. Device removal, stream invalidation, permission errors and other failures still use the existing stop/error path. This prevents a recoverable glitch from disconnecting the session; it does not eliminate audio dropouts caused by device or scheduling delays. See [CPAL's error categories](https://docs.rs/cpal/0.18.2/cpal/enum.ErrorKind.html#variant.Xrun).

A regression test injects repeated glitches followed by device failures without opening audio hardware. For this fix, 59 ordinary tests and Mac/Windows cross-target Clippy with warnings denied passed. Windows hardware retesting is still required; this fix does not address the separate macOS CoreAudio initialization stall described below.

Screen-sharing capture and publishing errors use their own modal source. Stopping capture does not wait for a server acknowledgement. Publishing commands use the same scheduler; a late start notification after cancellation is taken down instead of restarting capture.

## Control commands

`src/command_flow.rs` schedules application-issued TeamSpeak control commands. Voice packets and protocol keepalives bypass it.

- At most 64 queued/in-flight commands, with eight slots reserved for controls, joining and subscription. At most eight are in flight.
- Initially at least 250 ms between control commands. Interactive commands precede stream signaling, which precedes optional statistics/discovery/publisher metadata.
- Optional producers share one slot, send at most once per second, and wait for a quiet second after other commands. Existing per-client refresh intervals also apply.
- Mute/deafen changes apply to local protection immediately after delivery to the audio worker. A 150 ms coalescing window combines rapid toggles into the latest desired state. Only one combined audio-state command is outstanding; unsent obsolete updates are cancelled.
- The UI keeps local choices and acknowledged server state separate, showing pending/unconfirmed server updates when they differ. Unmuted microphone transmission requires acknowledgement of the current unmuted/undeafened state as well as the protocol's permission check. Muting is never delayed by the scheduler.
- Acknowledgement tracking records only the operation kind. Raw commands, channel passwords, SDP and ICE data are not logged.

## Flood response

A `ClientIsFlooding` reply identifies which scheduled operation failed. It starts a 10-second cooldown, doubling to 20/40/60 seconds if recovery attempts are also rejected. Additional rejected replies from the same burst do not repeatedly extend the cooldown. The ongoing command interval also doubles after flood feedback, up to two seconds, until reconnecting.

During cooldown, optional requests pause. Local mute/deafen choices remain effective, with only their latest unsent state retained. New joins and viewing requests report the cooldown instead of accumulating delayed actions. Rejected joins and video negotiation are not automatically replayed. A flood-rejected active video attempt stops locally; use Watch again afterward.

After cooldown, only one command is allowed through as a recovery probe. Other commands wait for its acknowledgement. Only a successful new probe clears the flood incident; old replies and elapsed time alone do not. Sent commands time out after ten seconds, releasing their tracking slot without treating a late reply as recovery. Permission failures retain their own behavior rather than being mistaken for flooding.

These are application-side controls, not a claim to know the server's command weights or configured thresholds. Server permissions and anti-flood settings are unchanged. Continued rejection still produces a dismissible, deduplicated incident.

## Validation

58 tests pass, including queue admission/priority, pacing, bounded capacity, cancelled obsolete toggles, local capture safety, acknowledgement correlation, timeout/late-reply handling, cooldown/probe recovery, modal dismissal/retry/deduplication, scoped error clearing, and background keyboard/scroll shielding. Existing password, join-permission, mute/deafen and settings-preservation tests continue to pass. Mac and Windows cross-target Clippy were run with warnings denied. The live network-only probe passed with three paced acknowledgements: mute/deafen update, channel subscription and connection statistics. It stayed muted/deafened in Lobby (#1), opened no audio devices, then disconnected. Deliberate flooding is tested with an injected clock, not against the user's server.

Native UI verification covered local validation errors, Close-button/Enter dismissal, Escape release in the final clean build, and background input protection. Short dialogs shrink to their text; long details scroll. Temporary action-only diagnostics were removed. Saved settings were not changed.

Two normal-app connection attempts stalled in macOS CoreAudio input-stream initialization, before any server connection or scheduler work. The captured call stack was `AudioUnitSetProperty` → CoreAudio HAL property IPC. Its root cause remains unestablished; the final app is running disconnected. Full voice/video runtime validation of this scheduler remains limited by that audio startup stall. See [validation evidence](measurements/2026-10-27-error-modals/validation.json).

On 2026-10-28 the new Mac development build successfully opened 48 kHz input/output and connected in Lobby, remaining muted with zero voice packets while the Streaming UI was inspected. No CoreAudio fix was applied, and this does not establish the cause of the earlier startup stalls or validate a two-person voice conversation.
