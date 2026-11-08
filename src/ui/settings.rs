//! The Settings window, and the sections onboarding reuses: audio, permissions and shortcuts.
//!
//! Settings floats over the window as a modal panel: categories and search lead, the chosen
//! category's sections trail. Every section is a list of rows with search terms, so a query
//! narrows the panel to the rows that match across all categories.

use rune::{Color, Cx, El, Icon, Tx, col, el, icon, row, text};
use rune_kit::{Kit, Switch, TextInput};

use super::theme::{Theme, motion, radius, shadow, size, ty};
use super::widgets::{
    Range, Variant, button, divider, field, form_row, group, group_note, group_title, icon_button,
    labelled, meter, muted, percent_slider, picker, scroll_page, slider,
};
use super::{dispatch, icons, send, updates};
use crate::settings::{Appearance, ColorScheme, ThemeName};
use crate::{App, Message, SettingsPage, audio_processing, permissions, shortcuts, transmission};

/// A trailing control at the form's control width.
fn trailing(e: El) -> El {
    el().w(size::FORM_CONTROL).child(e)
}

/// Buttons that act on a whole group, aligned to its trailing edge.
fn actions(buttons: impl IntoIterator<Item = El>) -> El {
    row()
        .w_full()
        .px(size::CARD_PAD)
        .py(10.0)
        .gap(size::GAP)
        .justify_end()
        .flex_wrap()
        .children(buttons)
}

/// One row of a section and the words that find it.
struct Row {
    terms: String,
    el: El,
}

fn item(terms: impl Into<String>, el: El) -> Row {
    Row {
        terms: terms.into(),
        el,
    }
}

/// A titled run of rows. Boxed sections sit in one grouped card; open ones (the appearance
/// pickers) stand on the page.
struct Section {
    key: &'static str,
    title: &'static str,
    /// Words that find the whole section, beyond its title.
    terms: &'static str,
    rows: Vec<Row>,
    note: Option<El>,
    boxed: bool,
}

impl Section {
    fn boxed(key: &'static str, title: &'static str, terms: &'static str, rows: Vec<Row>) -> Self {
        Self {
            key,
            title,
            terms,
            rows,
            note: None,
            boxed: true,
        }
    }

    fn open(key: &'static str, title: &'static str, terms: &'static str, rows: Vec<Row>) -> Self {
        Self {
            boxed: false,
            ..Self::boxed(key, title, terms, rows)
        }
    }

    fn note(mut self, note: El) -> Self {
        self.note = Some(note);
        self
    }

    /// The section with only the rows `query` finds; `None` when it finds none. A query that
    /// names the section keeps every row.
    fn render(self, t: &Theme, query: &str) -> Option<El> {
        let whole = matches(&format!("{} {}", self.title, self.terms), query);
        let rows: Vec<El> = self
            .rows
            .into_iter()
            .filter(|row| whole || matches(&row.terms, query))
            .map(|row| row.el)
            .collect();
        if rows.is_empty() {
            return None;
        }
        let mut e = col()
            .key(self.key)
            .w_full()
            .gap(8.0)
            .child(group_title(t, self.title));
        e = if self.boxed {
            e.child(group(t, rows))
        } else {
            e.children(rows)
        };
        if let Some(note) = self.note {
            e = e.child(note);
        }
        Some(e)
    }
}

/// Whether every word of `query` appears in `haystack`, ignoring case. An empty query finds all.
fn matches(haystack: &str, query: &str) -> bool {
    let haystack = haystack.to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

fn page_icon(page: SettingsPage) -> &'static Icon {
    match page {
        SettingsPage::Server => &icons::SERVER,
        SettingsPage::Appearance => &icons::PALETTE,
        SettingsPage::Audio => &icons::AUDIO_LINES,
        SettingsPage::Shortcuts => &icons::KEYBOARD,
        SettingsPage::Permissions => &icons::SHIELD,
        SettingsPage::Updates => &icons::DOWNLOAD,
        SettingsPage::File => &icons::FILE_TEXT,
    }
}

/// The sidebar's headings: which categories follow each one.
const NAV: &[(Option<&str>, &[SettingsPage])] = &[
    (None, &[SettingsPage::Server, SettingsPage::Appearance]),
    (
        Some("Voice"),
        &[
            SettingsPage::Audio,
            SettingsPage::Shortcuts,
            SettingsPage::Permissions,
        ],
    ),
    (Some("App"), &[SettingsPage::Updates, SettingsPage::File]),
];

fn sections(
    s: &App,
    cx: &mut Cx<App>,
    t: &Theme,
    k: &Kit,
    layer: &mut Vec<El>,
    page: SettingsPage,
) -> Vec<Section> {
    match page {
        SettingsPage::Server => vec![servers_section(s, t), identity_section(s, t, k)],
        SettingsPage::Appearance => appearance_sections(s, t, k),
        SettingsPage::Audio => vec![
            Section::boxed(
                "audio",
                "Devices",
                "audio sound",
                audio_rows(s, cx, t, k, layer),
            )
            .note(group_note(
                t,
                "Disconnect to change devices; volume applies immediately. Use headphones for the microphone test.",
            )),
            Section::boxed(
                "processing",
                "Microphone Processing",
                "audio voice filter",
                processing_rows(s, t, k, layer),
            ),
            Section::boxed(
                "transmission",
                "Voice Transmission",
                "audio talk send",
                transmission_rows(s, t, k, layer),
            )
            .note(group_note(
                t,
                "Stop the microphone test or disconnect from every server to change processing and transmission. Save to keep them.",
            )),
            sound_section(s, t, k, layer),
        ],
        SettingsPage::Shortcuts => vec![shortcut_section(s, t, k)],
        SettingsPage::Permissions => vec![permission_section(s, t)],
        SettingsPage::Updates => vec![
            Section::open(
                "updates",
                "Updates",
                "update upgrade version release download install restart automatic",
                vec![item("", updates::settings(s, t, k))],
            )
            .note(updates::note(t)),
            install_section(s, t),
        ],
        SettingsPage::File => vec![file_section(s, t), setup_section(s, t)],
    }
}

/// The floating Settings window, when it is open. Its menus go above it in `layer`.
pub fn window(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let Some(page) = s.settings_page else {
        return;
    };
    let query = s.settings_query.trim();
    let searching = !query.is_empty();
    let mut menus = Vec::new();
    // Which categories have a row the query finds, for the sidebar to dim the rest.
    let mut found = [true; SettingsPage::ALL.len()];
    let mut content = col()
        .key(if searching { "results" } else { "page" })
        .w_full()
        .max_w(700.0)
        .self_center()
        .padding(size::PAGE)
        .gap(18.0);
    if searching {
        let mut any = false;
        for (i, page) in SettingsPage::ALL.into_iter().enumerate() {
            let rendered: Vec<El> = sections(s, cx, t, k, &mut menus, page)
                .into_iter()
                .filter_map(|section| section.render(t, query))
                .collect();
            found[i] = !rendered.is_empty();
            any |= found[i];
            content = content.children(rendered);
        }
        if !any {
            content = content.child(
                col()
                    .key("no-results")
                    .w_full()
                    .pt(48.0)
                    .gap(6.0)
                    .center()
                    .child(icon(&icons::SEARCH, 22.0).color(t.faint).a11y_hidden())
                    .child(
                        text(format!("No settings match “{query}”"))
                            .font(ty::BODY_STRONG)
                            .color(t.text),
                    )
                    .child(muted(t, "Try a device, a shortcut or a theme name.")),
            );
        }
    } else {
        content = content.children(
            sections(s, cx, t, k, &mut menus, page)
                .into_iter()
                .filter_map(|section| section.render(t, "")),
        );
    }
    // Below the default width, the panel keeps a smaller margin and a narrower sidebar.
    let narrow = s.window_size.0 < 900.0;
    let corner = 12.0;
    let main = col()
        .key("settings-main")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(t.canvas)
        .child(window_header(s, t, page, searching))
        .child(divider(t))
        .child(
            col()
                .key(format!(
                    "settings-body-{}",
                    if searching { "search" } else { page.title() }
                ))
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .transition(rune::Transition::opacity())
                .motion(motion::QUICK)
                .child(scroll_page(k, "settings-scroll", content)),
        );
    let panel = row()
        .key("settings-window")
        .role("dialog")
        .label("Settings")
        .w_full()
        .h_full()
        .max_w(1040.0)
        .max_h(800.0)
        .radius(corner)
        .fill(t.canvas)
        .border(1.0, t.hairline)
        .clip_rounded(corner)
        .box_shadow(shadow(
            0.0,
            24.0,
            64.0,
            Color::BLACK.alpha(if t.dark { 0.5 } else { 0.22 }),
        ))
        .box_shadow(shadow(
            0.0,
            2.0,
            8.0,
            Color::BLACK.alpha(if t.dark { 0.3 } else { 0.08 }),
        ))
        .token_motion(k.motion.overlay)
        .on_key(|s: &mut App, key: &rune::KeyEvent, tx: &mut Tx| {
            // `/` jumps to search, as in the browser; ⌘S saves without leaving the window.
            if key.key == rune::Key::Text("/".into()) && !s.ui.settings_search.focused {
                s.ui.settings_search.focused = true;
                return true;
            }
            if key.is_command('s') {
                dispatch(s, tx, Message::SaveSettings);
                return true;
            }
            false
        })
        .child(sidebar(
            t,
            search_field(s, t, k, cx.is_reduced_motion()),
            page,
            searching,
            &found,
            narrow,
        ))
        .child(el().w(1.0).h_full().fill(t.hairline).a11y_hidden())
        .child(main)
        .modal(true, |s: &mut App| {
            let _ = s.update(Message::CloseSettings);
            s.sync_fields();
        });
    layer.push(
        // The app stays in view behind Settings, frosted rather than only dimmed.
        el().key("settings-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .backdrop_blur(18.0)
            .fill(Color::BLACK.alpha(if t.dark { 0.38 } else { 0.16 }))
            .token_motion(k.motion.overlay)
            .a11y_hidden(),
    );
    layer.push(
        el().key("settings-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(if narrow { 16.0 } else { 28.0 })
            // Windows draws its window buttons over the top band; keep Save and Close clear.
            .pt(if cfg!(windows) {
                size::HEADER
            } else if narrow {
                16.0
            } else {
                28.0
            })
            .center()
            .child(panel),
    );
    layer.extend(menus);
    server_dialog(s, t, k, layer);
}

fn sidebar(
    t: &Theme,
    search: El,
    page: SettingsPage,
    searching: bool,
    found: &[bool],
    narrow: bool,
) -> El {
    let mut nav = col()
        .key("settings-nav")
        .w_full()
        .gap(2.0)
        .role("tablist")
        .label("Categories");
    for (heading, pages) in NAV {
        if let Some(heading) = heading {
            nav = nav.child(
                text(*heading)
                    .font(ty::CAPTION)
                    .color(t.faint)
                    .px(10.0)
                    .pt(14.0)
                    .pb(4.0),
            );
        }
        for &p in *pages {
            let index = SettingsPage::ALL.iter().position(|&q| q == p).unwrap_or(0);
            nav = nav.child(nav_item(t, p, !searching && p == page, found[index]));
        }
    }
    col()
        .key("settings-sidebar")
        .w(if narrow { 204.0 } else { 232.0 })
        .h_full()
        .fill(t.chrome)
        .padding(10.0)
        .gap(6.0)
        .child(search)
        .child(nav)
}

/// The search field, in a ring of flowing liquid metal.
fn search_field(s: &App, t: &Theme, k: &Kit, reduced_motion: bool) -> El {
    const RING: f32 = 3.5;
    let mut input = TextInput::new(&s.ui.settings_search)
        .placeholder("Search")
        .label("Search settings")
        .leading_icon(&icons::SEARCH)
        .width(None)
        .height(size::CONTROL)
        .on_edit(|s: &mut App| &mut s.ui.settings_search)
        .on_change(|s: &mut App, tx: &mut Tx| {
            let value = s.ui.settings_search.text.clone();
            dispatch(s, tx, Message::SettingsSearch(value));
        })
        // Escape empties a search before it closes the window.
        .on_key(|s: &mut App, key: &rune::KeyEvent, tx: &mut Tx| {
            if key.key == rune::Key::Escape && !s.settings_query.is_empty() {
                dispatch(s, tx, Message::SettingsSearch(String::new()));
                return true;
            }
            false
        });
    if s.settings_query.is_empty() {
        input = input.trailing(
            el().key("slash")
                .size(18.0, 18.0)
                .center()
                .radius(4.0)
                .border(1.0, t.hairline)
                .child(text("/").font(ty::CAPTION).color(t.faint))
                .a11y_hidden(),
        );
    }
    let corner = radius::CONTROL + RING;
    el().key("settings-search").w_full().pb(6.0).child(
        el().w_full()
            .padding(RING)
            .radius(corner)
            .child(
                super::fx::liquid_metal_ring(RING, reduced_motion)
                    .key("search-metal")
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    .radius(corner)
                    .a11y_hidden(),
            )
            .child(input.build(k)),
    )
}

fn nav_item(t: &Theme, page: SettingsPage, active: bool, found: bool) -> El {
    let ink = if active { t.text } else { t.muted };
    let mut e = row()
        .key(format!("nav-{}", page.title()))
        .role("tab")
        .label(page.title())
        .checked(active)
        .w_full()
        .h(32.0)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .radius(radius::CONTROL)
        .fill(if active { t.raised } else { Color::TRANSPARENT })
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.05))
        .press(t.wash(0.09))
        .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(send(Message::OpenSettings(page)))
        .child(icon(page_icon(page), 15.0).color(ink).a11y_hidden())
        .child(
            text(page.title())
                .font(if active { ty::BODY_STRONG } else { ty::BODY })
                .color(ink)
                .truncate()
                .a11y_hidden(),
        );
    if !found {
        e = e.dim(0.45);
    }
    e
}

fn window_header(s: &App, t: &Theme, page: SettingsPage, searching: bool) -> El {
    let current = if searching { "Search" } else { page.title() };
    let mut bar = row()
        .key("settings-header")
        .h(size::HEADER)
        .w_full()
        .pl(20.0)
        .pr(12.0)
        .gap(size::GAP)
        .center_y()
        .child(text("Settings").font(ty::BODY).color(t.muted))
        .child(text("/").font(ty::BODY).color(t.faint))
        .child(
            text(current)
                .font(ty::BODY_STRONG)
                .color(t.text)
                .truncate()
                .min_w(0.0),
        )
        .child(el().grow(1.0));
    if let Some(message) = &s.settings_message {
        bar = bar.child(
            labelled(&icons::CHECK, message.clone(), t.good)
                .key("settings-message")
                .transition(rune::Transition::opacity()),
        );
    }
    bar.child(button(
        t,
        "save",
        "Save",
        None,
        Variant::Primary,
        Some(Message::SaveSettings),
    ))
    .child(icon_button(
        t,
        "close-settings",
        &icons::CLOSE,
        "Close · Esc",
        Variant::Ghost,
        Some(Message::CloseSettings),
    ))
}

/// The saved servers, one row each, and Add Server. The list saves as it changes.
fn servers_section(s: &App, t: &Theme) -> Section {
    let mut rows: Vec<Row> = s
        .servers
        .iter()
        .enumerate()
        .map(|(index, server)| {
            let session = s.session_at(&server.address);
            let mut names = col().grow(1.0).min_w(0.0).gap(2.0).child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(
                        text(server.title().to_owned())
                            .font(ty::BODY)
                            .color(t.text)
                            .truncate_fade()
                            .min_w(0.0),
                    )
                    .children(session.map(|session| {
                        badge(
                            t,
                            if session.connected {
                                "Connected"
                            } else {
                                "Connecting"
                            },
                        )
                    })),
            );
            if !server.name.trim().is_empty() {
                names = names.child(
                    text(server.address.clone())
                        .font(ty::CAPTION)
                        .color(t.faint)
                        .truncate_fade(),
                );
            }
            let e = row()
                .key(format!("saved-server-{index}"))
                .w_full()
                .min_h(52.0)
                .px(size::CARD_PAD)
                .py(8.0)
                .gap(12.0)
                .center_y()
                .child(
                    el().size(28.0, 28.0)
                        .radius(radius::CONTROL)
                        .center()
                        .fill(t.raised)
                        .child(icon(&icons::SERVER, 14.0).color(t.accent).a11y_hidden())
                        .a11y_hidden(),
                )
                .child(names)
                .child(
                    button(
                        t,
                        &format!("edit-server-{index}"),
                        "Edit",
                        Some(&icons::PENCIL),
                        Variant::Secondary,
                        Some(Message::EditServer(index)),
                    )
                    .help(format!("Edit {}", server.title())),
                );
            item(format!("server {} {}", server.name, server.address), e)
        })
        .collect();
    rows.push(item(
        "add server new",
        actions([button(
            t,
            "add-server-settings",
            "Add Server",
            Some(&icons::PLUS),
            Variant::Secondary,
            Some(Message::AddServer),
        )]),
    ));
    Section::boxed("servers", "Servers", "saved servers list bookmarks address hostname", rows)
        .note(group_note(
            t,
            "Save as many servers as you like and connect to several at once. Changes here save straight away.",
        ))
}

/// A small pill beside a name: the server in use.
fn badge(t: &Theme, label: &str) -> El {
    text(label.to_owned())
        .font(ty::CAPTION)
        .color(t.accent)
        .px(6.0)
        .py(1.0)
        .radius(radius::PILL)
        .fill(t.accent.alpha(0.14))
}

fn identity_section(s: &App, t: &Theme, k: &Kit) -> Section {
    let idle = s.idle();
    Section::boxed(
        "identity",
        "Identity",
        "you profile",
        vec![item(
            "nickname name username identity",
            form_row(
                t,
                "Nickname",
                Some("Your name on every server."),
                trailing(field(
                    k,
                    &s.ui.nickname,
                    |s| &mut s.ui.nickname,
                    "Nickname",
                    idle.then_some(Message::Nickname as fn(String) -> Message),
                    None,
                )),
            ),
        )],
    )
    .note(group_note(
        t,
        if idle {
            "Save keeps your nickname."
        } else {
            "Disconnect to change your nickname."
        },
    ))
}

/// The Add or Edit Server dialog, above the Settings window.
fn server_dialog(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let Some(editor) = &s.server_editor else {
        return;
    };
    let editing = editor.index;
    let in_use = editing.is_some_and(|index| s.server_in_use(index));
    let labelled_field = |label: &str, detail: Option<&str>, field: El| {
        let mut e = col()
            .w_full()
            .gap(6.0)
            .child(text(label.to_owned()).font(ty::BODY_STRONG).color(t.text));
        e = e.child(field);
        if let Some(detail) = detail {
            e = e.child(super::widgets::faint(t, detail.to_owned()));
        }
        e
    };
    let mut body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(14.0)
        .child(labelled_field(
            "Address",
            Some("A hostname or IP address, with :port when it isn't 9987."),
            field(
                k,
                &s.ui.server_address,
                |s| &mut s.ui.server_address,
                "hostname:9987",
                (!in_use).then_some(Message::ServerAddress as fn(String) -> Message),
                Some(Message::SubmitServer),
            ),
        ))
        .child(labelled_field(
            "Name",
            Some("Optional. Shown instead of the address."),
            field(
                k,
                &s.ui.server_name,
                |s| &mut s.ui.server_name,
                "Friends, Work…",
                (!in_use).then_some(Message::ServerName as fn(String) -> Message),
                Some(Message::SubmitServer),
            ),
        ))
        .child(labelled_field(
            "Password",
            Some("Optional. Kept until you quit Flexspeak; never saved."),
            field(
                k,
                &s.ui.server_password,
                |s| &mut s.ui.server_password,
                "None",
                (!in_use).then_some(Message::ServerPassword as fn(String) -> Message),
                Some(Message::SubmitServer),
            ),
        ));
    if let Some(error) = &editor.error {
        body = body.child(super::widgets::error_text(t, error.clone()));
    } else if in_use {
        body = body.child(super::widgets::muted(
            t,
            "Disconnect from this server to edit or remove it.",
        ));
    }
    let mut buttons = row().w_full().gap(size::GAP).center_y().pt(4.0);
    if editing.is_some() {
        let removable = s.servers.len() > 1 && !in_use;
        buttons = buttons.child(
            button(
                t,
                "remove-server",
                "Remove",
                Some(&icons::TRASH),
                Variant::Danger,
                removable.then_some(Message::RemoveServer),
            )
            .help(if s.servers.len() <= 1 {
                "Keep at least one server"
            } else {
                "Remove this server"
            }),
        );
    }
    buttons = buttons
        .child(el().grow(1.0))
        .child(button(
            t,
            "cancel-server",
            "Cancel",
            None,
            Variant::Secondary,
            Some(Message::CancelServer),
        ))
        .child(
            button(
                t,
                "submit-server",
                if editing.is_some() {
                    "Save"
                } else {
                    "Add Server"
                },
                None,
                Variant::Primary,
                (!in_use).then_some(Message::SubmitServer),
            )
            .default_button(),
        );
    rune_kit::Dialog::new(
        "server-editor",
        if editing.is_some() {
            "Edit Server"
        } else {
            "Add Server"
        },
    )
    .description(if editing.is_some() {
        "Changes save as soon as you choose Save."
    } else {
        "Add it to your saved servers, then connect to it from the home screen or the rail."
    })
    .open(true)
    .fit()
    .width(440.0)
    .on_close(|s: &mut App| {
        let _ = s.update(Message::CancelServer);
        s.sync_fields();
    })
    .child(body.child(buttons))
    .build(k, layer);
}

fn file_section(s: &App, t: &Theme) -> Section {
    let idle = s.idle();
    let location = s
        .settings_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "Unavailable".into());
    let rows = vec![
        item(
            "settings file location path copy json portable",
            form_row(
                t,
                "Location",
                Some(location.as_str()),
                button(
                    t,
                    "copy-path",
                    "Copy Path",
                    Some(&icons::COPY),
                    Variant::Secondary,
                    s.settings_path.as_ref().map(|_| Message::CopySettingsPath),
                ),
            ),
        ),
        item(
            "reload settings file disk",
            form_row(
                t,
                "Reload",
                Some(if idle {
                    "Read the file again after editing it elsewhere."
                } else {
                    "Disconnect to reload the file."
                }),
                button(
                    t,
                    "reload",
                    "Reload",
                    Some(&icons::REFRESH),
                    Variant::Secondary,
                    idle.then_some(Message::ReloadSettings),
                ),
            ),
        ),
    ];
    Section::boxed("settings-file", "Settings File", "storage save", rows).note(group_note(
        t,
        "Save writes your portable settings file. Appearance applies and saves as soon as you choose it.",
    ))
}

/// Running setup again over the saved profile; Cancel in setup keeps everything as it was.
fn setup_section(s: &App, t: &Theme) -> Section {
    let idle = s.idle();
    Section::boxed(
        "setup",
        "Setup",
        "onboarding welcome first run wizard again redo restart",
        vec![item(
            "run setup again onboarding redo",
            form_row(
                t,
                "Run setup again",
                Some(if idle {
                    "Walk through username, server, permissions, devices and shortcuts again."
                } else {
                    "Disconnect to run setup again."
                }),
                button(
                    t,
                    "redo-setup",
                    "Run Setup",
                    Some(&icons::SPARKLES),
                    Variant::Secondary,
                    idle.then_some(Message::RedoSetup),
                ),
            ),
        )],
    )
}

/// Where Flexspeak runs from, and installing it when that place is temporary.
fn install_section(s: &App, t: &Theme) -> Section {
    let running = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "Unknown".into());
    let row = match &s.install_offer {
        Some(offer) => form_row(
            t,
            "Not installed",
            Some(&format!(
                "Running from {}. Install to {} so it and its updates stay put.",
                offer.from.display(),
                offer.to.display()
            )),
            button(
                t,
                "install-now",
                if s.installing {
                    "Installing…"
                } else {
                    "Install"
                },
                Some(&icons::DOWNLOAD),
                Variant::Secondary,
                (!s.installing).then_some(Message::Install),
            ),
        ),
        None => form_row(
            t,
            "Installed",
            Some(&running),
            el().child(icon(&icons::CHECK, 16.0).color(t.good)),
        ),
    };
    Section::boxed(
        "installation",
        "Installation",
        "install location applications program files folder portable",
        vec![item("install installed location", row)],
    )
}

/// The colour scheme, then a theme for light and one for dark. Choices apply and save at once.
fn appearance_sections(s: &App, t: &Theme, k: &Kit) -> Vec<Section> {
    let current = &s.appearance;
    let schemes = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .role("radiogroup")
        .label("Color scheme")
        .children(
            ColorScheme::ALL
                .into_iter()
                .map(|scheme| scheme_card(t, current, scheme)),
        );
    // An empty sixth cell keeps the last row's cards the width of the others.
    let themes = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .children(
            ThemeName::ALL
                .into_iter()
                .map(|name| theme_card(t, current, name)),
        )
        .child(theme_cell().key("spacer").py(0.0).h(0.0).a11y_hidden());
    let theme_names: String = ThemeName::ALL
        .iter()
        .map(|name| name.title())
        .collect::<Vec<_>>()
        .join(" ");
    let mut sections = vec![
        Section::open(
            "scheme",
            "Color Scheme",
            "appearance light dark system mode",
            vec![item("", schemes)],
        ),
        Section::open(
            "themes",
            "Themes",
            "appearance theme accent colour color palette",
            vec![item(theme_names, themes)],
        )
        .note(group_note(
            t,
            "Pick one theme for light and one for dark. System switches between them with your computer.",
        )),
        Section::boxed(
            "motion",
            "Motion",
            "animation animations motion effects reduce",
            vec![item(
                "disable animations",
                form_row(
                    t,
                    "Disable animations",
                    Some("Stops movement and pulsing across the app."),
                    switch(
                        "disable-animations",
                        "Disable animations",
                        current.disable_animations,
                        true,
                        k,
                        |s, disable_animations| {
                            Message::Appearance(Appearance {
                                disable_animations,
                                ..s.appearance.clone()
                            })
                        },
                    ),
                ),
            )],
        ),
        more_themes(s, t),
    ];
    if let Some(current) = &s.omarchy {
        sections.insert(0, omarchy_section(s, t, k, &current.name));
    }
    sections
}

/// Only on an Omarchy desktop, where the app follows the desktop's theme unless told not to.
fn omarchy_section(s: &App, t: &Theme, k: &Kit, name: &str) -> Section {
    let following = !s.appearance.ignore_omarchy;
    let detail = if following {
        format!("Using {name}. Turn this off to choose a theme below.")
    } else {
        "Using the theme chosen below.".to_owned()
    };
    Section::boxed(
        "omarchy",
        "Omarchy",
        "omarchy desktop follow match theme",
        vec![item(
            "match omarchy theme",
            form_row(
                t,
                "Match Omarchy theme",
                Some(&detail),
                switch(
                    "follow-omarchy",
                    "Match Omarchy theme",
                    following,
                    true,
                    k,
                    |s, on| {
                        Message::Appearance(Appearance {
                            ignore_omarchy: !on,
                            ..s.appearance.clone()
                        })
                    },
                ),
            ),
        )],
    )
}

/// The bundled collections and the user's own themes, one group each.
fn more_themes(s: &App, t: &Theme) -> Section {
    let groups = s.theme_library.collections();
    let rows = groups
        .iter()
        .map(|(name, themes)| {
            let grid = row()
                .w_full()
                .gap(10.0)
                .flex_wrap()
                .children(themes.iter().map(|theme| extra_card(s, t, theme)))
                // Empty cells keep a short last row's cards the width of the others.
                .child(
                    theme_cell()
                        .key("spacer-1")
                        .px(10.0)
                        .py(0.0)
                        .h(0.0)
                        .a11y_hidden(),
                )
                .child(
                    theme_cell()
                        .key("spacer-2")
                        .px(10.0)
                        .py(0.0)
                        .h(0.0)
                        .a11y_hidden(),
                );
            let labels: Vec<&str> = themes.iter().map(|theme| theme.label.as_str()).collect();
            item(
                format!("{name} {}", labels.join(" ")),
                col()
                    .key(format!("collection-{name}"))
                    .w_full()
                    .gap(8.0)
                    .child(super::widgets::faint(t, (*name).to_owned()))
                    .child(grid),
            )
        })
        .collect();
    let mut note = col().w_full().gap(4.0).child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(
                col().grow(1.0).min_w(0.0).px(2.0).child(
                    super::widgets::faint(t, "Add your own under custom_themes in settings.json, in T3 Code's theme format, then press Reload on the Settings File page.").wrap().w_full(),
                ),
            )
            .child(button(
                t,
                "open-settings-folder",
                "Open Folder",
                Some(&icons::FOLDER_OPEN),
                Variant::Secondary,
                s.settings_path
                    .as_ref()
                    .map(|_| Message::OpenSettingsFolder),
            )),
    );
    for problem in &s.theme_library.problems {
        note = note.child(super::widgets::error_text(t, format!("Skipped {problem}")));
    }
    Section::open(
        "more-themes",
        "More Themes",
        "appearance theme custom tokyo night catppuccin omarchy",
        rows,
    )
    .note(note)
}

fn extra_card(s: &App, t: &Theme, theme: &crate::themes::Extra) -> El {
    let mut swatches = row().gap(8.0);
    for dark in [false, true] {
        if let Some(colors) = theme.colors(dark) {
            swatches = swatches.child(extra_orb(&s.appearance, t, theme, colors, dark));
        }
    }
    theme_cell()
        .key(format!("extra-{}", theme.id))
        .padding(10.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, t.hairline)
        .child(
            row().w_full().gap(10.0).center_y().child(swatches).child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(1.0)
                    .child(
                        text(theme.label.clone())
                            .font(ty::BODY_STRONG)
                            .color(t.text)
                            .truncate_fade(),
                    )
                    .child(super::widgets::faint(
                        t,
                        match (theme.light.is_some(), theme.dark.is_some()) {
                            (true, true) => "Light and dark",
                            (true, false) => "Light",
                            _ => "Dark",
                        },
                    )),
            ),
        )
}

/// Pressing a swatch uses that theme for its mode. On a fixed scheme it also switches to that
/// mode, so the choice shows at once; System keeps following the computer.
fn extra_orb(
    current: &Appearance,
    t: &Theme,
    theme: &crate::themes::Extra,
    colors: &crate::themes::Colors,
    dark: bool,
) -> El {
    const SIZE: f32 = 30.0;
    let palette = Theme::extra(colors, dark);
    let selected = current.extra_theme(dark) == Some(theme.id.as_str());
    let mode = if dark { "dark" } else { "light" };
    let help = format!("Use {} in {mode} mode", theme.label);
    let mut next = current.clone();
    if dark {
        next.dark_theme_id = Some(theme.id.clone());
    } else {
        next.light_theme_id = Some(theme.id.clone());
    }
    if next.scheme != ColorScheme::System {
        next.scheme = if dark {
            ColorScheme::Dark
        } else {
            ColorScheme::Light
        };
    }
    let mut orb = el()
        .key(format!("{}-{mode}", theme.id))
        .role("radio")
        .label(help.clone())
        .help(help)
        .checked(selected)
        .size(SIZE, SIZE)
        .radius(SIZE / 2.0)
        .fill(palette.surface)
        .radial_gradient(
            [0.62, 0.32],
            SIZE * 0.78,
            [
                (0.0, palette.accent),
                (0.45, palette.accent.alpha(0.55)),
                (1.0, palette.accent.alpha(0.0)),
            ],
        )
        .border(1.0, palette.hairline)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.08))
        .press(t.wash(0.14))
        .focus_ring(2.0, 3.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(send(Message::Appearance(next)));
    if selected {
        orb = orb.outline(2.0, 3.0, t.accent);
    }
    orb
}

fn scheme_card(t: &Theme, current: &Appearance, scheme: ColorScheme) -> El {
    let selected = current.scheme == scheme;
    let light = Theme::new(current.light_theme, false);
    let dark = Theme::new(current.dark_theme, true);
    let preview = match scheme {
        ColorScheme::Light => mini_window(&light),
        ColorScheme::Dark => mini_window(&dark),
        ColorScheme::System => row()
            .w_full()
            .h_full()
            .child(mini_window(&light).w_frac(0.5))
            .child(mini_window(&dark).w_frac(0.5)),
    };
    let mut card = col()
        .key(format!("scheme-{}", scheme.title()))
        .role("radio")
        .label(scheme.title())
        .checked(selected)
        .grow(1.0)
        .w(0.0)
        .min_w(150.0)
        .padding(8.0)
        .gap(8.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, if selected { t.accent } else { t.hairline })
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.04))
        .press(t.wash(0.08))
        .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(send(Message::Appearance(Appearance {
            scheme,
            ..current.clone()
        })))
        .child(
            el().w_full()
                .h(76.0)
                .radius(radius::CONTROL)
                .clip_rounded(radius::CONTROL)
                .border(1.0, t.hairline)
                .child(preview)
                .a11y_hidden(),
        )
        .child(
            text(scheme.title())
                .font(if selected { ty::BODY_STRONG } else { ty::BODY })
                .color(if selected { t.text } else { t.muted })
                .self_center(),
        );
    if selected {
        card = card.outline(1.0, 0.0, t.accent);
    }
    card
}

/// A window in miniature, painted with `p`: sidebar, a heading and two lines, and a composer.
fn mini_window(p: &Theme) -> El {
    let line = |w: f32, c: Color| el().w(w).h(5.0).radius(2.5).fill(c);
    let sidebar = col()
        .w_frac(0.26)
        .h_full()
        .padding(7.0)
        .gap(5.0)
        .fill(p.chrome)
        .child(el().w_full().h(7.0).radius(3.5).fill(p.raised))
        .child(el().w_full().h(5.0).radius(2.5).fill(p.accent.alpha(0.35)))
        .child(el().w_full().h(5.0).radius(2.5).fill(p.wash(0.08)))
        .child(el().w_frac(0.7).h(5.0).radius(2.5).fill(p.wash(0.08)));
    let main = col()
        .grow(1.0)
        .h_full()
        .padding(8.0)
        .gap(5.0)
        .fill(p.canvas)
        .child(line(46.0, p.wash(0.16)))
        .child(line(64.0, p.wash(0.08)))
        .child(line(38.0, p.wash(0.08)))
        .child(el().grow(1.0))
        .child(
            row()
                .w_full()
                .h(14.0)
                .px(4.0)
                .radius(7.0)
                .fill(p.surface)
                .border(1.0, p.hairline)
                .center_y()
                .child(el().grow(1.0))
                .child(el().size(7.0, 7.0).radius(3.5).fill(p.accent)),
        );
    row()
        .h_full()
        .grow(1.0)
        .w_full()
        .fill(p.canvas)
        .child(sidebar)
        .child(el().w(1.0).h_full().fill(p.hairline))
        .child(main)
}

/// One cell of the theme grid: three to a row, two or one as the window narrows.
fn theme_cell() -> El {
    col()
        .grow(1.0)
        .w(0.0)
        .min_w(170.0)
        .padding(size::CARD_PAD)
        .border(1.0, Color::TRANSPARENT)
}

fn theme_card(t: &Theme, current: &Appearance, name: ThemeName) -> El {
    theme_cell()
        .key(format!("theme-{}", name.title()))
        .gap(12.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, t.hairline)
        .child(
            row()
                .w_full()
                .gap(18.0)
                .justify_center()
                .py(4.0)
                .child(orb(t, current, name, false))
                .child(orb(t, current, name, true)),
        )
        .child(text(name.title()).font(ty::BODY_STRONG).color(t.text))
}

/// A swatch of `name`'s light or dark palette. Pressing it uses that theme for that mode.
fn orb(t: &Theme, current: &Appearance, name: ThemeName, dark: bool) -> El {
    const SIZE: f32 = 52.0;
    let p = Theme::new(name, dark);
    let selected = current.extra_theme(dark).is_none() && current.theme(dark) == name;
    let mode = if dark { "dark" } else { "light" };
    let help = format!("Use {} in {mode} mode", name.title());
    let next = if dark {
        Appearance {
            dark_theme: name,
            dark_theme_id: None,
            ..current.clone()
        }
    } else {
        Appearance {
            light_theme: name,
            light_theme_id: None,
            ..current.clone()
        }
    };
    let mut e = el()
        .key(format!("{}-{mode}", name.title()))
        .role("radio")
        .label(help.clone())
        .help(help)
        .checked(selected)
        .size(SIZE, SIZE)
        .radius(SIZE / 2.0)
        .fill(p.surface)
        .radial_gradient(
            [0.62, 0.32],
            SIZE * 0.78,
            [
                (0.0, p.accent),
                (0.45, p.accent.alpha(0.55)),
                (1.0, p.accent.alpha(0.0)),
            ],
        )
        .border(1.0, p.hairline)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.08))
        .press(t.wash(0.14))
        .focus_ring(2.0, 3.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(send(Message::Appearance(next)));
    if selected {
        e = e.outline(2.0, 3.0, t.accent).child(
            el().key("badge")
                .absolute(SIZE - 16.0, SIZE - 16.0)
                .size(20.0, 20.0)
                .radius(10.0)
                .center()
                .fill(t.surface)
                .border(1.0, t.hairline)
                .child(
                    icon(if dark { &icons::MOON } else { &icons::SUN }, 12.0)
                        .color(t.text)
                        .a11y_hidden(),
                )
                .a11y_hidden(),
        );
    }
    e
}

/// Devices and the microphone test, then processing and transmission. Onboarding shows it.
pub fn audio(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let rows = |rows: Vec<Row>| rows.into_iter().map(|row| row.el);
    col()
        .w_full()
        .gap(8.0)
        .child(group(t, rows(audio_rows(s, cx, t, k, layer))).key("devices"))
        .child(group_note(
            t,
            "Disconnect to change devices; volume applies immediately. Use headphones for the microphone test.",
        ))
        .child(group_title(t, "Microphone Processing"))
        .child(group(t, rows(processing_rows(s, t, k, layer))).key("processing"))
        .child(group_title(t, "Voice Transmission"))
        .child(group(t, rows(transmission_rows(s, t, k, layer))).key("transmission"))
        .child(group_note(
            t,
            "Stop the microphone test or disconnect from every server to change processing and transmission. Save to keep them.",
        ))
}

fn audio_rows(s: &App, _cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Vec<Row> {
    let busy = !s.idle();
    let volume = s.playback_volume.percent();
    let inputs: Vec<String> = s.devices.inputs.iter().map(|d| d.name.clone()).collect();
    let outputs: Vec<String> = s.devices.outputs.iter().map(|d| d.name.clone()).collect();
    let testing = busy && s.loopback;
    let input_status: String = if busy {
        s.microphone_status().to_string()
    } else {
        "Start a test to see input".into()
    };
    vec![
        item(
            "microphone input device mic",
            form_row(
                t,
                "Microphone",
                None,
                picker(
                    k,
                    s,
                    "input-device",
                    inputs,
                    s.input.as_ref().map(|d| d.name.as_str()),
                    if s.devices.inputs.is_empty() {
                        "No microphone detected"
                    } else {
                        "Choose…"
                    },
                    !busy,
                    size::FORM_CONTROL,
                    {
                        let devices = s.devices.inputs.clone();
                        move |index| Message::Input(devices[index].clone())
                    },
                    layer,
                ),
            ),
        ),
        item(
            "input level meter microphone",
            form_row(
                t,
                "Input level",
                Some(input_status.as_str()),
                trailing(meter(t, "input-meter", s.microphone_meter_level(), t.good)),
            ),
        ),
        item(
            "output playback device speakers headphones",
            form_row(
                t,
                "Output",
                None,
                picker(
                    k,
                    s,
                    "output-device",
                    outputs,
                    s.output.as_ref().map(|d| d.name.as_str()),
                    if s.devices.outputs.is_empty() {
                        "No playback device detected"
                    } else {
                        "Choose…"
                    },
                    !busy,
                    size::FORM_CONTROL,
                    {
                        let devices = s.devices.outputs.clone();
                        move |index| Message::Output(devices[index].clone())
                    },
                    layer,
                ),
            ),
        ),
        item(
            "output playback volume loudness",
            form_row(
                t,
                format!("Output volume · {volume}%"),
                None,
                trailing(percent_slider(
                    t,
                    "volume",
                    "Playback volume",
                    volume,
                    |s| s.playback_volume.percent(),
                    Message::PlaybackVolume,
                )),
            ),
        ),
        item(
            "refresh devices test microphone loopback",
            actions([
                button(
                    t,
                    "refresh-devices",
                    "Refresh Devices",
                    None,
                    Variant::Secondary,
                    (!busy).then_some(Message::Refresh),
                ),
                button(
                    t,
                    "mic-test",
                    if testing {
                        "Stop Test"
                    } else {
                        "Test Microphone"
                    },
                    None,
                    if testing {
                        Variant::Danger
                    } else {
                        Variant::Secondary
                    },
                    if s.server().stopping {
                        None
                    } else if testing {
                        Some(Message::Stop)
                    } else if !busy {
                        Some(Message::TestAudio)
                    } else {
                        None
                    },
                ),
            ]),
        ),
    ]
}

/// The OS's current answer as a tinted pill, so the row reads at a glance.
fn status_pill(t: &Theme, status: permissions::Status) -> El {
    use permissions::Status;
    let (glyph, color) = match status {
        Status::Granted => (&icons::CHECK, t.good),
        Status::Denied => (&icons::CLOSE, t.poor),
        Status::NotDetermined => (&icons::LOCK, t.fair),
        Status::NotRequired => (&icons::CHECK, t.muted),
        Status::Unknown => (&icons::LOCK, t.faint),
    };
    row()
        .h(22.0)
        .px(8.0)
        .gap(5.0)
        .center_y()
        .radius(11.0)
        .fill(color.alpha(0.14))
        .label(status.label())
        .child(rune::icon(glyph, 12.0).color(color).a11y_hidden())
        .child(
            text(status.label())
                .font(ty::LABEL)
                .color(color)
                .a11y_hidden(),
        )
}

/// A compact switch at a row's trailing edge; the row's label names it.
fn switch(
    key: &'static str,
    label: &str,
    on: bool,
    enabled: bool,
    k: &Kit,
    flip: fn(&App, bool) -> Message,
) -> El {
    Switch::new(on)
        .label(label)
        .hide_label()
        .disabled(!enabled)
        .on_toggle_tx(move |s: &mut App, tx| {
            let message = flip(s, !on);
            super::dispatch(s, tx, message);
        })
        .build(k)
        .key(key)
}

/// A processing change built from the applied options; `update` refuses it during a session.
fn processing_with(s: &App, change: impl FnOnce(&mut audio_processing::Settings)) -> Message {
    let mut next = s.audio_processing;
    change(&mut next);
    Message::AudioProcessing(next)
}

fn sound_cues_row(s: &App, t: &Theme, k: &Kit) -> Row {
    item(
        "sound cues beep notification enable",
        form_row(
            t,
            "Play sounds",
            Some("Short sounds for mute, pokes, channels and your stream."),
            switch(
                "sound-cues",
                "Play sounds",
                s.sound_cues,
                true,
                k,
                |_, on| Message::SoundCues(on),
            ),
        ),
    )
}

/// The volume of every sound, and which sound each event uses.
fn sound_section(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Section {
    use crate::sounds::{Sound, Source};
    let connected = s.sessions.iter().any(|session| session.connected);
    let volume = s.sounds.volume_percent;
    let packs: Vec<Source> = [Source::Flexspeak, Source::Soft, Source::TeamSpeak]
        .into_iter()
        .filter(|&pack| pack != Source::TeamSpeak || !s.sound_bank.is_empty())
        .collect();
    let pack_names = packs.iter().map(|p| p.title().to_owned()).collect();
    let chosen = |sound| s.sound_bank.usable(sound, s.sounds.source(sound));
    let first = chosen(Sound::ALL[0]);
    let pack = Sound::ALL.into_iter().all(|sound| chosen(sound) == first);
    let pack_picker = picker(
        k,
        s,
        "sound-pack",
        pack_names,
        (pack && first != Source::Off).then_some(first.title()),
        "Mixed",
        s.sound_cues,
        size::FORM_CONTROL,
        move |index| Message::SoundPack(packs[index]),
        layer,
    );
    let mut rows = vec![
        sound_cues_row(s, t, k),
        item(
            "sound cues volume loudness",
            form_row(
                t,
                format!("Sound volume · {volume}%"),
                None,
                trailing(percent_slider(
                    t,
                    "sound-volume",
                    "Sound volume",
                    volume,
                    |s| s.sounds.volume_percent,
                    Message::SoundVolume,
                )),
            ),
        ),
        item(
            "sound pack cues soft flexspeak teamspeak",
            form_row(
                t,
                "Sound pack",
                Some("Sets every sound below at once."),
                trailing(pack_picker),
            ),
        ),
    ];
    for sound in Sound::ALL {
        let choices: Vec<Source> = [
            Source::Flexspeak,
            Source::Soft,
            Source::TeamSpeak,
            Source::Off,
        ]
        .into_iter()
        .filter(|&choice| s.sound_bank.usable(sound, choice) == choice)
        .collect();
        let names = choices.iter().map(|c| c.title().to_owned()).collect();
        let current = s.sound_bank.usable(sound, s.sounds.source(sound));
        let chooser = picker(
            k,
            s,
            sound.key(),
            names,
            Some(current.title()),
            "Choose…",
            s.sound_cues,
            size::FORM_CONTROL - 36.0,
            move |index| Message::SoundSource(sound, choices[index]),
            layer,
        );
        let preview = icon_button(
            t,
            &format!("preview-{}", sound.key()),
            &icons::PLAY,
            if connected {
                "Play this sound"
            } else {
                "Connect to a server to hear it"
            },
            Variant::Ghost,
            (connected && current != Source::Off).then_some(Message::PreviewSound(sound)),
        );
        rows.push(item(
            format!("sound cues {}", sound.title().to_lowercase()),
            form_row(
                t,
                sound.title(),
                None,
                trailing(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(preview)
                        .child(el().grow(1.0).min_w(0.0).child(chooser)),
                ),
            ),
        ));
    }
    let note = if s.sound_bank.is_empty() {
        "TeamSpeak's sounds appear here if TeamSpeak is installed on this computer."
    } else {
        "TeamSpeak's sounds are read from TeamSpeak on this computer."
    };
    Section::boxed("sounds", "Sound Cues", "audio sound cues", rows).note(group_note(t, note))
}

fn typing_row(s: &App, t: &Theme, k: &Kit) -> Row {
    item(
        "typing suppression keyboard clicks clatter rnnoise noise",
        form_row(
            t,
            "Typing suppression",
            Some("Removes keyboard clicks and other clatter."),
            switch(
                "typing-suppression-switch",
                "Typing suppression",
                s.audio_processing.typing_suppression,
                s.idle(),
                k,
                |s, on| processing_with(s, |o| o.typing_suppression = on),
            ),
        )
        .key("typing-suppression"),
    )
}

fn processing_rows(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Vec<Row> {
    if !audio_processing::AVAILABLE {
        return vec![
            item(
                "noise suppression echo cancellation gain",
                form_row(
                    t,
                    "Noise suppression and echo cancellation",
                    Some("Available on Mac and Windows. Saved effects are not applied here."),
                    el(),
                ),
            ),
            typing_row(s, t, k),
        ];
    }
    let options = s.audio_processing;
    let editable = s.idle();
    let levels: Vec<String> = audio_processing::SuppressionLevel::ALL
        .iter()
        .map(ToString::to_string)
        .collect();
    let rows = [
        form_row(
            t,
            "Noise suppression",
            Some("Reduces steady background noise such as fans and hum."),
            switch(
                "noise-suppression-switch",
                "Noise suppression",
                options.noise_suppression,
                editable,
                k,
                |s, on| processing_with(s, |o| o.noise_suppression = on),
            ),
        )
        .key("noise-suppression"),
        form_row(
            t,
            "Suppression level",
            Some("Higher levels remove more noise and some voice."),
            picker(
                k,
                s,
                "suppression-level",
                levels,
                Some(options.suppression_level.to_string().as_str()),
                "Choose…",
                editable && options.noise_suppression,
                size::FORM_CONTROL,
                move |index| {
                    Message::AudioProcessing(audio_processing::Settings {
                        suppression_level: audio_processing::SuppressionLevel::ALL[index],
                        ..options
                    })
                },
                layer,
            ),
        )
        .key("suppression-level"),
        form_row(
            t,
            "Automatic gain",
            Some("Evens out your microphone level."),
            switch(
                "automatic-gain-switch",
                "Automatic gain",
                options.automatic_gain,
                editable,
                k,
                |s, on| processing_with(s, |o| o.automatic_gain = on),
            ),
        )
        .key("automatic-gain"),
        form_row(
            t,
            "Echo cancellation",
            Some("Experimental. Removes Flexspeak’s playback from your microphone in calls."),
            switch(
                "echo-cancellation-switch",
                "Echo cancellation",
                options.echo_cancellation,
                editable,
                k,
                |s, on| processing_with(s, |o| o.echo_cancellation = on),
            ),
        )
        .key("echo-cancellation"),
    ];
    let terms = [
        "noise suppression background fans hum",
        "noise suppression level strength",
        "automatic gain control volume loudness",
        "echo cancellation playback feedback",
    ];
    let mut rows: Vec<Row> = rows
        .into_iter()
        .zip(terms)
        .map(|(e, terms)| item(terms, e))
        .collect();
    // After noise suppression: it is the second, stronger pass.
    rows.insert(2, typing_row(s, t, k));
    rows
}

fn transmission_rows(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Vec<Row> {
    let settings = s.transmission;
    let editable = s.idle();
    let modes: Vec<String> = transmission::Mode::ALL
        .iter()
        .map(ToString::to_string)
        .collect();
    let ptt_key = s.keybinds.push_to_talk.trim();
    let detail: String = match settings.mode {
        transmission::Mode::Continuous => "Sends whenever you are unmuted.".into(),
        transmission::Mode::PushToTalk if !shortcuts::supported() => {
            "Global shortcuts are unavailable here. Choose another mode.".into()
        }
        transmission::Mode::PushToTalk if ptt_key.is_empty() => {
            "Set a push-to-talk key under Keyboard Shortcuts.".into()
        }
        transmission::Mode::PushToTalk => format!("Hold {ptt_key} to talk while unmuted."),
        transmission::Mode::VoiceActivation if !audio_processing::AVAILABLE => {
            "Speech detection needs Mac or Windows. Choose another mode.".into()
        }
        transmission::Mode::VoiceActivation => {
            "Sends when speech is detected above the threshold.".into()
        }
    };
    let mut rows = vec![
        form_row(
            t,
            "Transmission",
            Some(detail.as_str()),
            picker(
                k,
                s,
                "transmission-mode",
                modes,
                Some(settings.mode.to_string().as_str()),
                "Choose…",
                editable,
                size::FORM_CONTROL,
                move |index| {
                    Message::Transmission(transmission::Settings {
                        mode: transmission::Mode::ALL[index],
                        ..settings
                    })
                },
                layer,
            ),
        )
        .key("transmission-mode"),
    ];
    match settings.mode {
        transmission::Mode::Continuous => {}
        transmission::Mode::PushToTalk => rows.push(
            form_row(
                t,
                format!("Release delay · {} ms", settings.ptt_release_ms),
                Some("Keeps sending briefly after you let go."),
                trailing(locked(
                    editable,
                    slider(
                        t,
                        "ptt-release",
                        "Push-to-talk release delay",
                        i32::from(settings.ptt_release_ms),
                        format!("{} milliseconds", settings.ptt_release_ms),
                        Range {
                            min: 0,
                            max: 500,
                            step: 25,
                            key_step: 25,
                        },
                        |s| i32::from(s.transmission.ptt_release_ms),
                        move |value| {
                            Message::Transmission(transmission::Settings {
                                ptt_release_ms: value.clamp(0, 500) as u16,
                                ..settings
                            })
                        },
                    ),
                )),
            )
            .key("ptt-release"),
        ),
        transmission::Mode::VoiceActivation => rows.push(
            form_row(
                t,
                format!("Voice threshold · {} dBFS", settings.voice_threshold_db),
                Some("Move right to require a louder voice."),
                trailing(locked(
                    editable,
                    slider(
                        t,
                        "voice-threshold",
                        "Voice activation threshold",
                        i32::from(settings.voice_threshold_db),
                        format!("{} decibels", settings.voice_threshold_db),
                        Range {
                            min: -60,
                            max: 0,
                            step: 1,
                            key_step: 2,
                        },
                        |s| i32::from(s.transmission.voice_threshold_db),
                        move |value| {
                            Message::Transmission(transmission::Settings {
                                voice_threshold_db: value.clamp(-60, 0) as i16,
                                ..settings
                            })
                        },
                    ),
                )),
            )
            .key("voice-threshold"),
        ),
    }
    rows.into_iter()
        .map(|e| {
            item(
                "transmission mode push to talk voice activation threshold release delay continuous",
                e,
            )
        })
        .collect()
}

/// A control that `update` would refuse right now: shown at 42% and not interactive.
fn locked(editable: bool, control: El) -> El {
    if editable {
        control
    } else {
        control.dim(0.42).disabled(true)
    }
}

/// Operating-system access, with its note. Onboarding shows it too.
pub fn permissions(s: &App, t: &Theme) -> El {
    permission_section(s, t).render(t, "").unwrap_or_else(el)
}

fn permission_section(s: &App, t: &Theme) -> Section {
    use permissions::Status;
    let enabled = s.permissions.pending.is_none() && s.idle();
    let mut rows = Vec::new();
    for kind in permissions::Kind::ALL {
        let status = s.permissions.status[kind.index()];
        let pending = s.permissions.pending == Some(kind);
        let detail = match &s.permissions.messages[kind.index()] {
            Some(message) => format!("{} {message}", kind.detail()),
            None => kind.detail().to_owned(),
        };
        let mut controls = row().gap(size::GAP).center_y();
        if pending {
            controls = controls.child(super::widgets::pulse(t.accent));
        }
        controls = controls.child(status_pill(t, status));
        // Once denied, the OS won't ask again: the switch lives in its privacy settings.
        let give = match status {
            Status::NotDetermined | Status::Unknown => Some(Message::RequestPermission(kind)),
            Status::Denied => Some(Message::OpenPermissionSettings(kind)),
            Status::Granted | Status::NotRequired => None,
        };
        if let Some(give) = give {
            controls = controls.child(button(
                t,
                "request",
                "Give Access",
                None,
                Variant::Primary,
                enabled.then_some(give),
            ));
        }
        if cfg!(any(target_os = "macos", windows)) && status != Status::NotRequired {
            controls = controls.child(icon_button(
                t,
                "os-settings",
                &icons::SETTINGS,
                "Open system settings",
                Variant::Ghost,
                enabled.then_some(Message::OpenPermissionSettings(kind)),
            ));
        }
        rows.push(item(
            format!(
                "{} {detail} {} access privacy",
                kind.title(),
                status.label()
            ),
            form_row(t, kind.title(), Some(detail.as_str()), controls)
                .key(format!("permission-{}", kind.index())),
        ));
    }
    let mut note = String::from("Your operating system controls access; you can change it later.");
    if cfg!(target_os = "macos") {
        note.push_str(" This build appears as Flexspeak in Privacy & Security. macOS may ask you to reopen the app after allowing screen recording.");
    }
    if !s.idle() {
        note.push_str(" Disconnect or stop the local test before requesting access.");
    }
    Section::boxed("permissions", "Permissions", "privacy access", rows).note(group_note(t, note))
}

/// A shortcut and its Change button; pressed, the row listens for the new key combination.
fn shortcut_row(
    s: &App,
    t: &Theme,
    k: &Kit,
    binding: shortcuts::Binding,
    label: &str,
    detail: Option<&str>,
) -> El {
    let Some(capture) = s.key_capture.as_ref().filter(|c| c.binding == binding) else {
        let current = s.keybind_draft.get(binding).trim();
        let shown = row()
            .key("shortcut-value")
            .grow(1.0)
            .min_w(0.0)
            .h(size::CONTROL)
            .px(10.0)
            .center_y()
            .radius(radius::CONTROL)
            .fill(t.wash(0.05))
            .border(1.0, t.hairline)
            .child(
                text(if current.is_empty() {
                    "Not set"
                } else {
                    current
                })
                .font(ty::NUMERIC)
                .color(if current.is_empty() { t.faint } else { t.text })
                .truncate_fade(),
            );
        let change = button(
            t,
            &format!("change-{binding:?}"),
            "Change",
            None,
            Variant::Secondary,
            Some(Message::RecordShortcut(binding)),
        );
        return form_row(
            t,
            label,
            detail,
            trailing(
                row()
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(shown)
                    .child(change),
            ),
        );
    };
    let recorder = TextInput::new(&s.ui.key_capture)
        .placeholder("Press a shortcut…")
        .label(format!("Press the new shortcut for {label}"))
        .width(None)
        .height(size::CONTROL)
        .on_edit(|s: &mut App| &mut s.ui.key_capture)
        // Taking Return here keeps it from pressing the window's default button.
        .on_submit(|_: &mut App, _: &mut Tx| {})
        .on_key(|s: &mut App, key: &rune::KeyEvent, tx: &mut Tx| {
            let recorded = shortcuts::record(key, tx.physical_key());
            dispatch(s, tx, Message::Recorded(recorded));
            true
        })
        .build(k);
    let mut control = col().w_full().gap(4.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(el().grow(1.0).min_w(0.0).child(recorder))
            .child(button(
                t,
                "cancel-recording",
                "Cancel",
                None,
                Variant::Secondary,
                Some(Message::CancelRecording),
            )),
    );
    if let Some(error) = &capture.error {
        control = control.child(super::widgets::error_text(t, error.clone()));
    }
    form_row(
        t,
        label,
        Some(if binding == shortcuts::Binding::PushToTalk {
            "Esc to cancel · Backspace to clear"
        } else {
            "Esc to cancel"
        }),
        trailing(control),
    )
}

/// The mute and deafen shortcuts and their test. Onboarding shows it too.
pub fn shortcut_settings(s: &App, t: &Theme, k: &Kit) -> El {
    shortcut_section(s, t, k).render(t, "").unwrap_or_else(el)
}

fn shortcut_section(s: &App, t: &Theme, k: &Kit) -> Section {
    let detected = match s.last_shortcut {
        Some(shortcuts::Action::Mute) => "Detected mute / unmute",
        Some(shortcuts::Action::Deafen) => "Detected deafen / undeafen",
        Some(shortcuts::Action::PushToTalk(true)) => "Detected push-to-talk pressed",
        Some(shortcuts::Action::PushToTalk(false)) => "Detected push-to-talk released",
        None => "Press a shortcut after testing to check it",
    };
    Section::boxed(
        "shortcuts",
        "Keyboard Shortcuts",
        "hotkeys keybinds keys",
        vec![
            item(
                "toggle microphone mute unmute",
                shortcut_row(s, t, k, shortcuts::Binding::Mute, "Toggle microphone", None),
            ),
            item(
                "toggle deafen undeafen",
                shortcut_row(s, t, k, shortcuts::Binding::Deafen, "Toggle deafen", None),
            ),
            item(
                "push to talk ptt hold transmit",
                shortcut_row(
                    s,
                    t,
                    k,
                    shortcuts::Binding::PushToTalk,
                    "Push to talk",
                    Some("Held, not toggled. Used in push-to-talk mode."),
                ),
            ),
            item(
                "global shortcuts test detect",
                form_row(
                    t,
                    "Global shortcuts",
                    Some(detected),
                    button(
                        t,
                        "test-shortcuts",
                        "Test Shortcuts",
                        None,
                        Variant::Secondary,
                        s.key_capture.is_none().then_some(Message::TestHotkeys),
                    ),
                )
                .key("detected"),
            ),
        ],
    )
    .note(shortcut_hint(s, t).px(2.0))
}

/// The global shortcuts in force, or why they aren't.
fn shortcut_hint(s: &App, t: &Theme) -> El {
    if let Some(error) = &s.shortcut_error {
        super::widgets::error_text(
            t,
            if shortcuts::supported() {
                "Global shortcuts unavailable. Configure them in Settings.".into()
            } else {
                format!("Global shortcuts unavailable: {error}")
            },
        )
    } else {
        let mut hint = format!(
            "Global: {} · mute/unmute   |   {} · deafen/undeafen",
            s.keybinds.toggle_mute, s.keybinds.toggle_deafen
        );
        if !s.keybinds.push_to_talk.trim().is_empty() {
            hint.push_str(&format!(
                "   |   {} · hold to talk",
                s.keybinds.push_to_talk
            ));
        }
        super::widgets::faint(t, hint)
    }
}
