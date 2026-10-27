# Flexspeak design system

The implementation lives in [`src/appearance.rs`](../src/appearance.rs). Use its shared constructors and styles rather than adjusting individual buttons.

## Surfaces

| Token | Color | Use |
| --- | --- | --- |
| `CANVAS` | `#10141c` | Main window background |
| `SIDEBAR` | `#171e29` | Sidebar, navigation, footer, inset chart rows |
| `SURFACE` | `#1e2836` | Content cards and secondary controls |
| `RAISED` | `#29364a` | Active navigation, menus, floating details |
| `OUTLINE` | `#334258` | One-pixel borders and separators |
| `TEXT` | `#e6edf7` | Primary text |
| `MUTED` | `#9baac1` | Supporting text and disabled field values |
| `ACCENT` | `#85a7ff` | Primary actions, focus and selection |

All rectangular UI uses **0 corner radius**. System window decoration, icons and health dots are separate from widget borders.

## Buttons

Sizes are logical pixels, independent of display scaling.

| Size | Height | Text | Icon | Horizontal padding | Use |
| --- | ---: | ---: | ---: | ---: | --- |
| Standard | 36 | 14 | 16 | 12 per side | Voice controls, settings, dialogs, navigation, trace actions |
| Compact | 28 | 12 | — | 8 per side | Small disclosure controls |
| Channel row | 32 | 13 | 16 | 8 per side | Clickable channel heading; square hover border |
| Icon only | 36 × 36 | — | 18 | Centered | Top-right utilities, sidebar audio/connection controls, narrow navigation |

- `label_button` and `icon_button` share the same standard geometry and vertically centered content. Width follows the label, with equal side padding.
- `icon_only_button` centers its glyph in a standard square target. It needs a descriptive tooltip and uses the active navigation style for the selected page.
- `compact_button` handles small disclosures. Channel headings use `CHANNEL_ROW_HEIGHT` and `channel_row`, with a separate disclosure target; clicking the name joins the channel.
- `primary_button` accepts composed content, such as a client row, while retaining standard height and alignment.
- Primary, secondary and danger variants change appearance, not geometry. Rectangular borders use zero radius; filled and outlined actions have the same one-pixel border width.
- Related controls use an 8-pixel gap. Disconnect, mute and deafen form one icon-only row above the sidebar server name, with state styling and tooltips.
- Do not override per-button padding or text size to fit a label. Use the shared size or let a row wrap.

## Layout and feedback

Use cards to group one task: connection, audio, shortcuts, or a set of measurements. Page insets are 24–28 pixels; card insets are 12–20 pixels, depending on density. Headings use 18–30 pixels, body text 13–14, and supporting labels 10–12.

The header has a one-pixel bottom border and icon/text Voice and Streaming tabs; their labels collapse in narrow windows and retain hover tooltips. There is no duplicate product wordmark. The left side contains channel navigation, separated into channel groups with indented client rows, no avatars or user counts, and mute/deafen icons only when applicable. Health dots precede client names; hovering explains degraded/missing metrics. Expandable peer measurements remain in the right-side content without repeated channel labels. The fixed footer places own connection health at the left, then a single row of four small line charts (loss in/out, ping, idle), with process resources stacked at the right. Footer charts are 20 logical pixels high with a compact label/value row above and a minimum width of 110 logical pixels each. The graph strip expands into available space, then scrolls horizontally when its 476-pixel minimum width does not fit; the health indicator and process resources remain fixed. The strip reserves a thin scrollbar within its 44-pixel height; the connected footer uses 8-pixel vertical and 14-pixel horizontal padding. It stays outside scrolling content on every normal-mode tab. The overall-health tooltip lists only degraded metrics; each chart tooltip explains its own reading, health and history. Stale or unavailable data must not appear healthy. Performance mode omits the paused connection graphs and places resources above the health indicator to fit the 440×320 window.

Health colors are semantic: green Good, amber Fair, red Poor, neutral Unknown. Idle time is neutral. Connection histories use continuous lines and a translucent ping-variation band, with gaps for missing data. Static surfaces and cached charts add no animation loop or audio-callback work.

The native Mac preview was checked for matching standard-button heights and centered text/icons, the voice toolbar's common right edge, and the inset Joined/Join controls. Final Clippy, formatting and the macOS release build passed. Changes remain local; the active voice session was not restarted.
