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
| `canvas` | `#1c1d20` | Window background |
| `chrome` | `#232427` | Header, sidebar and footer |
| `surface` | `#26272b` | Groups and cards |
| `raised` | `#34353a` | Controls, selected navigation |
| `hairline` | white 8% | Dividers and group edges |
| `text` / `muted` / `faint` | `#ebebee` / `#a0a1a8` / `#75767e` | Primary, supporting and tertiary text |
| `accent` | `#8ea8ff` | Default buttons, focus, selection, the current channel |
| `good` / `fair` / `poor` / `unknown` | `#6fdc9a` / `#f2c46b` / `#ff7a8a` / `#8c95ab` | Health, and nothing else |
| `live` | `#ff7a3d` | Streams, sharing and trace recording; used sparingly |
| `cpu` / `ram` | `#46cfe0` / `#a98ff8` | Resource charts and bars |

rune-kit components (fields, pickers, switches, checkboxes, dialogs, scroll bars) are themed from the same palette by `Theme::kit`.

## Shape

Content surfaces are flat: a `surface` fill and a 1 pt `hairline` edge (`widgets::card`, `widgets::group`). There are no drop shadows or halos on content; elevation belongs to things that float above the window (menus, popovers, dialogs). Recessed **wells** (`widgets::well`) hold charts and code-like details.

| Radius | Points | Use |
| --- | ---: | --- |
| `CARD` | 10 | Groups, cards and dialogs |
| `CONTROL` | 6 | Buttons, fields, icon buttons |
| `ROW` | 6 | Channel and member rows |
| `PILL` | full | Status pills |

## Type

Inter, bundled by Rune. `DISPLAY` 22/650 (onboarding only), `TITLE` 17 for a pane title, `HEADING` 13/600 for group titles and the header, `BODY` 13, `LABEL` 13 for controls, `SMALL` 11.5 and `CAPTION` 11 for supporting text, `OVERLINE` 11/600 for sidebar section labels (sentence case). Numbers use tabular figures (`VALUE`, `BIG_VALUE`, `NUMERIC`). Button labels use title case.

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

Settings is a modal panel over the window (`settings::window`), not a page: a frosted scrim (the app behind blurred at σ 18 pt under black at 38% in dark palettes, 16% in light), then a panel up to 1040 × 800 pt with a 28 pt margin (16 pt below a 900 pt window), a `CARD`-plus radius of 12, a hairline edge and the window's two-layer floating shadow. The sidebar (232 pt, 204 pt when narrow) is `chrome` and holds the search field, framed by a 3.5 pt ring of flowing liquid-metal chrome (rune-fx, drawn by `ui::fx::Effects`), and categories grouped under caption headings; the selected category has a `raised` fill and strong label. The content side is `canvas`, with a 52 pt header (breadcrumb, save confirmation, **Save**, close) over a scrolling column of sections at most 700 pt wide. Each section is a `group_title`, a `group` of `form_row`s (or open content such as the appearance cards) and an optional `group_note`. Rows carry search terms; a query keeps the rows it matches, or a whole section when it names it, and dims categories without a match. Menus opened inside the window draw above it. Onboarding and performance mode have no header; a band across the top drags the window there, and onboarding's steps sit directly on the window.

Disconnected, a saved-servers card (at most 560 pt wide) is centred: one 52 pt row per server, each a `wash(0.03)` tile with a hairline edge, a server icon, name over address, and its own Connect. The server in use gets an `accent` tint and border; the list scrolls past 300 pt. Connected, the 260 pt sidebar holds the server, the voice panel (mute, deafen, disconnect and the microphone meter) and the channel tree; streaming fills the right. Channel rows are 26 pt with a separate disclosure chevron; members are indented with a health dot and mute icons only when muted.

The footer is fixed on every normal page: own health pill on the left, then four 20 pt connection charts (at least 110 pt each; the strip scrolls sideways below that), then CPU, RAM and footprint in tabular figures. Performance mode stacks the readings above the health pill to fit the 440 × 320 window.

## Motion

Springs, not curves: `QUICK` (0.22 s) for hover, `SETTLE` (0.38 s, slight bounce) for panels, `READING` (0.45 s) for colours following data, such as health. Readings that change every second or faster (meters, resource figures, Statistics bars) step without a spring, so a call or an open Statistics page does not keep the window drawing. Nothing animates on the first frame, and Reduce Motion is honoured by Rune.

- Pages swap with a short cross-fade.
- The saved-server card fades in on launch.
- The voice panel's edge turns green while you are heard, easing only as that starts or stops.
- The search field's liquid-metal ring flows while Settings is open, so the window keeps drawing only then; under Reduce Motion it rests on one pose.
- Pulsing dots mark work in progress: connecting, pending server audio, live streams, recording.
- Members, stream rows, viewer requests and peer details fade in and out as they appear and leave.

Idle windows draw nothing; only the 1 s usage tick (10 s in performance mode) and pulse loops (sampled at 12 fps, shown only for short-lived work) request frames.

## Charts

Charts are Rune vector shapes sized from layout. Connection charts show 60 s with health-coloured segments, a translucent ping-variation band and gaps for missing data. CPU/RAM history shows 5 min as a line over a 12% area fill, with the latest reading marked. Stale or unavailable data never appears healthy.

## Previews

`flexspeak --snap out.png --scene <name>` renders a screen headlessly with demo data (`--list-scenes`, `--size 760x560`, `--at 1.5`). `src/ui/scenes.rs` defines the scenes, and its tests build each one at the normal and compact minimum sizes.
