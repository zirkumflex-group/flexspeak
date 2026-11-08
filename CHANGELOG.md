<!-- Compiled into the app and shown under Settings → Updates, newest first. Each release build
adds itself and any build missing here from its commit's New:/Fixed:/Changed: lines
(`scripts/release.py`), so this file only needs editing to reword an entry. -->

# v0.1.0-build.29 · 2026-11-08

## Fixed

- Someone sharing several streams at once now shows all of them, not just the newest

# v0.1.0-build.28 · 2026-11-08

## New

- Kick from channel, kick from server and ban people from their right-click menu, with a reason
- Tick server groups and set a channel group from the right-click menu, for one person or a whole selection

# v0.1.0-build.27 · 2026-11-08

## New

- A round app icon, and the Flexspeak mark beside the name in the title bar
- Hover CPU in the status bar to see the load as a share of all cores, as Task Manager shows it

## Changed

- Screen shares show at their own resolution and frame rate instead of at most 720p and 30 FPS
- Flexspeak uses noticeably less CPU while you're connected and watching streams
- Clearer buttons to make a stream large and to go back to the grid

# v0.1.0-build.26 · 2026-11-08

## New

- See what changed in each version under Settings → Updates, back to the first build

## Fixed

- Opening Settings for the first time no longer freezes the window

# v0.1.0-build.25 · 2026-11-08

## New

- Drag the splitter above the server log to resize it, or fold it with the chevron; it stays as you left it

## Changed

- The sidebar and main pane are rounded cards on the window background
- The main pane opens with the current channel and its member count
- The footer is a slim status bar with loss in, loss out and ping, then CPU and RAM
- The voice panel's buttons share its width, and speaking shows in green
- On Windows, the caption buttons look like Windows 11's and the window has rounded corners

# v0.1.0-build.24 · 2026-11-08

## New

- Server log above the voice controls, with who connected, moved, left, was kicked or banned, and who went live, under hour headings
- Search the server log and filter it to one person from the magnifier that appears on hover
- Soft sound pack, with its own character for each kind of action
- Sound pack menu that sets every sound at once
- Sounds for connecting, disconnecting, losing and regaining the connection, people joining or leaving your channel, starting and stopping your stream, and being kicked or banned
- Each sound cue can use Flexspeak's sound, the Soft pack, TeamSpeak's sound if TeamSpeak is installed, or none
- One volume control for all sound cues
- The share dialog shows a still preview of the chosen camera, and of screens and windows on Windows
- Sharing starts with the quality, access and sound you last used

## Changed

- You are sharing and requests to watch moved next to the mic and disconnect buttons
- The streams pane is taller; its title is gone and its controls appear when you hover over the streams

# v0.1.0-build.23 · 2026-11-08

## New

- Flexspeak follows your Omarchy theme, including themes you installed yourself
- Sound cues when you mute, unmute, deafen or undeafen
- Right-click someone to set their volume or mute them, just for you
- Right-click a server in the left rail to disconnect from it
- An Open Folder button for adding your own themes

## Fixed

- Dragging someone onto a channel now drops them on the channel under your cursor
- Searching Settings for a theme collection shows only that collection

## Changed

- More Themes shows which themes are light or dark
- Right-click menu icons are coloured

# v0.1.0-build.22 · 2026-11-08

## New

- Tokyo Night, Catppuccin and Omarchy themes under Settings → Appearance
- Add your own themes in the settings file, in T3 Code's theme format
- A switch to disable animations under Settings → Appearance
- Hover the update notice to see what changed

## Changed

- Shortcuts can now be a single key

# v0.1.0-build.21 · 2026-11-08

## New

- Connect to several servers at once, with a rail down the left showing each one's mic and sound state, who's talking, unseen pokes and connection progress
- Each server keeps its own mute and deafen; every server that isn't deafened is heard, and your microphone goes only to the active one

## Changed

- Keyboard shortcuts act on the active server
- Watching streams follows the active server, and your own share keeps running when you switch
- Errors from a background server name it
- Restarting after an update reconnects every server

# v0.1.0-build.20 · 2026-11-08

## New

- Record a shortcut by pressing Change and then the keys you want
- Setup can carry over your display name, saved servers, audio devices and push-to-talk from TeamSpeak 6

## Fixed

- Restarting after an update no longer shows an "already in channel" error

# v0.1.0-build.19 · 2026-11-08

## New

- Keep watching streams while you share your own
- Watch up to five streams at once
- Running streams you haven't joined appear as boxes with Watch in the centre and the stream's details below
- Switch between a grid and one large stream with the others below or beside it
- Drag a stream onto another to trade their places

## Changed

- The grid picks the column count that gives the largest pictures for the space it has

# v0.1.0-build.18 · 2026-11-08

## Fixed

- Official TeamSpeak 6 clients can now watch Flexspeak streams
- Stream bitrate reaches the server correctly

# v0.1.0-build.17 · 2026-11-07

## New

- The Windows window has its own title bar, with minimize, maximize and close in the app
- Choose presets for sharing, or set resolution, frame rate, video and audio bitrate separately
- Share a camera on Mac and Windows
- Share publicly or privately

## Fixed

- The Mac app has its icon again

## Changed

- The Mac installer opens with Flexspeak, an arrow and Applications
- Statistics label packet loss in and out, show no idle chart for yourself, and are the only place private memory appears

# v0.1.0-build.16 · 2026-11-07

## New

- Right-click someone to poke them or move them to a channel
- Select several people with ⌘ or Ctrl-click, and drag them onto a channel to move them
- Pokes play a chime, raise a system notification and open a dialog
- Watch up to four streams at once, in a grid or with one on the stage, each with its own volume and mute
- Start sharing from a dialog with the source, quality and system audio
- Flexspeak offers to install itself when run from Downloads, a disk image or an unpacked ZIP, so updates persist
- Run setup again from Settings → Settings File

## Changed

- Performance mode is gone

# v0.1.0-build.14 · 2026-11-07

## New

- Typing suppression for the microphone, under Settings → Audio

# v0.1.0-build.13 · 2026-11-06

## New

- Settings opens as a window over the app, with search and categories
- Save several servers and connect to any of them from the home screen

## Changed

- The app is frosted behind Settings, and its search field sits in a ring of liquid metal

# v0.1.0-build.12 · 2026-11-06

## Changed

- Release build changes only; nothing changes in the app

# v0.1.0-build.11 · 2026-11-06

## Changed

- Release build changes only; nothing changes in the app

# v0.1.0-build.10 · 2026-11-06

## New

- Flexspeak updates itself: it checks for new builds, downloads and verifies them, and installs when you choose Restart
- After an update, Flexspeak returns to your server and channel with mute and deafen as they were
- Light and dark themes with a choice of five palettes
- Push-to-talk and voice activation
- Noise suppression, automatic gain and echo cancellation for the microphone, on Mac and Windows

## Changed

- Settings and setup show each permission's live status, with a Give Access button when one is missing

# v0.1.0-build.7 · 2026-11-06

## New

- A ring shows who is speaking
- A red icon marks anyone sharing a stream
- An eye switches between seeing every channel's members and only your own
- Hide the footer down to a slim bar, and Flexspeak remembers
- CPU and memory readings turn amber and then red under heavy load

## Changed

- Connection health is a globe in the health colour
- The voice controls moved to the bottom of the sidebar
- The server and channel names are no longer repeated
- The Windows title bar shows the Flexspeak icon

# v0.1.0-build.6 · 2026-11-06

## Changed

- A new desktop-style interface: compact controls, flat grouped surfaces, a neutral dark palette, and Settings as a grouped form

# v0.1.0-build.5 · 2026-11-01

## New

- First-run setup for your name, server, permissions, audio devices and shortcuts
- A home screen of saved servers, and a connected view with channels on the left and streams on the right
- Share your screen or a window, with sound, on Mac and Windows; you approve each viewer

# v0.1.0-build.4 · 2026-10-27

## New

- See the health of your channel's connections
- Watch streams in the app

## Changed

- More reliable error handling

# v0.1.0-build.3 · 2026-10-27

## New

- Precise CPU readings
- Status icons in both view modes

# v0.1.0-build.2 · 2026-10-27

## New

- First release: a native TeamSpeak voice client with audio, statistics and global controls, for Windows, Apple Silicon Macs and Linux, including packages for Omarchy
