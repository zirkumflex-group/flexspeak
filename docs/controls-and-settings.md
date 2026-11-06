# Setup, controls and saved settings

## First launch

When `settings.json` does not exist, the app opens a five-step setup: username, one saved server, OS permissions, audio devices/local testing, and hotkeys. It writes the configuration only after **Finish setup** succeeds. Closing an unfinished setup leaves no default file behind, so the next launch starts setup again. A valid existing configuration skips setup. A malformed or unreadable file is preserved and opens Settings with an error instead of being replaced by onboarding.

Microphone and playback selections are saved by device ID. If a saved device is unavailable on a later launch, the picker remains unselected so the user can explicitly choose a replacement. Older files without device IDs use the detected defaults. The local microphone test makes no server connection; stop it before advancing setup. Passwords remain session-only.

The Permissions step, and the same section in Settings, shows what the operating system currently allows for microphone, screen sharing and system audio: **Allowed**, **Not allowed**, **Not allowed yet** (never asked), **Not needed** or **Unknown**. Reading the status never prompts. It is re-read while the section is visible, so a change made in the OS privacy settings shows up on return. **Give Access** appears only when access is missing. If the user has never been asked, it shows the system prompt. If access was denied, it opens the matching privacy pane, because the OS will not ask again. Mac reads the microphone from `AVAudioApplication.recordPermission`, screen recording from `CGPreflightScreenCaptureAccess`, and system audio (which has no public status call) from TCC's `TCCAccessPreflight`, loaded at runtime with an **Unknown** fallback. Requests use the native audio permission request, the Core Graphics screen-access request, and a brief output-loopback check whose callback discards samples. macOS may need the app reopened before a new screen-recording grant takes effect. Windows reads the microphone privacy switches (device, apps and desktop apps) from the CapabilityAccessManager consent store in the registry; desktop screen capture and output loopback need no grant and show **Not needed**, though system policy can still restrict them. Permission denial can be handled later in Settings and does not trap the user in setup. Camera and accessibility permissions are not requested for these features.

## Server view and window bounds

Normal windows have a **760 × 560 logical-pixel minimum**. Performance mode keeps its **440 × 320** minimum, and leaving it restores the normal minimum before resizing back.

The top bar contains performance mode, Statistics and Settings; Voice and Streaming tabs are removed. The disconnected home screen centers one saved-server card with an icon-only Connect button and tooltip. Edit the saved server, username or session-only server password in Settings. Unsaved address/name drafts do not change the server shown on the card or used by Connect.

Once connected, the left sidebar contains the server/channel tree and Disconnect, Mute and Deafen. The channel tree reserves 16 logical pixels before its scrollbar. The right pane contains stream publishing and available streams. Opening Settings or Statistics stops watching/sharing, as leaving the old Streaming view did. Peer connection details and voice packet counts are in Statistics; sidebar health dots remain on the server view.

## Global shortcuts

Default global shortcuts:

- `Ctrl+Shift+M`: toggle microphone mute.
- `Ctrl+Shift+D`: toggle deafen. Deafening stops playback and microphone transmission; undeafening restores the existing microphone-mute choice.

Both use the same control handlers as the buttons and work in normal/performance mode. They act only during a connected voice session, not during connection setup, shutdown or the local microphone test. Holding a key does not repeatedly toggle it: macOS/X11 events are filtered by press/release state; Windows registration uses `MOD_NOREPEAT`.

Lucide icons accompany the CPU, RAM, server, channel, microphone, playback and volume labels. The microphone icon reflects effective transmission state, including deafen and server restrictions; the crossed-out headphones indicate deafen. Icons keep their text labels in both normal and performance modes.

## Change and save controls

Open **Settings** using the gear icon at the top right. The adjacent graph icon opens **Statistics**; both show a label on hover and highlight the selected page. Edit the server address, nickname, and shortcut text, then choose **Save settings**. A hostname, IP address, or address with a port can be used. Examples of shortcut text include `Ctrl+Shift+M`, `Ctrl+Alt+D`, `F8`, and `CmdOrCtrl+Shift+M`. The latter uses Command on macOS and Control on other platforms.

The two actions must use different shortcuts. Invalid syntax is rejected. New keys are registered before old keys are released; an ordinary registration conflict preserves the previous working keys and leaves the file unchanged. The Settings page shows registration/save errors and the last shortcut received, so the keys can be checked while disconnected.

Global shortcuts use the native event loop on macOS and Windows, and X11 on Linux. Native Wayland is not supported by the selected hotkey library and is reported as unavailable; settings can still be saved there. The app's event bridge is driven by key presses, with no periodic hotkey polling.

## Microphone activity

In **Settings → Audio**, the **Input activity** bar below the Microphone selector shows the captured microphone level during a local microphone test or while unmuted and allowed to speak in a call. It remains empty while disconnected, muted, deafened, blocked by the server, or stopping. Use **Test microphone locally** and **Stop microphone test** there to check it without connecting. Performance mode keeps microphone-meter updates paused.

## Playback volume

The slider directly below **Settings → Audio → Playback** controls this app from 0–100%, starting at 100%. It works before connecting, during a session, and during the local microphone test. It applies output gain immediately through an atomic value read by the playback callback, with a short ramp to avoid abrupt changes. No allocation or lock is added to the audio callback.

0% silences playback while continuing to drain queued samples. It does not mute the microphone or change system volume. Use **Settings → Save settings** to retain the volume for the next launch.

## Channels and clients

While connected, the server sidebar places icon-only Disconnect, Mute and Deafen controls above the server name. Hover to see their actions. Audio settings live in the top-right Settings tab. Channel headings use speaker icons, clear spacing and a border on hover; clicking the heading joins the channel, while its separate arrow expands/collapses the hierarchy. The current channel stays highlighted. User avatars, channel user counts and separate Join/Joined buttons are omitted. Client rows show a health dot before the name and microphone/sound icons only while muted or deafened (including unavailable audio hardware). The app subscribes to channel updates once per connection. A channel whose members are not visible is explicitly marked rather than reported as empty.

Channel names are clickable when the server allows access or access is not known in advance; the server accepts or rejects the request. Explicit denials and unsupported voice codecs disable joining. Password-protected channels prompt only after clicking the channel name; passwords remain in memory and are cleared after submission or cancellation. Missing/incorrect passwords reopen the prompt with an error. Press Escape or Cancel to dismiss it. The server always makes the final decision, including passwords, channel capacity and permission changes.

## Connection health

While connected in normal mode, the fixed bottom bar shows incoming/outgoing packet loss, ping with variation, and idle time in a row immediately right of the overall-health indicator. Each reading has a value and a 20-pixel-high, 60-second line chart; its color indicates health and its hover detail includes the health label. The row stays visible on the server view, Settings and Statistics while content scrolls. Graphs have a minimum width of 110 logical pixels; if the window is too narrow, only this graph strip scrolls horizontally, while the health indicator and resource readings stay fixed. Samples move left as time passes; continuous colored lines join adjacent measured values and a translucent purple band shows ping ± variation. Each chart scales vertically to its data. Missing samples and intervals longer than 15 seconds leave gaps. Loss is Good below 1%, Fair from 1%, and Poor from 5%. Ping is Fair from 100 ms or 20 ms variation, and Poor from 200 ms or 50 ms variation. These are UI guidelines, not server guarantees; hover over a reading for its thresholds. Idle time has a neutral Active/Idle label and is not treated as a network fault.

**Channel connections**, in Statistics, lists only the other clients in your current channel. Every row shows a health dot before the name even while collapsed: Good (green), Fair (amber), Poor (red), or Unknown (neutral). The repeated channel name is omitted. Hover the dot for its health label and only problematic or unavailable measurements, using the same explanation as the footer. Sidebar client dots use the same data; your own dot uses your local connection metrics, and clients outside your channel show Unknown with an explanation that they are not sampled. The worst available loss/ping/variation rating wins; idle time is not a fault. Good requires all primary readings. Click a row to expand the same four charts; one client is expanded at a time. The channel tree keeps its normal names and mute indicators.

While the server view or Statistics is visible in normal mode, the worker refreshes all current-channel clients, including collapsed ones. Requests are staggered: at most one statistics request per second and one outstanding request across your own server statistics and all peers. Each peer is requested no more often than every five seconds, with the oldest request first. Your own five-second statistics retain priority. Small channels therefore refresh in roughly five seconds; large channels take longer, and samples older than 15 seconds become Unknown until refreshed. Opening Settings pauses peer polling; performance mode pauses all optional network telemetry and drops chart histories/caches. Membership and channel changes remove obsolete samples and polling targets.

For a small channel, each peer adds up to 12 requests per minute, but the shared cap stays at 60 total requests per minute including your own. The pinned protocol's example plaintext reply is 314 bytes; a synthetic reply containing all declared fields with ten-character values is 2,012 bytes. At the shared one-request/second cap those examples represent about 0.3–2 KB/s of response payload total, before transport framing, acknowledgements, retries and any server-to-peer exchange. These are size estimates, not measured live traffic or guaranteed bounds. Roster/mute-state subscriptions remain separate.

Only the latest sample/error is cached for each current-channel peer. Histories retain at most 60 samples each for your connection and the expanded client. They reuse existing update events, add no audio-callback work, and are included in the known-history buffer count in Statistics. Opening a different client starts a fresh history rather than retaining a growing history per user.

Missing or stale values show `—` and overall Unknown, never a fabricated healthy reading. Remote idle time is sampled rather than extrapolated. Denials/timeouts appear inside the details and stop that peer's requests until a new channel membership or reconnection resets them. The app copies displayed metrics from connection-info responses, not client IP addresses.

Local validation: 39 tests passed (one opt-in network test skipped), covering fair polling order, five-second per-peer spacing, denial handling, collapsed-label updates, stale data, and removal on channel/member changes. Clippy with warnings denied, formatting, and the macOS release build passed. An isolated native preview verified collapsed Good/Fair/Poor/Unknown labels and an expanded panel whose single poor metric correctly made the overall label Poor. The active call was left running on the earlier build. Live server polling cost and Windows/Linux runtime behavior remain unmeasured.

## Visual layers and connection footer

The [design system](design-system.md) defines shared button geometry, typography, spacing and surface colors in `src/appearance.rs`: a dark canvas, sidebar/chrome, content cards, and a raised layer for controls and floating details. Voice, Settings, Statistics, dialogs and the footer use the same palette. All rectangular widgets use zero corner radius, including buttons, inputs, dropdowns, menus, meters and scrollbars. These are static widget backgrounds and borders, with no new animation, renderer, dependencies or sampling tasks.

The fixed footer shows your channel connection's overall health at the bottom left. Hover to see only problematic incoming/outgoing loss, latency or ping variation. Healthy measurements and idle time are omitted from problem details; missing data is identified when it prevents a reading. Complete healthy readings show “No connection issues detected.” Your four compact metric graphs sit immediately to its right, with CPU/RAM/Footprint stacked at the far right. The indicator and graphs remain visible across all normal-mode tabs. Expanded peer graphs are in Statistics.

Performance mode shows Unknown with a tooltip explaining that connection-health sampling is paused, rather than presenting stale measurements as healthy. Its footer keeps the resource row above the health indicator to fit the compact window. Disconnected, connecting, stopping and local-test states are explicitly labeled.

Footer-row validation: the optimized Mac app showed all four graphs at normal width and kept them visible on Voice, Settings and Streaming. At a user-narrowed 464-logical-pixel window, horizontal scrolling and dragging the thin scrollbar reached the final Idle graph while health and process readings stayed fixed. Performance mode and restoration to normal mode were checked; the microphone remained muted. Clippy with warnings denied, formatting, diff whitespace checks and the release build passed. No additional polling or audio work was introduced.

Design validation: 41 tests passed (one explicit network test skipped), and final formatting/Clippy checks passed. The native offline preview confirmed the layered Voice/Settings/Statistics views, readable disabled fields, square controls, continuous connection lines with a shaded ping-variation band, and all five compact status rows fitting above the footer. Hovering the footer showed only outgoing packet loss and ping variation from the deliberately degraded fixture. The local macOS release bundle was built; the real session was left running on its earlier executable. Windows/Linux UI behavior was not checked for this redesign.

## Fixed resource bar

CPU, RAM and the platform-specific memory detail stay in the bottom bar while the server view, Statistics, Settings or the channel tree scroll. RAM here means resident memory (RSS). These are the same process readings used by Statistics, refreshed every second in normal mode and every ten seconds in performance mode; the bar adds no separate sampling timer.

## Errors and rate limiting

Connection, audio, command, settings, trace and streaming failures use a shared Rune modal with Close/Enter/Escape dismissal; the window behind it is inert and Tab stays inside. Repeats are grouped and dismissed incidents stay suppressed until a retry or relevant recovery. Wrong channel passwords stay in the password dialog; missing telemetry remains a status/tooltip.

Server commands share a bounded scheduler with acknowledgement tracking. Rapid mute/deafen changes retain only the latest unsent state while applying local protection immediately. Pending/unconfirmed server state is shown separately. Flood responses pause optional work, use a 10–60 second cooldown and a single acknowledged recovery probe, and lower the command rate. Server permissions and thresholds are unchanged. See [error lifetime, pacing and validation](error-handling.md).

## JSON file

The default file is `settings.json` beside the local identity file. **Copy settings path** gives its exact location, and `--settings-path` prints it. On this Mac it is:

```text
/Users/jangj/Library/Application Support/io.Venator.TeamSpeakNative/settings.json
```

The portable format is shown in [`settings.example.json`](../settings.example.json):

```json
{
  "server_address": "ts.zirkumflex.io",
  "nickname": "Flexspeak",
  "playback_volume_percent": 100,
  "keybinds": {
    "toggle_mute": "Ctrl+Shift+M",
    "toggle_deafen": "Ctrl+Shift+D"
  }
}
```

Copy the JSON to the settings location on another computer, or set `NATIVE_VOICE_SETTINGS` to a custom JSON path. After external edits, disconnect and choose **Reload JSON**, or restart. The file may also contain optional `input_device_id` and `output_device_id` strings. These are machine-specific: choose devices again, or omit the fields to use defaults, when copying settings to another computer. Server/channel passwords and the private TeamSpeak identity are not saved in this file. `"auto_download_updates": false` turns off background update downloads (see [releases](releases.md#in-app-updates)); like `footer_hidden`, the Settings switch saves it immediately.

Saving is explicit. Missing files start onboarding; missing known fields in an existing file receive defaults. Invalid JSON, unknown field names, invalid shortcuts and out-of-range volume produce an error. Loading an invalid file does not rewrite it. A successful Save writes a complete temporary file and renames it into place; failed writes do not leave partially written settings. Saving the displayed values after a load error explicitly replaces the invalid file.

## Earlier controls/settings validation

All 26 tests passed, including settings round trips/defaults, invalid-file preservation, invalid/duplicate bindings, repeated-key filtering, failed control delivery, deafen/mute state, volume scaling, silence, and gain ramping. Clippy with warnings denied and macOS/Windows release builds pass. Native Mac inspection confirmed the slider, editable Settings fields and a saved 39% test volume in an isolated JSON profile. Editing that JSON and reloading restored a different server address, nickname and 73% volume; editing a keybind in Settings updated both the registered-key hint and JSON file. The user confirmed both global actions were detected with physical key presses while another app was focused, and the UI showed the received deafen action. App-targeted synthetic key injection did not exercise macOS's global shortcut path.

The disconnected test profile uses separate keys and a temporary settings path so validation does not change the user's active voice session or saved defaults. Windows global-hotkey/audio runtime validation and native Wayland support remain outside the completed checks.

## Audio processing options

Noise suppression (Rauschunterdrückung), automatic microphone gain and echo cancellation are not yet wired into the microphone path. The bundled native WebRTC APM on Mac/Windows already exposes these plus a high-pass filter. A practical next step is microphone-only noise suppression and optional automatic gain; the current binding supports on/off switches. Echo cancellation additionally needs synchronized playback reference frames (including voice and received stream audio), the actual playback gain and device delay. These effects should not process shared music/system audio by default. See [WebRTC APM's capture/render contract](https://webrtc.googlesource.com/src/+/refs/heads/main/api/audio/audio_processing.h).

## Validation of this update

74 ordinary tests, the three packaging tests, Mac/Windows cross-target Clippy and both optimized builds passed. Automated checks cover setup order, required username/server input, interrupted setup, device persistence, failed-write and invalid-hotkey handling, old/corrupt config behavior, and the unified view's sampling/streaming visibility. Native permission dialogs, device playback and visual layout were not exercised through computer-use automation, per the user's request to stop that testing.
