//! The Settings window, and the sections onboarding reuses: audio, permissions and shortcuts.
//!
//! Settings floats over the window as a modal panel: categories and search lead, the chosen
//! category's sections trail. Every section is a list of rows with search terms, so a query
//! narrows the panel to the rows that match across all categories.

use rune::{Color, Cx, El, Icon, TextEdit, Tx, col, el, icon, row, text};
use rune_kit::{Kit, TextInput};

use super::theme::{Theme, motion, radius, shadow, size, ty};
use super::widgets::{
    Variant, button, divider, field, form_row, group, group_note, group_title, icon_button,
    labelled, meter, muted, percent_slider, picker, scroll_page,
};
use super::{dispatch, home, icons, send};
use crate::settings::{Appearance, ColorScheme, ThemeName};
use crate::{App, Message, SettingsPage, permissions, shortcuts};

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
    (Some("Storage"), &[SettingsPage::File]),
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
        SettingsPage::Appearance => appearance_sections(s, t),
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
        ],
        SettingsPage::Shortcuts => vec![shortcut_section(s, t, k)],
        SettingsPage::Permissions => vec![permission_section(s, t)],
        SettingsPage::File => vec![file_section(s, t)],
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
        .child(sidebar(s, t, k, page, searching, &found, narrow))
        .child(el().w(1.0).h_full().fill(t.hairline).a11y_hidden())
        .child(main)
        .modal(true, |s: &mut App| {
            let _ = s.update(Message::CloseSettings);
            s.sync_fields();
        });
    layer.push(
        el().key("settings-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(k.c.scrim)
            .token_motion(k.motion.overlay)
            .a11y_hidden(),
    );
    layer.push(
        el().key("settings-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .padding(if narrow { 16.0 } else { 28.0 })
            .center()
            .child(panel),
    );
    layer.extend(menus);
    server_dialog(s, t, k, layer);
}

fn sidebar(
    s: &App,
    t: &Theme,
    k: &Kit,
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
        .child(search_field(s, t, k))
        .child(nav)
}

fn search_field(s: &App, t: &Theme, k: &Kit) -> El {
    let mut input = TextInput::new(&s.ui.settings_search)
        .placeholder("Search")
        .label("Search settings")
        .leading_icon(&icons::SEARCH)
        .width(None)
        .height(size::CONTROL + 2.0)
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
    el().key("settings-search")
        .w_full()
        .pb(6.0)
        .child(input.build(k))
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
    let current = s.session_address();
    let mut rows: Vec<Row> = s
        .servers
        .iter()
        .enumerate()
        .map(|(index, server)| {
            let mine = current == Some(server.address.trim());
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
                    .children(mine.then(|| {
                        badge(
                            t,
                            if s.connected {
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
            "Save as many servers as you like and connect to one at a time from the home screen. Changes here save straight away.",
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
    let idle = s.session.is_none();
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
        "Add it to your saved servers. You connect to one at a time."
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
    let idle = s.session.is_none();
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

/// The colour scheme, then a theme for light and one for dark. Choices apply and save at once.
fn appearance_sections(s: &App, t: &Theme) -> Vec<Section> {
    let current = s.appearance;
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
    vec![
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
    ]
}

fn scheme_card(t: &Theme, current: Appearance, scheme: ColorScheme) -> El {
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
        .on_press_tx(send(Message::Appearance(Appearance { scheme, ..current })))
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

fn theme_card(t: &Theme, current: Appearance, name: ThemeName) -> El {
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
fn orb(t: &Theme, current: Appearance, name: ThemeName, dark: bool) -> El {
    const SIZE: f32 = 52.0;
    let p = Theme::new(name, dark);
    let selected = current.theme(dark) == name;
    let mode = if dark { "dark" } else { "light" };
    let help = format!("Use {} in {mode} mode", name.title());
    let next = if dark {
        Appearance {
            dark_theme: name,
            ..current
        }
    } else {
        Appearance {
            light_theme: name,
            ..current
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

/// Devices, levels and the microphone test, as one group. Onboarding shows it too.
pub fn audio(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    group(
        t,
        audio_rows(s, cx, t, k, layer).into_iter().map(|row| row.el),
    )
}

fn audio_rows(s: &App, _cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Vec<Row> {
    let busy = s.session.is_some();
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
                    if s.stopping {
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

/// Operating-system access, with its note. Onboarding shows it too.
pub fn permissions(s: &App, t: &Theme) -> El {
    permission_section(s, t).render(t, "").unwrap_or_else(el)
}

fn permission_section(s: &App, t: &Theme) -> Section {
    let enabled = s.permissions.pending.is_none() && s.session.is_none();
    let mut rows = Vec::new();
    for kind in permissions::Kind::ALL {
        let request_label = if cfg!(windows) && kind == permissions::Kind::Microphone {
            "Open Privacy Settings"
        } else if cfg!(target_os = "macos") {
            "Request Access"
        } else {
            "Review Access"
        };
        let pending = s.permissions.pending == Some(kind);
        let detail = match &s.permissions.messages[kind.index()] {
            Some(message) => format!("{} {message}", kind.detail()),
            None => kind.detail().to_owned(),
        };
        let mut controls = row().gap(size::GAP).center_y();
        if pending {
            controls = controls.child(super::widgets::pulse(t.accent));
        }
        controls = controls.child(button(
            t,
            "request",
            request_label,
            None,
            Variant::Secondary,
            enabled.then_some(Message::RequestPermission(kind)),
        ));
        if cfg!(any(target_os = "macos", windows)) {
            controls = controls.child(super::widgets::icon_button(
                t,
                "os-settings",
                &icons::SETTINGS,
                "Open system settings",
                Variant::Ghost,
                enabled.then_some(Message::OpenPermissionSettings(kind)),
            ));
        }
        rows.push(item(
            format!("{} {detail} access privacy", kind.title()),
            form_row(t, kind.title(), Some(detail.as_str()), controls)
                .key(format!("permission-{}", kind.index())),
        ));
    }
    let mut note = String::from("Your operating system controls access; you can change it later.");
    if cfg!(target_os = "macos") {
        note.push_str(" This build appears as Flexspeak in Privacy & Security.");
    }
    if s.session.is_some() {
        note.push_str(" Disconnect or stop the local test before requesting access.");
    }
    Section::boxed("permissions", "Permissions", "privacy access", rows).note(group_note(t, note))
}

fn key_field(
    k: &Kit,
    edit: &TextEdit,
    access: fn(&mut App) -> &mut TextEdit,
    placeholder: &str,
    change: fn(String) -> Message,
) -> El {
    trailing(field(k, edit, access, placeholder, Some(change), None))
}

/// The mute and deafen shortcuts and their test. Onboarding shows it too.
pub fn shortcut_settings(s: &App, t: &Theme, k: &Kit) -> El {
    shortcut_section(s, t, k).render(t, "").unwrap_or_else(el)
}

fn shortcut_section(s: &App, t: &Theme, k: &Kit) -> Section {
    let detected = match s.last_shortcut {
        Some(shortcuts::Action::Mute) => "Detected mute / unmute",
        Some(shortcuts::Action::Deafen) => "Detected deafen / undeafen",
        None => "Press a shortcut after testing to check it",
    };
    Section::boxed(
        "shortcuts",
        "Keyboard Shortcuts",
        "hotkeys keybinds keys",
        vec![
            item(
                "toggle microphone mute unmute",
                form_row(
                    t,
                    "Toggle microphone",
                    None,
                    key_field(
                        k,
                        &s.ui.mute_key,
                        |s| &mut s.ui.mute_key,
                        "Ctrl+Shift+M",
                        Message::MuteKeybind,
                    ),
                ),
            ),
            item(
                "toggle deafen undeafen",
                form_row(
                    t,
                    "Toggle deafen",
                    None,
                    key_field(
                        k,
                        &s.ui.deafen_key,
                        |s| &mut s.ui.deafen_key,
                        "Ctrl+Shift+D",
                        Message::DeafenKeybind,
                    ),
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
                        Some(Message::TestHotkeys),
                    ),
                )
                .key("detected"),
            ),
        ],
    )
    .note(home::shortcut_hint(s, t).px(2.0))
}
