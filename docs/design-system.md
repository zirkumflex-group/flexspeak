# Flexspeak design system

The interface is built with [Rune](https://github.com/anomalyco/rune) and rune-kit. Tokens live in [`src/ui/theme.rs`](../src/ui/theme.rs) and the shared building blocks in [`src/ui/widgets.rs`](../src/ui/widgets.rs). Use those constructors rather than styling individual elements.

## Principles

Flexspeak should read as a desktop app, not a web page: compact controls, flat grouped surfaces, labels beside their controls, and effects only where they carry state. The page title lives in the header, never repeated as a heading in the page.

## Surfaces

Every token comes from one of ten palettes: five themes (Flexspeak, Grove, Ocean, Ember, Iris), each light and dark, chosen in Settings › Color Scheme and Themes. `Theme::new(name, dark)` builds one; views take the current one from `ui::theme(s)` and never name colours themselves. Themes change the tinted neutrals and the accent; status, live and chart colours are shared, with darker variants for light palettes. Hover, press and track overlays use `wash(alpha)`, which is white on dark palettes and black on light ones, and wells use `recess`.

Each palette keeps primary text at 7:1 or better, muted text and the accent (including on its own 16% selection tint, and white on accent buttons) at 4.5:1, and faint text and status colours at 3:1 against cards and chrome. Check a new or changed palette against those ratios before shipping it.

The Flexspeak dark palette:

| Token | Color | Use |
| --- | --- | --- |
| `canvas` | `#1c1d20` | Window background: title bar, server rail and status bar |
| `chrome` | `#232427` | Header, sidebar and footer |
| `surface` | `#26272b` | Groups, cards, the connected sidebar and Statistics |
| `stage` | `#151618` | The stream pane; derived from `canvas` (42% toward black) on dark palettes, `chrome` on light ones |
| `raised` | `#34353a` | Controls, selected navigation |
| `hairline` | white 8% | Dividers and group edges |
| `text` / `muted` / `faint` | `#ebebee` / `#a0a1a8` / `#75767e` | Primary, supporting and tertiary text |
| `accent` | `#8ea8ff` | Default buttons, focus, selection, the current channel |
| `good` / `fair` / `poor` / `unknown` | `#6fdc9a` / `#f2c46b` / `#ff7a8a` / `#8c95ab` | Health, and nothing else |
| `live` | `#ff5a52` | Streams, sharing and trace recording that are running; used sparingly |
| `private` | `#d2a8ff` | Private messages, wherever chat shows one: tabs, tags, badges and the handle's dot |
| `cpu` / `ram` | `#46cfe0` / `#a98ff8` | Resource charts and bars |

rune-kit components (fields, pickers, switches, checkboxes, dialogs, scroll bars) are themed from the same palette by `Theme::kit`.

## Shape

The connected window is built from panels, not cards: they run edge to edge between the title bar and the status bar, with one `hairline` where two meet, as a desktop app's do. Avoid boxes inside boxes and cards floating on the window; reach for a hairline, a heading or spacing first. Rounded `surface` boxes with a hairline edge (`widgets::card`, `widgets::group`) are for things that stand alone: the saved-servers card, Settings groups, dialogs and stream tiles. There are no drop shadows or halos on content; elevation belongs to things that float above the window (menus, popovers, dialogs). Recessed **wells** (`widgets::well`) hold charts and code-like details.

| Radius | Points | Use |
| --- | ---: | --- |
| `PANE` | 12 | The hidden rail's peek |
| `CARD` | 10 | Groups, cards, dialogs and stream tiles |
| `CONTROL` | 6 | Buttons, fields, icon buttons |
| `ROW` | 6 | Channel and member rows |
| `PILL` | full | Go-live chips and small tags |

## Type

Inter, bundled by Rune. `DISPLAY` 22/650 (onboarding only), `TITLE` 17 for a pane title, `HEADING` 13/600 for group titles and the header, `BODY` 13, `LABEL` 13 for controls, `SMALL` 11.5 and `CAPTION` 11 for supporting text, `OVERLINE` 11/600 for sidebar section labels (sentence case). Numbers use tabular figures (`BIG_VALUE`, `NUMERIC`). Button labels use title case.

## Controls

| Control | Size | Notes |
| --- | --- | --- |
| `button` | 28 high, 12 side padding | Optional 14 pt leading icon. Variants: Primary, Secondary, Danger, Ghost, Active. Sized to its label, never stretched across a card |
| `icon_button` | 28 × 28, 16 pt icon | Always has a help tooltip, which also names it for screen readers |
| Fields | 28 high | rune-kit `TextInput` |
| Pickers | 28 high | rune-kit `Select` |

Primary is the default action of a view (Connect, Save, Continue) and is the window's default button where it makes sense. Disabled controls keep their geometry and fade to 42%. Related controls use an 8 point gap; a group's actions align to its trailing edge. Controls show a keyboard-only focus ring in the accent colour.

## Forms

Settings-style content uses `widgets::group` with `widgets::form_row`: the label (and an optional one-line explanation) leads, the control trails at `FORM_CONTROL` (300 pt) width, and rows are separated by inset hairlines. A `group_title` sits above each group and an optional `group_note` below it. Rows wrap the control under its label when the window is narrow. Keep explanatory text to one short sentence; longer detail belongs in a tooltip.

## Layout

The header (52 pt) is also the macOS titlebar: the traffic lights sit centred in it and the header drags the window. It holds the back button on utility pages, the page or server title, the performance-mode switch, and Statistics/Settings. Escape returns to the server.

### Settings window

Settings is a modal panel over the window (`settings::window`), not a page: a frosted scrim (the app behind blurred at σ 18 pt under black at 38% in dark palettes, 16% in light), then a panel up to 1040 × 800 pt with a 28 pt margin (16 pt below a 900 pt window), a `CARD`-plus radius of 12, a hairline edge and the window's two-layer floating shadow. The sidebar (232 pt, 204 pt when narrow) is `chrome` and holds the search field, a plain `TextInput` styled like every other field, and categories grouped under caption headings; the selected category has a `raised` fill and strong label. The content side is `canvas`, with a 52 pt header (breadcrumb, save confirmation, **Save**, close) over a scrolling column of sections at most 700 pt wide. Each section is a `group_title`, a `group` of `form_row`s (or open content such as the appearance cards) and an optional `group_note`. Rows carry search terms; a query keeps the rows it matches, or a whole section when it names it, and dims categories without a match. Menus opened inside the window draw above it. Onboarding and performance mode have no header; a band across the top drags the window there, and onboarding's steps sit directly on the window.

Disconnected, a saved-servers card (at most 560 pt wide) is centred: one 52 pt list row per server, parted by inset hairlines, each led by the server's coloured initials tile (the same one the rail and sidebar show), name over address, and its own Connect. The server being connected gets an `accent` tint; the list scrolls past 300 pt. Connected, the window is the title bar, server rail and status bar on `canvas`, with two panels between them, edge to edge: the 260 pt sidebar on `surface` and the stream pane on the darker `stage`, parted by a full-height hairline. The rail marks the server on screen with a 3 × 24 pt accent pill and keeps each server's own mic and sound state under its tile. The sidebar opens with the server's tile and name (`SUBTITLE`, 14/620) and the every-channel toggle, lit only while it narrows the tree to your channel, then the channel tree with no heading of its own, the server log under a hairline, and the voice bar at the foot of the sidebar with no frame or rule of its own: four buttons share its width at 32 pt high, over the microphone meter, which shows when you are heard. With one server the rail hides behind a thin handle on the window's left edge; it is a handle, not a status light, and stays grey while people talk. Muted microphone and deafened sound use the `Danger` variant, Disconnect the solid `Hangup` variant. A 9 pt splitter above the log resizes it (two rows up to 60% of the sidebar), folds it to its heading when dragged below one row or double-clicked, and its chevron folds it too; the size and fold are saved straight away. Channel rows are 26 pt with a separate disclosure chevron. A channel's name is `accent` while you are in it, `text` with people in it, `muted` when empty and `faint` when you can't join it, so where people are reads first. Subchannels step in 14 pt and members 32 pt, which lines a channel's members up with its subchannels, one level below its own name. A member row leads with a speaking ring that fills `good` green while heard, or a mute icon in its place; signal bars, a stream icon and your own mute of them trail the name only when they apply. Mute icons are two-tone, as TeamSpeak draws them (`widgets::struck`): the glyph in `muted` with only its strike or cross in `poor`, so a tree full of muted people doesn't turn red. The rail's mic and sound icons under each server tile are drawn the same way. The main pane opens with the current channel (`TITLE`) and its member count. With no stream it shows one muted line and the ways to start one, with no icon tile or fine print (the streaming rules are the line's tooltip): a Primary button for the main screen and a Secondary one for the game being played, each live with one click, then More Options for the share dialog (alone, as Share Screen, when there is no quick source). While your share is the only one, the line says so over its viewers. A lone stream fills the pane edge to edge with no box, as a video player does, and crops up to 2% of the picture rather than leave hairline bars when the pane is a hair off its shape; its name, volume and Stop float over the bottom in a frosted bar (the same as the toolbar at the top right) while the pointer is over it or while it connects. Several streams sit 12 pt in from the pane's edges. The stream on the stage has a hide button (panel-close icon) that tucks the others away, watched or not: the stage stream then shows as a lone one, with a small Secondary "N more streams" tab above its bar that brings them back; like the bar, it shows only while the pointer is over the stream, so nothing covers the picture. Switching to the grid or choosing a layout shows them again. Streams fill the pane otherwise, and going live with one click moves to plain chips above the voice buttons, under a Go live label; the `live` colour is kept for what is running. Statistics fills the window between title bar and status bar on `surface`: the CPU and RAM charts straight on the panel, then the readings as two plain tables (Connection, Transfers) with hairlines between rows, and peers as plain hover rows; nothing on it is boxed except wells.

The chat column (`ui/chat.rs`) is a third panel on `surface` to the right of the stage, 280 to 340 pt wide, parted by a hairline. One colour per place, everywhere: the channel is `accent`, the server `fair`, a private message `private`. A 40 pt bar leads: in Tabs, one tab per conversation with the open one underlined in its colour; in One Feed, filter chips. Lines rest on the message box, newest last: a 26 pt round initial, the name, the time in tabular figures, and the text with web addresses as links. One Feed adds a tinted tag after the name and sets private lines on a `private` tint. A poke is history, not chat: a bell in the avatar's place, `faint` text and the note in italics, at 85%. The message box is one recessed frame: a 28 pt **To** line (a glyph or the person's initial, the name in the place's colour, and who reads it) over a borderless field, edged in the place's colour while it has the keyboard. A channel you have left keeps a `faint` tab; open, its To line offers Join and the field is locked. Grouped, the bar holds one tab per kind, each showing the open conversation of its kind and a chevron; its list hangs from it on `raised`, as the rail's peek does, 30 pt rows with a check on the open one. Hidden, the column leaves the rail's handle on the right edge, with a 9 pt dot above it for anything unread, and peeks as the rail does, on `raised` with the same shadow. A message that arrives then is a notice card: the sender's initial, a tag, the text, and Reply (Primary) beside Open Chat; a private one adds a `private` outline. Unread counts are 16 pt pills in the place's colour. Setup and Settings › Chat choose the layout from two cards holding an inert sample of each.

The footer is a 40 pt status bar on `canvas`, fixed on every normal page: own health on the left as a dot and a word in its colour, like the readings beside it rather than a badge, then loss in, loss out and ping on one line each with a 56 × 16 pt sparkline (the strip scrolls sideways when narrow), then CPU and RAM in tabular figures and the collapse chevron. On Windows 11 the borderless window asks DWM for rounded corners. Its caption buttons are 46 pt wide with Windows 11's thin 10 pt glyphs (`CAPTION_*` icons; Maximize becomes Restore while maximized) and the system red behind a white glyph on Close; the title bar's own icons use a 1.5 pt stroke to match. Performance mode stacks the readings above the health dot to fit the 440 × 320 window.

## Motion

Springs, not curves: `QUICK` (0.22 s) for hover, `SETTLE` (0.38 s, slight bounce) for panels, `READING` (0.45 s) for colours following data, such as health. Readings that change every second or faster (meters, resource figures, Statistics bars) step without a spring, so a call or an open Statistics page does not keep the window drawing. Nothing animates on the first frame, and Reduce Motion is honoured by Rune.

- Pages swap with a short cross-fade.
- The saved-server card fades in on launch.
- The search field's liquid-metal ring flows while Settings is open, so the window keeps drawing only then; under Reduce Motion it rests on one pose.
- Pulsing dots mark work in progress: connecting, pending server audio, live streams, recording.
- Members, stream rows, viewer requests and peer details fade in and out as they appear and leave.

Idle windows draw nothing; only the 1 s usage tick (10 s in performance mode) and pulse loops (sampled at 12 fps, shown only for short-lived work) request frames.

## Charts

Charts are Rune vector shapes sized from layout. Connection charts show 60 s with health-coloured segments, a translucent ping-variation band and gaps for missing data. CPU/RAM history shows 5 min as a line over a 12% area fill, with the latest reading marked. Stale or unavailable data never appears healthy.

## Previews

`flexspeak --snap out.png --scene <name>` renders a screen headlessly with demo data (`--list-scenes`, `--size 760x560`, `--at 1.5`). `src/ui/scenes.rs` defines the scenes, and its tests build each one at the normal and compact minimum sizes.
