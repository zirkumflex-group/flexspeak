# Setup, controls and saved settings

## First launch

When `settings.json` does not exist, the app opens a six-step setup: username, one saved server, OS permissions, audio devices/local testing, the chat layout, and hotkeys. It writes the configuration only after **Finish setup** succeeds. Closing an unfinished setup leaves no default file behind, so the next launch starts setup again. A valid existing configuration skips setup. A malformed or unreadable file is preserved and opens **Settings → Settings File** with an error instead of being replaced by onboarding.

If a TeamSpeak 6 install is found, the first step offers to carry over its settings. Nothing is read until that button is pressed. The display name, saved server addresses, the microphone and speakers (matched by name) and push-to-talk with its release delay are then filled in, and the Username, Server and Devices steps show each value under **From TeamSpeak 6**, with **Use** to return to it after a change. Every imported server is saved unless removed there. The display name shares a record with login data: only that field is copied out, and the rest is neither kept nor logged. Server passwords, identities and shortcuts are not imported. TeamSpeak does not document this file, so anything unexpected is skipped. The location is confirmed on macOS (`~/Library/Preferences/TeamSpeak/Default/settings.db`); the Windows and Linux equivalents are assumed.

Microphone and playback selections are saved by device ID. If a saved device is unavailable on a later launch, the picker remains unselected so the user can explicitly choose a replacement. Older files without device IDs use the detected defaults. The local microphone test makes no server connection; stop it before advancing setup. Passwords remain session-only.

The Permissions step, and the same section in Settings, shows what the operating system currently allows for microphone, screen sharing and system audio: **Allowed**, **Not allowed**, **Not allowed yet** (never asked), **Not needed** or **Unknown**. Reading the status never prompts. It is re-read while the section is visible, so a change made in the OS privacy settings shows up on return. **Give Access** appears only when access is missing. If the user has never been asked, it shows the system prompt. If access was denied, it opens the matching privacy pane, because the OS will not ask again. Mac reads the microphone from `AVAudioApplication.recordPermission`, screen recording from `CGPreflightScreenCaptureAccess`, and system audio (which has no public status call) from TCC's `TCCAccessPreflight`, loaded at runtime with an **Unknown** fallback. Requests use the native audio permission request, the Core Graphics screen-access request, and a brief output-loopback check whose callback discards samples. macOS may need the app reopened before a new screen-recording grant takes effect. Windows reads the microphone privacy switches (device, apps and desktop apps) from the CapabilityAccessManager consent store in the registry; desktop screen capture and output loopback need no grant and show **Not needed**, though system policy can still restrict them. Permission denial can be handled later in Settings and does not trap the user in setup. Camera access is not part of setup: it is requested only when you go live with a camera (Mac shows the system prompt; Windows applies its camera privacy switch when the device opens). Accessibility permission is never requested.

## Server view and window bounds

The window has a **760 × 560 logical-pixel minimum**. It opens where it was last closed, at the same size and maximized if it was, unless that place is no longer on a connected display. Only one copy runs: opening Flexspeak again brings the running window to the front, from the notification area too. While connected, the window's **×** asks **Still connected**: **Cancel**, **Hide to Tray** (only while the notification-area icon is on) or **Quit**, with **Don't Ask Again** to keep the answer. **Settings → Appearance → Window → Close button** sets it to Ask, Hide to Tray or Quit; with nothing connected, × always quits. Hidden, every server stays connected; the tray icon's click or **Show Flexspeak** brings the window back, and its menu has **Disconnect** while connected. On Windows, **Start with Windows** opens Flexspeak at sign-in (a Run entry for the current user), and **Start minimized** has it wait in the notification area. **Disconnect** asks first only while you are sharing your screen, since leaving ends the stream for its viewers.

The top bar contains Statistics and Settings; Voice and Streaming tabs are removed. The disconnected home screen centers a **Saved servers** card: one row per server (its name, or address, with the address beneath a name) and its own **Connect** button. The first server's Connect is the default button, so Return connects to it; **+** adds a server. While a server connects, its row offers **Cancel**; the other servers stay available.

Manage the list in **Settings → Server → Servers**. **Add Server** and each row's **Edit** open a dialog with **Address** (required, unique in the list), an optional **Name**, and an optional **Password**. The password is kept per server until the app quits and is never written to the file. Saving the dialog writes the list straight away. **+** on the home screen or the rail opens the dialog over Settings, and Settings closes again with it. **Remove** is in the Edit dialog and asks for a second press; the last server can't be removed, and the server in use can't be edited or removed until you disconnect. The row of the server in use is marked **Connecting** or **Connected**. Your **Nickname** (Settings → Server → Identity) is used on every server; it is saved on Return, when the field loses the keyboard, when Settings closes and when you connect. An empty nickname isn't kept: the field says so underneath, and the saved name comes back when Settings closes. Each row's **Auto connect** switch connects to that server when Flexspeak opens; the first such server is the one shown. Passwords aren't saved, so a server that needs one can't connect this way. A restart for an update returns to the servers you were on instead.

Once connected, the left sidebar contains the server/channel tree and Disconnect, Mute and Deafen. While you share your screen, the same panel shows your viewers, requests to watch, and the way to stop. A **Server log** above that panel lists who connected, moved, left, was kicked or banned, and who started or stopped streaming, under pinned hour headings; it keeps the last 1000 entries of each server and is hidden in very short windows. Hover it for a magnifier that searches names, actions and channels and filters to one person; Esc closes the search. Stream controls such as refresh and layout appear over the streams while the pointer is on them. The share dialog shows one still picture of the chosen source (screens and windows on Windows, cameras on both; on a Mac, the system picker has its own previews), and sharing starts with the quality, access and sound you last used. The channel tree reserves 16 logical pixels before its scrollbar. The right pane contains stream publishing and available streams. Opening Statistics leaves your share and the streams you watch running, and they are on screen again when you return; Settings floats over the server view and leaves streams, sharing and peer polling running. Peer connection details and voice packet counts are in Statistics; sidebar health dots remain on the server view. On a TeamSpeak 6 server, while you aren't sharing, chips above the voice buttons go live with one click and no confirmation: your main screen, then the game you're playing. These streams are always public, whatever the share dialog is set to.

## Several servers at once

Connect to as many servers as you like. Once one is connected, a rail runs down the far left of the window with a tile for each. The tile shows the server's initials on a colour derived from its name; the active one has a rounded-square tile and a white pill on its left edge. Under every tile, a mic and a headphones icon show that server's own mute and deafen. They're red with a strike when off, and grey otherwise. A green ring means someone is talking there, and a small amber dot on the corner means it's still connecting. The **+** tile at the bottom opens your saved servers to connect another; the new one comes to the front once it has connected, unless you've picked a server in the meantime. With only one server connected the rail hides to leave more room: hover the handle on the window's left edge to bring it back, and it returns for good once a second server connects.

Click a tile to make that server **active**. The sidebar, channel tree, voice panel, streams pane and footer statistics show the active server. Each server keeps its own mute and deafen. You **hear every server you haven't deafened**, and your microphone (continuous, push-to-talk or voice activation) transmits **only to the active server**. Switching mid-sentence ends the utterance on the old server and starts a new one on the new server, so nobody hears anything you didn't mean for them. The microphone, playback device, echo cancellation and audio buffers are shared: they open with the first connection and close with the last, and a device failure disconnects every server with one error. Disconnect ends the active server and brings another to the front, or returns to the saved-server home when it was the last.

A poke on a server you aren't looking at shows an amber marker with a hand on its tile, with a count when there's more than one; switching to that server clears the marker, and the poke's dialog is waiting there. It still plays the normal chime. A desktop notification shows only while Flexspeak is not the window in front, at most one every 10 seconds, and the taskbar button flashes until you come back. An error from a background server names that server in its dialog instead of looking like the active one failing, and a background server dropping leaves the active view alone.

Audio devices, microphone processing and transmission can change while servers are connected or the local microphone test runs: the shared devices reopen with the new choice, and a new threshold or release delay applies without reopening them. The nickname and the settings file's Reload wait until you disconnect. The local microphone test runs only when no server is connected, and connecting is blocked during it. An update's restart reconnects every server and returns to the active one.

## Global shortcuts

Default global shortcuts:

- `Ctrl+Shift+M`: toggle microphone mute.
- `Ctrl+Shift+D`: toggle deafen. Deafening stops playback and microphone transmission; undeafening restores the existing microphone-mute choice.

Both use the same control handlers as the buttons and work on every page, on the active server: with more than one connected, a hint under the server name says so. They act only during a connected voice session, not during connection setup, shutdown or the local microphone test. Holding a key does not repeatedly toggle it: macOS/X11 events are filtered by press/release state; Windows registration uses `MOD_NOREPEAT`.

Lucide icons accompany the CPU, RAM, server, channel, microphone, playback and volume labels. The microphone icon reflects effective transmission state, including deafen and server restrictions; the crossed-out headphones indicate deafen.

## Change and save controls

Open **Settings** using the gear icon at the top right, or **⌘,** (**Ctrl+,** on Windows and Linux). It opens as a window over the app: categories on the left (**Server**, **Appearance**, **Chat**, **Streaming**, then **Voice**: **Audio**, **Sounds**, **Shortcuts**, **Permissions**, then **App**: **General**, **Updates**, **Error Log**, **Settings File**) and the chosen category's settings on the right. **General** holds the tray icon switch and **Run Setup**. **Search** at the top of the sidebar (or press **/**) filters every category at once to the rows that match, such as `mic`, `deafen` or `ember`, heads each category's results with its name, and dims the categories with no match. **Escape** clears a search first, then closes the window; **×** or a click outside the panel also closes it, and the gear reopens it on the category last used. Every setting saves as it changes; there is no Save button. Sliders write the file when Settings closes or the app quits. Settings is unavailable during setup and in performance mode. The adjacent graph icon opens **Statistics**; both show a label on hover and highlight while open. A hostname, IP address, or address with a port can be used. Examples of shortcut text include `Ctrl+Shift+M`, `Ctrl+Alt+D`, `F8`, and `CmdOrCtrl+Shift+M`. The latter uses Command on macOS and Control on other platforms.

The actions must use different shortcuts, and any of them can be left unset. Invalid syntax is rejected. New keys are registered before old keys are released; an ordinary registration conflict preserves the previous working keys and leaves the file unchanged. A key another app holds says **In use by another app** on its row, and Flexspeak takes it once it's free. When global shortcuts can't work at all, the page says why under the rows.

Each shortcut has a **Change** button: press it, then press the new key combination, and it is recorded and saved at once. Keys are read by their physical position, so a shortcut does not depend on the keyboard layout. A single key works as a shortcut, but a global shortcut takes that key from every other app while Flexspeak runs, so a plain letter can no longer be typed elsewhere; function keys avoid this. Escape cancels, and Backspace clears the shortcut. The existing shortcuts are released while recording, so they can be pressed and reassigned.

Global shortcuts use the native event loop on macOS and Windows, and X11 on Linux. Native Wayland is not supported by the selected hotkey library and is reported as unavailable; settings can still be saved there. The app's event bridge is driven by key presses, with no periodic hotkey polling.

## Microphone activity

In **Settings → Audio**, the **Input activity** bar below the Microphone selector shows the captured microphone level during a local microphone test or while unmuted and allowed to speak in a call. It remains empty while disconnected, muted, deafened, blocked by the server, or stopping. Use **Test microphone locally** and **Stop microphone test** there to check it without connecting. Performance mode keeps microphone-meter updates paused.

## Playback volume

The slider directly below **Settings → Audio → Playback** controls this app from 0–100%, starting at 100%. It works before connecting, during a session, and during the local microphone test. It applies output gain immediately through an atomic value read by the playback callback, with a short ramp to avoid abrupt changes. No allocation or lock is added to the audio callback.

0% silences playback while continuing to drain queued samples. It does not mute the microphone or change system volume. The volume is saved when Settings closes or the app quits.

## Channels and clients

While connected, the server sidebar places icon-only Disconnect, Mute and Deafen controls above the server name. Hover to see their actions. Audio settings live in the top-right Settings tab. Channel headings use speaker icons, clear spacing and a border on hover; clicking the heading joins the channel, while its separate arrow expands/collapses the hierarchy. The current channel stays highlighted. User avatars, channel user counts and separate Join/Joined buttons are omitted. Client rows show a health dot before the name and microphone/sound icons only while muted or deafened (including unavailable audio hardware). The app subscribes to channel updates once per connection. A channel whose members are not visible is explicitly marked rather than reported as empty.

Channel names are clickable when the server allows access or access is not known in advance; the server accepts or rejects the request. Explicit denials and unsupported voice codecs disable joining. Password-protected channels prompt only after clicking the channel name; passwords remain in memory and are cleared after submission or cancellation. Missing/incorrect passwords reopen the prompt with an error. Press Escape or Cancel to dismiss it. The server always makes the final decision, including passwords, channel capacity and permission changes.

## Text chat

A chat column sits to the right of the stage while you are connected. It carries the three places TeamSpeak sends text to: your channel, the whole server, and one person. **Settings → Chat** (and a step of setup) picks its layout, and the choice is saved at once:

- **Tabs** shows one conversation at a time: a tab for the channel, one for the server, and one per person you message. A channel you leave keeps a fainter tab for as long as it has something to read; you can't write there, but its tab can be closed and the message box offers **Join**. Going back makes it the channel tab again, with what was said before. An **All** tab at the far left shows every conversation together, tagged as One Feed tags them, with the message box choosing where it sends; pressing a line's tag opens that conversation. **Settings → Chat → Conversations in the bar** chooses how many tabs fit: **Side by Side** scrolls the bar sideways, and **Grouped** shows one tab each for channels, the server and people, listing a group's conversations when you point at it or press it.
- **One Feed** shows every line in one stream, tagged `#Channel`, `Server` or `to you` / `to Name`, with private lines on a tint. Chips narrow it to one kind. With **Grouped** chosen in Settings, the **Channels** and **People** chips also list each channel (the ones you left included) and each person: picking one narrows the feed to it alone, the chip then carries its name, and pressing the chip widens to its kind again. Narrowed to a person, the message box writes to them. Pointing at or pressing the **To** line above the message box lists where it can send; pressing a line's tag points it there, and Escape returns to the channel.

The message box names who gets the message on a **To** line above the field: the channel, the server or the person, and who reads it. Double-click someone in the channel tree, or right-click them and choose **Message**, to write to them. While Flexspeak isn't the window in front (another app is, or it is minimized or hidden in the tray), new messages wait as unread; a private one also plays the **Private message** sound and flashes the taskbar button, while channel and server messages only wait. A private conversation is filed under the person's identity, so it survives them reconnecting; the message box locks while they are off the server. Web addresses are links. A TeamSpeak server keeps no history, so Flexspeak keeps its own: messages and pokes are saved in `chat.db` beside `settings.json` (SQLite, plain text like the settings, never sent anywhere) and come back when you next connect to that server, with a line between days. They are filed under the address you connect by. Each conversation brings back its latest 200 lines, and a server keeps its latest 10,000. A tab you closed stays closed until something new is said there. **Settings → Chat → History** switches keeping it off, which also stops reading it, and **Clear History** (pressed twice) forgets every server's conversations. Messages are sent as the server takes them (up to 1024 bytes, addresses wrapped in `[URL]` for TeamSpeak's own clients), and a refusal shows as a "Message not sent" notice.

Pokes you receive and send are kept in the conversation as faded lines with a bell. They still chime and open their dialog.

The panel button at the column's top right, or ⇧⌘C (Ctrl+Shift+C), hides the column behind a handle on the window's right edge, the twin of the server rail's on the left. Pointing at the handle peeks the chat over the stage, and pressing it docks the column again. In a window narrower than 1000 points the column always peeks instead of docking. While it is hidden, a new message lights a dot on the handle (violet for a private one) and pops up at the top right for five seconds with **Reply** and **Open Chat**; Settings → Chat can limit pop-ups to private messages, turn them off, or hide the column by itself while a stream is being watched. Unread private messages also show as a count on the person in the tree, and on the server's tile in the rail while another server is on screen.

## Connection health

While connected in normal mode, the fixed bottom bar shows incoming/outgoing packet loss, ping with variation, and idle time in a row immediately right of the overall-health indicator. Each reading has a value and a 20-pixel-high, 60-second line chart; its color indicates health and its hover detail includes the health label. The row stays visible on the server view, Settings and Statistics while content scrolls. Graphs have a minimum width of 110 logical pixels; if the window is too narrow, only this graph strip scrolls horizontally, while the health indicator and resource readings stay fixed. Samples move left as time passes; continuous colored lines join adjacent measured values and a translucent purple band shows ping ± variation. Each chart scales vertically to its data. Missing samples and intervals longer than 15 seconds leave gaps. Loss is Good below 1%, Fair from 1%, and Poor from 5%. Ping is Fair from 100 ms or 20 ms variation, and Poor from 200 ms or 50 ms variation. These are UI guidelines, not server guarantees; hover over a reading for its thresholds. Idle time has a neutral Active/Idle label and is not treated as a network fault.

**Channel connections**, in Statistics, lists only the other clients in your current channel. Every row shows a health dot before the name even while collapsed: Good (green), Fair (amber), Poor (red), or Unknown (neutral). The repeated channel name is omitted. Hover the dot for its health label and only problematic or unavailable measurements, using the same explanation as the footer. Sidebar client dots use the same data; your own dot uses your local connection metrics, and clients outside your channel show Unknown with an explanation that they are not sampled. The worst available loss/ping/variation rating wins; idle time is not a fault. Good requires all primary readings. Click a row to expand the same four charts; one client is expanded at a time. The channel tree keeps its normal names and mute indicators.

While the server view or Statistics is visible in normal mode, the worker refreshes all current-channel clients, including collapsed ones. Requests are staggered: at most one statistics request per second and one outstanding request across your own server statistics and all peers. Each peer is requested no more often than every five seconds, with the oldest request first. Your own five-second statistics retain priority. Small channels therefore refresh in roughly five seconds; large channels take longer, and samples older than 15 seconds become Unknown until refreshed. Settings floats over the server view and leaves peer polling running; performance mode pauses all optional network telemetry and drops chart histories/caches. Membership and channel changes remove obsolete samples and polling targets.

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

## Appearance

**Settings → Appearance** holds **Color Scheme** (System, Light or Dark) and **Themes**. Five themes ship with the app — Flexspeak (blue), Grove (green), Ocean (teal), Ember (orange) and Iris (violet) — and each has a light and a dark palette. Each theme card shows two swatches: press the light one to use that theme while the window is light, and the dark one for while it is dark. The two choices are independent, so Grove can be the light theme and Ember the dark one. System follows the computer's appearance: on macOS the app's effective appearance (including Auto), on Windows the app mode in Personalisation, and on Linux GNOME's `color-scheme`, read once at launch. macOS and Windows are checked with each usage tick, so a change shows within a second (ten in performance mode).

Right-click a server in the rail to **Disconnect** from it without switching to it.

Right-click someone in the channel tree, or a ⌘/Ctrl-click selection of several people, for **Server groups**, **Channel group**, **Kick from channel…**, **Kick from server…** and **Ban…**. Kick and ban only appear when the server allows them for everyone selected, and ask for a reason and confirmation first. In **Server groups**, tick or untick any number of groups; each one applies straight away and the menu stays open. **Some** means only some of the selected people have that group. Each person has one channel group per channel, so **Channel group** is a single choice. The server's default group is shown greyed out, because nobody can be added to or removed from it. The server decides whether you may change a group, and says so if you may not.

Right-click someone in the channel tree to **Mute** them or set their **Volume** from 0 to 200%. This only changes what you hear, is remembered by their identity across servers and reconnects, and a person you muted shows a red speaker icon on their row. **Sound Cues** under Settings → Sounds covers your own mute, unmute, deafen and undeafen, being poked, joining or being moved to a channel, and people joining or leaving your stream. It also covers connecting, disconnecting, losing and regaining the connection, people joining or leaving your channel, starting and stopping your stream, and being kicked or banned. It has one volume and one **Sound pack** for all of them: Flexspeak's own, Soft, or TeamSpeak's. The volume moves in even steps of loudness: 100% plays the sounds as they are, every tenth below that is 4 dB quieter, and 0% is silent. A new install starts at 50%, which is 20 dB under full. The three packs play equally loud, so changing the pack doesn't change how loud the sounds are. A volume saved by an earlier version, when it was a plain percentage and the packs differed, is converted to the setting that sounds the same. The sounds are in seven groups, each with a switch. Pressing a group's name opens it to its sounds, each with its own switch and a button to hear it, connected or not; searching Settings for a sound shows it under its group. A group's switch silences all of its sounds and keeps their own switches for when it comes back. Settings files from when each sound chose its own pack are read as the pack most sounds used, with the sounds that were off still off. TeamSpeak's sounds are not shipped with Flexspeak: they are read from a TeamSpeak install on your computer, and the choice only appears when one is found. The location is confirmed on macOS and Windows; the Linux location is assumed. The old `sounds.folder` setting is ignored and omitted on the next save. Someone in your channel connecting, disconnecting, losing connection, being moved in or out, being kicked or being banned each has its own sound. Sounds also cover someone recording in your channel, a whisper to you, your talk power and groups changing, and the server refusing an action. Someone else in your channel starting or stopping a stream has its own sound, and with TeamSpeak's sounds it is the one TeamSpeak plays for any stream. TeamSpeak has no sound for people joining or leaving your stream, so with its sounds chosen Flexspeak plays its own stream sounds for them, raised a little: **Stream started** a whole tone higher as someone arrives and **Stream stopped** a semitone higher as they leave, with the room after the notes cut short. In every pack, these two play 6 dB under the other sounds, because they come often. TeamSpeak's files differ a lot in loudness, so Flexspeak plays them all equally loud and without the silence at their ends. A sound waits for the one before it to finish instead of cutting it off. At most two wait, the same sound isn't repeated within a second, and one that would start more than three seconds late is skipped, so a burst of events doesn't play on after it is over. Your own mute, deafen, channel switch and disconnect are heard at once, in place of whatever was playing. The last sound finishes before the audio device closes, so **Disconnected** and **Kicked** are heard in full.

On an Omarchy desktop, Flexspeak follows the active Omarchy theme and updates within a second of a switch, including themes you installed yourself. An **Omarchy** section appears at the top of Appearance only there; turn off **Match Omarchy theme** to use the themes below instead.

**More Themes** lists the bundled Tokyo Night, Catppuccin and Omarchy palettes, each marked light or dark, and any themes you add under `custom_themes` in the settings file. **Open Folder** shows the folder holding it; press Reload on the Settings File page to pick up edits. The format is T3 Code's, and its roles beyond these are ignored, so its themes can be pasted in:

```json
{
  "custom_themes": [{
    "id": "my-theme",
    "label": "My Theme",
    "appearance": "dark",
    "colors": {
      "canvas": "#101014", "chrome": "#16161c", "surface": "#1c1c24", "surfaceRaised": "#2a2a34",
      "text": "#e8e8f0", "textMuted": "#a0a0b0", "placeholder": "#707080",
      "accent": "#8ea8ff", "accentForeground": "#0b0f1c",
      "border": "#ffffff14", "error": "#ff7a8a", "warning": "#f2c46b", "success": "#6fdc9a"
    },
    "variants": { "light": { "canvas": "#ffffff", "surface": "#ffffff", "text": "#000000", "accent": "#264adc" } },
    "collection": { "id": "mine", "label": "Mine" }
  }]
}
```

`canvas`, `surface`, `text` and `accent` are required; the rest fall back sensibly. Colours may be `#rgb`, `#rrggbb` (either with alpha), `rgb()` or `oklch()`. A theme that can't be read is skipped and named under the section, and never stops the settings from loading. A theme with the same `id` as a bundled one replaces it. Palettes are derived from [Tokyo Night](https://github.com/folke/tokyonight.nvim) (Apache-2.0), [Catppuccin](https://github.com/catppuccin/palette) (MIT) and [Omarchy](https://github.com/basecamp/omarchy) (MIT), with neutrals and contrast adjusted to Flexspeak's roles.

**Motion** has **Disable animations**, off by default. Turning it on stops movement and pulsing across the app; opacity and colour changes keep a very brief fade. Left off, the system's Reduce Motion setting still applies.

Like the footer choice, appearance applies and saves immediately. It is stored as an optional `appearance` object and left out of the file while it is the default:

```json
"appearance": { "scheme": "system", "light_theme": "grove", "dark_theme": "ember" }
```

## Errors and rate limiting

Connection, audio, command, settings, trace and streaming failures show as notices in a corner of the window, each with a close button and, where it helps, one action such as **Try Again** or **Audio Settings**; the window behind stays usable. Repeats are grouped and dismissed incidents stay suppressed until a retry or relevant recovery. Wrong channel passwords stay in the password dialog; missing telemetry remains a status/tooltip.

Server commands share a bounded scheduler with acknowledgement tracking. Rapid mute/deafen changes retain only the latest unsent state while applying local protection immediately. Pending/unconfirmed server state is shown separately. Flood responses pause optional work, use a 10–60 second cooldown and a single acknowledged recovery probe, and lower the command rate. Server permissions and thresholds are unchanged. See [error lifetime, pacing and validation](error-handling.md).

## JSON file

The default file is `settings.json` beside the local identity file. **Settings → Settings File → Copy Path** gives its exact location, and `--settings-path` prints it. On this Mac it is:

```text
/Users/jangj/Library/Application Support/io.Venator.TeamSpeakNative/settings.json
```

The portable format is shown in [`settings.example.json`](../settings.example.json):

```json
{
  "servers": [
    {
      "address": "ts.zirkumflex.io"
    }
  ],
  "nickname": "Flexspeak",
  "playback_volume_percent": 100,
  "audio_processing": {
    "noise_suppression": false,
    "suppression_level": "moderate",
    "automatic_gain": false,
    "echo_cancellation": false
  },
  "transmission": {
    "mode": "continuous",
    "ptt_release_ms": 150,
    "voice_threshold_db": -40
  },
  "keybinds": {
    "toggle_mute": "Ctrl+Shift+M",
    "toggle_deafen": "Ctrl+Shift+D",
    "push_to_talk": "Ctrl+Shift+Space"
  }
}
```

Copy the JSON to the settings location on another computer, or set `NATIVE_VOICE_SETTINGS` to a custom JSON path. After external edits, disconnect and choose **Settings → Settings File → Reload**, or restart. Each entry in `servers` has an `address` and an optional `name`, in the order the home screen lists them. A file written before the server list, with one `server_address`, still loads: that server becomes the first in the list, and the next save writes `servers` instead. The file may also contain optional `input_device_id` and `output_device_id` strings. These are machine-specific: choose devices again, or omit the fields to use defaults, when copying settings to another computer. Server/channel passwords and the private TeamSpeak identity are not saved in this file. `"auto_download_updates": false` turns off background update downloads (see [releases](releases.md#in-app-updates)); like `footer_hidden`, the Settings switch saves it immediately.

Every change is saved as it is made. Missing files start onboarding; missing known fields in an existing file receive defaults. Invalid JSON, unknown field names, invalid shortcuts and out-of-range volume produce an error. Loading an invalid file does not rewrite it. Each save writes a complete temporary file and renames it into place; failed writes do not leave partially written settings.

## Earlier controls/settings validation

All 26 tests passed, including settings round trips/defaults, invalid-file preservation, invalid/duplicate bindings, repeated-key filtering, failed control delivery, deafen/mute state, volume scaling, silence, and gain ramping. Clippy with warnings denied and macOS/Windows release builds pass. Native Mac inspection confirmed the slider, editable Settings fields and a saved 39% test volume in an isolated JSON profile. Editing that JSON and reloading restored a different server address, nickname and 73% volume; editing a keybind in Settings updated both the registered-key hint and JSON file. The user confirmed both global actions were detected with physical key presses while another app was focused, and the UI showed the received deafen action. App-targeted synthetic key injection did not exercise macOS's global shortcut path.

The disconnected test profile uses separate keys and a temporary settings path so validation does not change the user's active voice session or saved defaults. Windows global-hotkey/audio runtime validation and native Wayland support remain outside the completed checks.

## Audio processing options

**Settings → Audio → Microphone Processing** offers three independent, opt-in WebRTC effects on Mac and Windows, plus typing suppression on every platform:

- **Noise suppression** (Rauschunterdrückung) has Low, Moderate, High and Very high strength. Moderate preserves the earlier behavior; stronger settings can affect voice quality.
- **Typing suppression** runs [RNNoise](https://github.com/xiph/rnnoise) (the pure-Rust `nnnoiseless` port) after the WebRTC effects. WebRTC's suppressor targets steady noise; RNNoise also removes keyboard clicks and other clatter. It is the counterpart of TeamSpeak's typing suppression, but a different algorithm. It runs after WebRTC so echo cancellation still analyses the unaltered microphone, and it needs no native library, so it also works on Linux.
- **Automatic microphone gain** applies adaptive digital gain to keep voice levels more consistent. It does not change the operating system’s microphone volume.
- **Echo cancellation (experimental)** reduces this app’s speaker playback returning through the microphone. Its reference includes received voice and stream audio, after app volume, channel mapping and device-format conversion. Audio played independently by other apps is outside that reference.

All effects default to **off**, including when loading older JSON. Effects save as they change and apply at once, during a call or the local test too. The local headphone test previews suppression and gain but bypasses echo cancellation: listening to your own microphone through speakers is a feedback loop, not a useful echo test. Linux retains saved preferences but does not apply the WebRTC effects; its settings explain that they are unavailable and still offer typing suppression. Effects are on/off switches, and the strength picker is enabled while noise suppression is on.

Microphone processing runs on the existing voice worker in 10 ms / 480-sample frames after resampling to 48 kHz and before local monitoring or 20 ms Opus encoding. The PCM16 adapter bounds nonfinite/out-of-range input; when effects are off, original float samples bypass it. Enabling any WebRTC effect also enables WebRTC’s high-pass filter. Shared music/system-audio publishing is not processed. Microphone meters remain input meters. Transmission mode is applied after processing and before Opus encoding; mute, deafen and server restrictions remain authoritative.

Echo reference samples are copied into a bounded queue from the actual playback buffer, including silence on underrun. Each callback maps its own CPAL clock to the worker’s monotonic clock; the worker accounts for resampling latency and both capture/playback delays. Reference processing precedes capture processing. Capture generations reject in-flight pre-mute samples, and mute/resume, audio-buffer discontinuities, device changes and reconnects reset partial frames and processor history. Echo cancellation needs a short reconvergence period after resets; startup capture is discarded until usable reference timing arrives (at most 500 ms).

Processor errors, unavailable timestamps, a stale reference or out-of-range echo delay stop the audio session and show the existing audio error dialog. Nothing silently falls back to unprocessed transmission. Change devices or disable echo cancellation before retrying a timing failure. Buffer glitches reset processing rather than being treated as device loss.

Automated checks cover the pinned native processor, irregular frame sizes, silence/reset behavior, exact bypass, PCM bounds, independent timestamp origins, reference timing failures, post-volume/mono/stereo output references, overflow and pre-mute sample rejection. A generated delayed-echo test verifies attenuation through the native AEC. These are synthetic checks, not proof of real speaker, Bluetooth or Windows hardware quality; echo cancellation stays labeled experimental. No new SDK/provider dependency or Krisp integration was added. See the [research and integration evidence](audio-processing-research.md).

Validation on 2026-11-01: 83 ordinary Rust tests and three packaging tests passed (three explicit network/media integration tests remained ignored). Formatting, Mac/Windows all-target Clippy and optimized Mac/Windows builds passed. The final input-timing guard was rechecked with the focused processor tests and Mac Clippy before the release builds. The new app was not launched and microphone/speaker hardware was not exercised.

## Voice transmission

Select **Settings → Audio → Voice Transmission** while disconnected or after stopping the local test, then save:

- **Continuous** preserves the existing behavior: unmuting sends continuously.
- **Push to talk** captures/transmits while the assigned global key is held, followed by the chosen **0–500 ms release delay** (default 150 ms). Set the key in the **Push-to-talk key** row that appears under the mode, or under **Keyboard Shortcuts**, then unmute before holding it during a call. New profiles suggest `Ctrl+Shift+Space`; older two-key profiles retain their existing bindings and leave PTT unassigned until you choose a key. Empty PTT bindings do not disable mute/deafen, but starting a PTT session requires a registered PTT key. Duplicate bindings are rejected.
- **Voice activation** requires both WebRTC speech detection and a level above the **−60 to 0 dBFS** threshold (default −40). Level is RMS after microphone processing, so it differs from the raw peak input meter. It keeps up to 60 ms of preceding audio and a 200 ms tail to avoid clipping word beginnings/endings. Speech detection is not a guarantee against every background sound.

The local headphone test uses the selected transmission mode without connecting: hold the PTT key in PTT mode, or speak above the threshold in voice-activation mode. Stop the test to adjust settings. Echo cancellation remains bypassed in this feedback test. Continuous stays the default for older configurations; imported unsupported modes never silently switch to continuous.

PTT state reaches the voice worker directly from the hotkey callback, so a delayed UI queue cannot miss a release or reopen the microphone. A release closes capture after its configured tail. Mute/deafen, unavailable talk permission, stop and disconnect override it. The pinned Windows hotkey backend detects key release at approximately 50 ms intervals, in addition to the selected delay and audio buffering. Global PTT is supported on Mac, Windows and Linux X11; native Wayland cannot start a PTT session. Speech detection and selectable WebRTC suppression are currently Mac/Windows only.

The worker gates complete 20 ms frames, bounds pre-roll, clears it on mute/reset, resets the Opus encoder at utterance starts and sends an empty TeamSpeak voice packet at utterance ends. Waiting/transmit status changes are reported independently of the meter, including in performance mode. Shared stream/system audio remains outside microphone processing and transmission gating.

`native/audio.cpp` is a small fixed-frame C interface to the existing pinned WebRTC binary. It exposes APM suppression strength and WebRTC VAD; it does not add a second DSP engine, model download or Krisp. Build-time `cc` and `webrtc-sys-build` use the same headers, defines and artifact as `libwebrtc`; keep their pins aligned when upgrading. `webrtc-sys` and `webrtc-sys-build` are patched to the zirkumflex-group fork (see `[patch.crates-io]` in `Cargo.toml`); an upgrade means rebasing that fork branch onto the new release tag. Native errors stop the audio session rather than bypassing the selected speech gate.

Validation of transmission controls: 91 ordinary Rust tests and three packaging tests passed, with three opt-in network/media tests ignored. Mac/Windows all-target Clippy and optimized Mac/Windows builds passed. Tests cover old two-key profiles, duplicate/optional PTT bindings, press/release behavior, permission overrides, bounded pre-roll/hangover, actual native speech detection and suppression-strength differences, utterance markers, and status delivery with a busy UI or performance mode. Physical keyboard holds, real conversations and native UI interaction were not tested for this update.

The controls were rebuilt for the Rune interface on 2026-11-06: switches and a strength picker under **Microphone Processing**, a mode picker with a release-delay or threshold slider under **Voice Transmission**, and a **Push to talk** field under **Keyboard Shortcuts**. Onboarding's device step shows the same audio section. The voice panel's green edge now lights only while the transmission gate is open. Preview scenes: `audio-processing`, `push-to-talk` and `push-to-talk-connected`.

## Earlier onboarding validation

74 ordinary tests, the three packaging tests, Mac/Windows cross-target Clippy and both optimized builds passed. Automated checks cover setup order, required username/server input, interrupted setup, device persistence, failed-write and invalid-hotkey handling, old/corrupt config behavior, and the unified view's sampling/streaming visibility. Native permission dialogs, device playback and visual layout were not exercised through computer-use automation, per the user's request to stop that testing.
