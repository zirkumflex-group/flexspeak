//! The secret Monster page: add a can by its unique number, get a card, watch the board fill.
//!
//! A card is a stack of rune-fx layers (see `fx::CardFx`) around a can built from more of them.
//! Cards sit on a fixed dark cabinet in every theme, because their glow and fire are light.

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use rune::{Color, Cx, Drag, El, TextStyle, Transition, Tx, col, el, icon, row, text};
use rune_kit::Kit;

use super::can3d::{self, HOVERED, REDUCED, SHOWCASE};
use super::fx::{CardFx, card_fx, card_layers};
use super::theme::{Theme, motion, radius, rgb, size, ty};
use super::widgets::{
    Variant, button, error_text, faint, field, group_note, group_title, heading, icon_button,
    muted, scroll_page, title, well,
};
use super::{icons, send};
use crate::monster::{
    self, Action, Beat, Board, CARDS, Can, FLAVOURS, Flavour, Gate, Notice, Ranking, Rarity,
    Reveal, Section, Standing,
};
use crate::{App, Message};

const CABINET: u32 = 0x0b0c10;
const LARGE_CARD: f32 = 236.0;
const SMALL_CARD: f32 = 150.0;
const CARD_RATIO: f32 = 1.4;
/// A heatmap square and the gap after it.
const CELL: f32 = 11.0;
const CELL_GAP: f32 = 3.0;
const WEEKDAY_COLUMN: f32 = 30.0;
const MAX_WEEKS: usize = 53;
const MIN_WEEKS: usize = 13;
/// How many flavours the board names before lumping the rest together.
const LEGEND_FLAVOURS: usize = 7;

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit) -> El {
    let reduced = cx.is_reduced_motion();
    if let Some(gate) = s.monster.gate.as_ref().filter(|_| s.monster.online()) {
        let content = col()
            .w_full()
            .max_w(1100.0)
            .self_center()
            .padding(size::PAGE)
            .child(sign_in(s, t, k, gate));
        return scroll_page(k, "monster-scroll-sign-in", content);
    }
    let today = Local::now().date_naive();
    let section = s.monster.section;
    let (key, sections) = match section {
        Section::Overview => ("overview".to_owned(), overview(s, t, k, today, reduced)),
        Section::Flavours => ("flavours".to_owned(), flavour_list(s, t, reduced)),
        Section::Flavour(index) => (
            format!("flavour-{index}"),
            flavour_page(s, t, index, today, reduced),
        ),
        Section::Leaderboard => ("leaderboard".to_owned(), leaderboard(s, t)),
        Section::Person(user) => (
            format!("person-{user}"),
            person_page(s, t, user, today, reduced),
        ),
    };
    let content = col()
        .w_full()
        .max_w(1100.0)
        .self_center()
        .padding(size::PAGE)
        .gap(20.0)
        // Nothing covers it on the leaderboard, and no reveal starts from there.
        .children(
            (!matches!(section, Section::Leaderboard | Section::Person(_)))
                .then(|| warm_up(reduced)),
        )
        .child(tabs(s, t, section))
        .children(sections);
    // Keyed by section, so each one opens at its top.
    scroll_page(k, &format!("monster-scroll-{key}"), content)
}

/// The sign-in steps, as the strip above the form names them.
const GATE_STEPS: [&str; 4] = ["Email", "Code", "Passkey", "Identity"];

/// The strip of steps: those done are ticked, the current one is filled.
fn gate_steps(t: &Theme, current: usize) -> El {
    let mut strip = row().w_full().center_y().gap(8.0).a11y_hidden();
    for (n, name) in GATE_STEPS.into_iter().enumerate() {
        if n > 0 {
            let line = if n <= current { t.accent } else { t.hairline };
            strip = strip.child(el().grow(1.0).h(1.0).fill(line));
        }
        let badge = el().size(22.0, 22.0).radius(11.0).center();
        let badge = if n < current {
            badge
                .border(1.0, t.accent)
                .child(icon(&icons::CHECK, 12.0).color(t.accent))
        } else if n == current {
            badge.fill(t.accent).child(
                text((n + 1).to_string())
                    .font(ty::SMALL_STRONG)
                    .color(t.on_accent),
            )
        } else {
            badge
                .border(1.0, t.hairline)
                .child(text((n + 1).to_string()).font(ty::SMALL).color(t.faint))
        };
        let ink = if n == current { t.text } else { t.faint };
        strip = strip.child(
            row()
                .gap(6.0)
                .center_y()
                .child(badge)
                .child(text(name).font(ty::SMALL_STRONG).color(ink)),
        );
    }
    strip
}

/// The round mark above a step's title.
fn gate_mark(t: &Theme, glyph: &'static rune::Icon) -> El {
    el().size(44.0, 44.0)
        .radius(22.0)
        .fill(t.recess)
        .border(1.0, t.hairline)
        .center()
        .child(icon(glyph, 20.0).color(t.accent))
        .a11y_hidden()
}

fn gate_buttons(buttons: impl IntoIterator<Item = El>) -> El {
    row()
        .w_full()
        .gap(size::GAP)
        .center_y()
        .flex_wrap()
        .mt(4.0)
        .children(buttons)
}

/// The primary button of a step: its label while ready, "Sending…" while its request is out.
fn gate_primary(
    t: &Theme,
    key: &str,
    label: &str,
    glyph: Option<&'static rune::Icon>,
    busy: bool,
    message: Option<Message>,
) -> El {
    button(
        t,
        key,
        if busy { "Sending…" } else { label },
        glyph,
        Variant::Primary,
        message.filter(|_| !busy),
    )
}

/// What a wrong code leaves, in words.
fn wrong_code(tries_left: Option<u32>) -> String {
    match tries_left {
        None => "That code doesn't match.".into(),
        Some(0) => "That code doesn't match, and it can't be tried again. Send a new one.".into(),
        Some(1) => "That code doesn't match. One try left.".into(),
        Some(n) => format!("That code doesn't match. {n} tries left."),
    }
}

/// Signing in, in the page's place: an email code, then a passkey, then the TeamSpeak
/// identity if the person wants it linked.
fn sign_in(s: &App, t: &Theme, k: &Kit, gate: &Gate) -> El {
    let tracker = &s.monster;
    let busy = tracker.busy;
    let act = |action: Action| Some(Message::Monster(action));
    let mut form = match gate {
        Gate::Email => {
            let email = s.ui.monster_email.text.trim().to_owned();
            let next = email
                .contains('@')
                .then(|| act(Action::SendCode(email)))
                .flatten();
            vec![
                gate_mark(t, &icons::MAIL),
                title(t, "Sign In to Your Collection"),
                muted(
                    t,
                    "Enter your email address and we'll send you a six-digit code. There is no \
                     password.",
                ),
                field(
                    k,
                    &s.ui.monster_email,
                    |s: &mut App| &mut s.ui.monster_email,
                    "Email address",
                    Some(|_| Message::MonsterSignInTyped),
                    next.clone().filter(|_| !busy),
                ),
                gate_buttons([
                    gate_primary(
                        t,
                        "monster-send-code",
                        "Send Code",
                        Some(&icons::MAIL),
                        busy,
                        next,
                    ),
                    button(
                        t,
                        "monster-passkey-sign-in",
                        "Sign In With a Passkey",
                        Some(&icons::KEY_ROUND),
                        Variant::Ghost,
                        act(Action::PasskeySignIn).filter(|_| !busy),
                    ),
                ]),
            ]
        }
        Gate::Code {
            email,
            wrong,
            tries_left,
            resent,
        } => {
            let code = s.ui.monster_login_code.text.trim().to_owned();
            let whole = code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit());
            let next = whole.then(|| act(Action::CheckCode(code))).flatten();
            let mut form = vec![
                gate_mark(t, &icons::MAIL),
                title(t, "Check Your Email"),
                muted(
                    t,
                    format!(
                        "We sent a code to {email}. Type it here. The code works for ten minutes."
                    ),
                ),
                field(
                    k,
                    &s.ui.monster_login_code,
                    |s: &mut App| &mut s.ui.monster_login_code,
                    "Six-digit code",
                    Some(|_| Message::MonsterSignInTyped),
                    next.clone().filter(|_| !busy),
                ),
            ];
            if *wrong {
                form.push(error_text(t, wrong_code(*tries_left)).key("monster-wrong-code"));
            } else if *resent {
                form.push(faint(
                    t,
                    "We sent a new code. The one before it no longer works.",
                ));
            }
            form.push(gate_buttons([
                gate_primary(t, "monster-check-code", "Continue", None, busy, next),
                button(
                    t,
                    "monster-send-again",
                    "Send Again",
                    None,
                    Variant::Ghost,
                    act(Action::SendAgain).filter(|_| !busy),
                ),
                button(
                    t,
                    "monster-other-address",
                    "Use Another Address",
                    None,
                    Variant::Ghost,
                    act(Action::OtherAddress),
                ),
            ]));
            form
        }
        Gate::Passkey {
            create, waiting, ..
        } => {
            let (heading, about) = if *create {
                (
                    "Create a Passkey",
                    "A passkey signs you in from now on, with Touch ID, Windows Hello or your \
                     password manager. You won't need a code again on this device.",
                )
            } else {
                (
                    "Sign In With Your Passkey",
                    "Use the passkey you made before, with Touch ID, Windows Hello or your \
                     password manager. No code needed.",
                )
            };
            let mut form = vec![
                gate_mark(t, &icons::KEY_ROUND),
                title(t, heading),
                muted(t, about),
            ];
            if *waiting {
                form.push(
                    muted(
                        t,
                        "We opened a page in your browser. Finish there, and this page carries on \
                         by itself.",
                    )
                    .key("monster-passkey-browser"),
                );
            } else {
                form.push(group_note(
                    t,
                    "It opens in your browser. The passkey stays with you; we store only its \
                     public half.",
                ));
            }
            form.push(gate_buttons(if *waiting {
                vec![
                    button(
                        t,
                        "monster-passkey",
                        "Waiting for Your Browser…",
                        Some(&icons::KEY_ROUND),
                        Variant::Primary,
                        None,
                    ),
                    button(
                        t,
                        "monster-passkey-again",
                        "Open Again",
                        None,
                        Variant::Ghost,
                        act(Action::OpenPasskey),
                    ),
                    button(
                        t,
                        "monster-passkey-cancel",
                        "Cancel",
                        None,
                        Variant::Ghost,
                        act(Action::CancelPasskey),
                    ),
                ]
            } else {
                vec![button(
                    t,
                    "monster-passkey",
                    if *create {
                        "Create Passkey"
                    } else {
                        "Use Passkey"
                    },
                    Some(&icons::KEY_ROUND),
                    Variant::Primary,
                    act(Action::OpenPasskey),
                )]
            }));
            form
        }
        Gate::Identity(found) => {
            let mut list = well(t).key("monster-identities");
            if found.is_empty() {
                list = list.child(
                    el().padding(12.0)
                        .child(muted(t, "No TeamSpeak identity found on this computer.")),
                );
            }
            for (n, identity) in found.iter().enumerate() {
                if n > 0 {
                    list = list.child(el().w_full().h(1.0).fill(t.hairline).a11y_hidden());
                }
                let state = if identity.linked {
                    row()
                        .key(format!("monster-linked-{n}"))
                        .gap(6.0)
                        .center_y()
                        .child(icon(&icons::CHECK, 14.0).color(t.good).a11y_hidden())
                        .child(text("Linked").font(ty::SMALL_STRONG).color(t.good))
                } else {
                    button(
                        t,
                        &format!("monster-link-identity-{n}"),
                        if busy { "Linking…" } else { "Link" },
                        None,
                        Variant::Secondary,
                        act(Action::Link(n)).filter(|_| !busy),
                    )
                };
                let who = row()
                    .gap(6.0)
                    .center_y()
                    .child(
                        text(identity.name.clone())
                            .font(ty::BODY_STRONG)
                            .color(t.text),
                    )
                    .child(text(identity.source).font(ty::SMALL).color(t.muted));
                list =
                    list.child(
                        row()
                            .w_full()
                            .padding(12.0)
                            .gap(10.0)
                            .center_y()
                            .child(icon(&icons::USER, 16.0).color(t.muted).a11y_hidden())
                            .child(
                                col().grow(1.0).gap(2.0).child(who).child(
                                    text(identity.uid.clone()).font(ty::NUMERIC).color(t.faint),
                                ),
                            )
                            .child(state),
                    );
            }
            let open = act(Action::Enter);
            let unlinked = found.iter().filter(|identity| !identity.linked).count();
            let buttons = if unlinked == 0 {
                vec![button(
                    t,
                    "monster-enter",
                    "Open My Collection",
                    None,
                    Variant::Primary,
                    open,
                )]
            } else {
                let none_linked = unlinked == found.len();
                let mut buttons = vec![];
                if unlinked > 1 || !none_linked {
                    buttons.push(button(
                        t,
                        "monster-link-all",
                        if none_linked {
                            format!("Link All {unlinked}")
                        } else if unlinked == 1 {
                            "Link the Other One".to_owned()
                        } else {
                            format!("Link the Other {unlinked}")
                        },
                        Some(&icons::LINK),
                        Variant::Primary,
                        act(Action::LinkAll).filter(|_| !busy),
                    ));
                }
                buttons.push(button(
                    t,
                    "monster-enter",
                    if none_linked {
                        "Skip for Now"
                    } else {
                        "Continue"
                    },
                    None,
                    Variant::Ghost,
                    open,
                ));
                buttons
            };
            vec![
                gate_mark(t, &icons::LINK),
                title(t, "Link Your TeamSpeak Identity"),
                muted(
                    t,
                    "Optional. Linking saves the identity's public key to your account, so \
                     people you share a server with can find your collection later. The \
                     private key never leaves this computer.",
                ),
                muted(t, "Found on This Computer").mt(4.0),
                list,
                group_note(
                    t,
                    "Identities on another computer are offered when you sign in there.",
                ),
                gate_buttons(buttons),
            ]
        }
    };
    if let Some(problem) = &tracker.gate_problem {
        // Above the buttons, so it reads before what to do about it.
        let at = form.len().saturating_sub(1);
        form.insert(
            at,
            error_text(t, problem.clone()).key("monster-gate-problem"),
        );
    }
    col()
        .key("monster-sign-in")
        .w_full()
        .max_w(460.0)
        .self_center()
        .mt(24.0)
        .gap(20.0)
        .child(gate_steps(t, gate.step()))
        .child(
            col()
                .w_full()
                .padding(24.0)
                .gap(12.0)
                .radius(radius::CARD)
                .fill(t.surface)
                .border(1.0, t.hairline)
                .children(form),
        )
        .child(faint(
            t,
            "Only this page needs an account. The rest of Flexspeak works without one.",
        ))
}

/// A round badge with someone's initial.
fn avatar(t: &Theme, name: &str, diameter: f32, you: bool) -> El {
    let initial = name
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    el().size(diameter, diameter)
        .radius(diameter / 2.0)
        .fill(if you { t.accent } else { t.recess })
        .border(1.0, t.hairline)
        .center()
        .child(
            text(initial)
                .font(ty::SMALL_STRONG)
                .color(if you { t.on_accent } else { t.text }),
        )
        .a11y_hidden()
}

/// Where someone places, with the top three in their medal's colour.
fn place(t: &Theme, n: usize) -> El {
    let color = match n {
        1 => rgb(Rarity::Legendary.color()),
        2 => rgb(0xc9ced8),
        3 => rgb(0xc98a55),
        _ => t.faint,
    };
    text(n.to_string())
        .font(ty::BODY_STRONG)
        .color(color)
        .w(28.0)
        .a11y_hidden()
}

fn score_label(ranking: Ranking, standing: Standing) -> (String, &'static str) {
    match ranking {
        Ranking::Collection => (format!("{} / {CARDS}", standing.cards), "cards"),
        Ranking::Legendary => (standing.legendary.to_string(), "Legendary"),
        Ranking::Flavours => (
            format!("{} / {}", standing.flavours, FLAVOURS.len()),
            "flavours",
        ),
    }
}

/// Everyone who shows on the leaderboard, ranked by their collection. A row opens that
/// person's board; your own opens the overview.
fn leaderboard(s: &App, t: &Theme) -> Vec<El> {
    let tracker = &s.monster;
    if !tracker.online() {
        return vec![
            title(t, "Leaderboard"),
            row()
                .key("monster-leaderboard-local")
                .w_full()
                .gap(size::GAP)
                .center_y()
                .flex_wrap()
                .child(
                    muted(
                        t,
                        "The leaderboard needs a collection server. Set one in Settings › WIP.",
                    )
                    .grow(1.0),
                )
                .child(button(
                    t,
                    "monster-open-wip",
                    "Open Settings",
                    None,
                    Variant::Secondary,
                    Some(Message::OpenSettings(crate::SettingsPage::Wip)),
                )),
        ];
    }
    let ranking = tracker.ranking;
    let mut choices = row().gap(size::GAP).flex_wrap();
    for option in Ranking::ALL {
        choices = choices.child(button(
            t,
            &format!("monster-rank-{}", option.name().to_lowercase()),
            option.name(),
            None,
            if option == ranking {
                Variant::Active
            } else {
                Variant::Ghost
            },
            Some(Message::MonsterRanking(option)),
        ));
    }
    let rows = monster::ranked(&tracker.people, ranking, !tracker.hidden);
    let mut list = well(t).key("monster-leaderboard");
    if rows.is_empty() {
        list = list.child(el().padding(16.0).child(muted(
            t,
            if tracker.people_loading {
                "Loading the leaderboard…"
            } else {
                "Nobody shows on the leaderboard yet."
            },
        )));
    }
    // Ties share a place.
    let mut shown_place = 0;
    let mut last_score = None;
    for (position, person) in rows.iter().enumerate() {
        let standing = person.standing;
        let score = standing.score(ranking);
        if last_score != Some(score) {
            shown_place = position + 1;
            last_score = Some(score);
        }
        if position > 0 {
            list = list.child(el().w_full().h(1.0).fill(t.hairline).a11y_hidden());
        }
        let (value, unit) = score_label(ranking, standing);
        let mut name = row().gap(6.0).center_y().child(
            text(person.name.clone())
                .font(ty::BODY_STRONG)
                .color(t.text),
        );
        if person.you {
            name = name.child(
                el().px(6.0)
                    .py(1.0)
                    .radius(radius::PILL)
                    .fill(t.accent.alpha(0.18))
                    .child(text("You").font(ty::CAPTION).color(t.accent)),
            );
        }
        let detail = format!(
            "{} flavours · {} Legendary · {} cards",
            standing.flavours, standing.legendary, standing.cards
        );
        let target = if person.you {
            Section::Overview
        } else {
            Section::Person(person.user)
        };
        let key = format!("monster-person-{}", person.user);
        let hovered = s.ui.can_hover.as_deref() == Some(key.as_str());
        list = list.child(
            row()
                .key(key.clone())
                .role("button")
                .label(format!(
                    "{}, place {shown_place}, {value} {unit}",
                    person.name
                ))
                .w_full()
                .padding(12.0)
                .gap(12.0)
                .center_y()
                .radius(radius::ROW)
                .fill(if hovered {
                    t.surface
                } else {
                    Color::TRANSPARENT
                })
                .cursor(rune::Cursor::Pointer)
                .focusable(true)
                .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
                .on_hover_change(hovering(key))
                .on_press_tx(send(Message::MonsterShow(target)))
                .child(place(t, shown_place))
                .child(avatar(t, &person.name, 32.0, person.you))
                .child(
                    col()
                        .grow(1.0)
                        .gap(2.0)
                        .child(name)
                        .child(text(detail).font(ty::SMALL).color(t.faint)),
                )
                .child(
                    col()
                        .gap(0.0)
                        .child(text(value).font(ty::NUMERIC).color(t.text).self_end())
                        .child(text(unit).font(ty::CAPTION).color(t.faint).self_end()),
                )
                .child(
                    icon(&icons::CHEVRON_RIGHT, 14.0)
                        .color(t.faint)
                        .a11y_hidden(),
                ),
        );
    }
    let privacy = if tracker.hidden {
        row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .flex_wrap()
            .child(
                muted(
                    t,
                    "You're hidden. Others can't see your place or open your board.",
                )
                .grow(1.0),
            )
            .child(button(
                t,
                "monster-show-me",
                "Show Me",
                None,
                Variant::Secondary,
                Some(Message::MonsterHidden(false)),
            ))
    } else {
        row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .flex_wrap()
            .child(
                muted(
                    t,
                    "Anyone on the leaderboard can open your board: your stats, heatmap and cards.",
                )
                .grow(1.0),
            )
            .child(button(
                t,
                "monster-hide-me",
                "Hide Me",
                None,
                Variant::Ghost,
                Some(Message::MonsterHidden(true)),
            ))
    };
    let mut parts = vec![
        row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .flex_wrap()
            .child(title(t, "Leaderboard").grow(1.0))
            .child(choices),
        muted(
            t,
            match ranking {
                Ranking::Collection => {
                    "Different cards collected: each flavour at each rarity counts once. Cards \
                     moved from a computer don't count."
                }
                Ranking::Legendary => "Legendary cards pulled.",
                Ranking::Flavours => "Different flavours collected.",
            },
        ),
        list,
    ];
    if let Some(problem) = &tracker.people_problem {
        parts.push(error_text(t, problem.clone()).key("monster-leaderboard-problem"));
    }
    parts.push(privacy);
    parts
}

/// Someone else's board and cards, as they'd see their own, without anything to change.
fn person_page(s: &App, t: &Theme, user: i64, today: NaiveDate, reduced: bool) -> Vec<El> {
    let tracker = &s.monster;
    let Some(visit) = tracker.visit.as_ref().filter(|visit| visit.user == user) else {
        return vec![muted(t, "This person is no longer on the leaderboard.")];
    };
    let person = tracker.people.iter().find(|person| person.user == user);
    let name = if visit.name.is_empty() {
        person.map_or("Someone", |person| person.name.as_str())
    } else {
        visit.name.as_str()
    };
    let place = monster::ranked(&tracker.people, tracker.ranking, !tracker.hidden)
        .iter()
        .position(|row| row.user == user)
        .map(|position| position + 1);
    // The server's count, which leaves out cards moved from a computer.
    let standing = person.map_or_else(
        || {
            visit
                .cans
                .as_deref()
                .map(monster::standing)
                .unwrap_or_default()
        },
        |person| person.standing,
    );
    let mut summary = format!(
        "{} / {CARDS} cards · {} / {} flavours · {} Legendary",
        standing.cards,
        standing.flavours,
        FLAVOURS.len(),
        standing.legendary
    );
    if let Some(place) = place {
        summary = format!(
            "#{place} by {} · {summary}",
            tracker.ranking.name().to_lowercase()
        );
    }
    let header = row()
        .key("monster-person")
        .w_full()
        .gap(14.0)
        .center_y()
        .child(avatar(t, name, 56.0, false))
        .child(
            col()
                .grow(1.0)
                .gap(4.0)
                .child(title(t, name.to_owned()))
                .child(muted(t, summary)),
        );
    if let Some(problem) = &visit.problem {
        return vec![
            header,
            error_text(t, problem.clone()).key("monster-person-problem"),
        ];
    }
    let Some(cans) = &visit.cans else {
        return vec![
            header,
            well(t)
                .key("monster-person-loading")
                .w_full()
                .padding(16.0)
                .child(muted(t, format!("Loading {name}'s board…"))),
        ];
    };
    let board = Board::new(cans, today, |at| local_date(at).unwrap_or(today));
    let mut collection = col()
        .key("person-collection")
        .w_full()
        .gap(10.0)
        .child(group_title(t, format!("Collection · {}", cans.len())));
    collection = if cans.is_empty() {
        collection.child(muted(t, "No cards yet."))
    } else {
        collection
            .child(cards(
                t,
                cans.iter().rev(),
                tracker.shown,
                None,
                s.ui.can_hover.as_deref(),
                false,
                reduced,
            ))
            .children(more(t, cans.len(), tracker.shown))
    };
    vec![
        header,
        profile(s, t, &format!("{name}'s Board"), &board, today),
        collection,
    ]
}

/// The header's title for the part of the page on show.
pub fn header_title(s: &App) -> &'static str {
    if s.monster.gate.is_some() && s.monster.online() {
        return "Monster Collection";
    }
    match s.monster.section {
        Section::Overview => "Monster Collection",
        Section::Flavours => "Flavours",
        Section::Flavour(index) => FLAVOURS.get(index).map_or("Flavours", |f| f.name),
        Section::Leaderboard | Section::Person(_) => "Leaderboard",
    }
}

fn tabs(s: &App, t: &Theme, section: Section) -> El {
    let tab = |key: &'static str, label: &'static str, active: bool, target: Section| {
        button(
            t,
            key,
            label,
            None,
            if active {
                Variant::Active
            } else {
                Variant::Ghost
            },
            Some(Message::MonsterShow(target)),
        )
    };
    let tabs = row()
        .key("monster-tabs")
        .gap(size::GAP)
        .child(tab(
            "monster-tab-overview",
            "Overview",
            section == Section::Overview,
            Section::Overview,
        ))
        .child(tab(
            "monster-tab-flavours",
            "Flavours",
            matches!(section, Section::Flavours | Section::Flavour(_)),
            Section::Flavours,
        ))
        .child(tab(
            "monster-tab-leaderboard",
            "Leaderboard",
            matches!(section, Section::Leaderboard | Section::Person(_)),
            Section::Leaderboard,
        ));
    let mut bar = row()
        .w_full()
        .gap(size::GAP)
        .center_y()
        .flex_wrap()
        .child(tabs.grow(1.0));
    // Who is signed in, and the way out.
    if let Some(me) = &s.monster.me {
        bar = bar.child(
            row()
                .key("monster-account")
                .gap(size::GAP)
                .center_y()
                .child(icon(&icons::USER, 14.0).color(t.faint).a11y_hidden())
                .child(faint(t, me.email.clone()).truncate())
                .child(button(
                    t,
                    "monster-sign-out",
                    "Sign Out",
                    None,
                    Variant::Ghost,
                    Some(Message::Monster(Action::SignOut)),
                )),
        );
    }
    bar
}

fn overview(s: &App, t: &Theme, k: &Kit, today: NaiveDate, reduced: bool) -> Vec<El> {
    let board = Board::new(&s.monster.cans, today, |at| local_date(at).unwrap_or(today));
    vec![
        row()
            .w_full()
            .gap(20.0)
            .flex_wrap()
            .child(showcase(s, reduced))
            .child(add_panel(s, t, k)),
        profile(s, t, "Your Board", &board, today),
        collection(s, t, reduced),
    ]
}

fn local_date(at: i64) -> Option<NaiveDate> {
    Local
        .timestamp_opt(at, 0)
        .earliest()
        .map(|time| time.date_naive())
}

fn cabinet() -> El {
    el().radius(radius::CARD).fill(rgb(CABINET))
}

// ---------------------------------------------------------------- cards

/// A full-size effect layer filling its card.
fn wash(part: CardFx, can: &Can, reduced: bool, corner: f32) -> El {
    card_fx(part, can.flavour_index(), can.rarity(), can.seed(), reduced)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(corner)
        .a11y_hidden()
}

/// One card, `w` points wide. Its can, name and number are the same at every rarity; what
/// changes is how many shaders it takes to draw.
/// `look` is what the can in the middle should know: `SHOWCASE`, `HOVERED` or neither.
fn card(can: &Can, w: f32, look: u32, reduced: bool) -> El {
    let h = w * CARD_RATIO;
    let corner = w * 0.075;
    let rarity = can.rarity();
    let flavour = can.flavour();
    let (behind, over) = card_layers(rarity);
    let large = w >= LARGE_CARD;
    let inset = w * 0.05;

    let mut e = el().size(w, h).radius(corner);
    for part in behind {
        e = e.child(wash(*part, can, reduced, corner));
    }
    e = e.child(drink(can, w, h, look, reduced));
    for part in over {
        e = e.child(wash(*part, can, reduced, corner));
    }
    let name_style = if large {
        ty::SUBTITLE
    } else {
        ty::SMALL_STRONG
    };
    let number_style = if large {
        ty::NUMERIC
    } else {
        TextStyle::new(10.0, 500.0)
    };
    let plate_h = h * 0.2;
    e.child(
        el().absolute(inset * 1.4, inset * 1.4)
            .h(if large { 22.0 } else { 18.0 })
            .px(if large { 9.0 } else { 7.0 })
            .center_y()
            .radius(radius::PILL)
            .fill(Color::BLACK.alpha(0.66))
            .child(
                text(rarity.name())
                    .font(if large {
                        ty::SMALL_STRONG
                    } else {
                        TextStyle::new(10.0, 600.0)
                    })
                    .color(rgb(rarity.color())),
            ),
    )
    .child(
        col()
            .absolute(inset * 1.4, h - plate_h - inset * 1.4)
            .w(w - inset * 2.8)
            .h(plate_h)
            .px(inset * 1.4)
            .gap(if large { 3.0 } else { 1.0 })
            .justify_center()
            .radius(corner * 0.7)
            .fill(Color::BLACK.alpha(0.8))
            .child(
                text(flavour.name)
                    .font(name_style)
                    .color(Color::WHITE)
                    .truncate(),
            )
            .child(
                text(can.number())
                    .font(number_style)
                    .color(Color::WHITE.alpha(0.74))
                    .truncate(),
            ),
    )
}

/// Where the can sits on a card `w` by `h`: x, y, width and height.
fn can_box(w: f32, h: f32) -> [f32; 4] {
    let (box_w, box_h) = (w * 0.6, h * 0.615);
    [(w - box_w) / 2.0, h * 0.12, box_w, box_h]
}

/// The can in the middle of a card, in real 3D.
fn drink(can: &Can, w: f32, h: f32, look: u32, reduced: bool) -> El {
    let [x, y, box_w, box_h] = can_box(w, h);
    let look = look | if reduced { REDUCED } else { 0 };
    can3d::can(can.flavour_index(), can.rarity(), can.seed(), look)
        .absolute(x, y)
        .size(box_w, box_h)
        .a11y_hidden()
}

/// The big card, or an empty slot waiting for the first can.
fn showcase(s: &App, reduced: bool) -> El {
    large_card(
        s.monster.featured(),
        s.monster.reveal.as_ref(),
        "Your first card lands here",
        reduced,
    )
}

/// A one-point speck of every effect a reveal uses. Shaders compile the first time they draw,
/// and without this the first Legendary would stutter at the worst possible moment.
fn warm_up(reduced: bool) -> El {
    // Clipped, because glow and fire reach well outside their box.
    // Placed where every section's cabinet, drawn after it, covers it.
    let mut speck = el()
        .absolute(size::PAGE + 40.0, 150.0)
        .size(1.0, 1.0)
        .clip_rounded(0.0)
        .a11y_hidden();
    for part in [
        CardFx::Back,
        CardFx::RevealGlow,
        CardFx::RevealSparkles,
        CardFx::RevealElectric,
        CardFx::RevealFire,
        CardFx::Ripple,
    ] {
        speck = speck.child(
            card_fx(part, 0, Rarity::Legendary, 100, reduced)
                .absolute(0.0, 0.0)
                .size(1.0, 1.0),
        );
    }
    speck.child(
        can3d::mystery(None, 0.0, reduced)
            .absolute(0.0, 0.0)
            .size(1.0, 1.0),
    )
}

/// One layer of the face-down card, filling it.
fn reveal_wash(part: CardFx, rung: Rarity, level: f32, reduced: bool) -> El {
    card_fx(part, 0, rung, (level * 100.0) as u16, reduced)
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(LARGE_CARD * 0.075)
        .a11y_hidden()
}

/// The card being opened at this beat: its back, lit up to the rarity it has reached so far.
fn revealing(reveal: &Reveal, can: &Can, reduced: bool) -> El {
    let (w, h) = (LARGE_CARD, LARGE_CARD * CARD_RATIO);
    let beat = reveal.beat();
    if beat.face_up() {
        let mut face = el()
            .key("reveal-face")
            .size(w, h)
            .transition(Transition::scale(1.04))
            .motion(motion::QUICK)
            .child(card(can, w, SHOWCASE, reduced));
        // One-shots can't be stilled, so Reduce Motion goes without them.
        if beat == Beat::Hit && !reduced {
            face = face
                .child(reveal_wash(CardFx::Ripple, reveal.rarity, 1.0, reduced))
                .child(reveal_wash(CardFx::Burst, reveal.rarity, 1.0, reduced));
        }
        return face;
    }
    let (rung, level) = match beat {
        Beat::Rung(rung) => (Some(rung), 1.0),
        Beat::Hush(Rarity::Legendary) => (Some(Rarity::Legendary), 0.25),
        Beat::Hush(rung) => (Some(rung), 0.5),
        _ => (None, 0.0),
    };
    let mut back = el()
        .key("reveal-back")
        .size(w, h)
        .radius(w * 0.075)
        .transition(Transition::scale(0.94))
        .motion(motion::QUICK)
        .center();
    if rung == Some(Rarity::Legendary) {
        back = back.child(reveal_wash(
            CardFx::RevealFire,
            Rarity::Legendary,
            level,
            reduced,
        ));
    }
    if let Some(rung) = rung {
        back = back.child(reveal_wash(CardFx::RevealGlow, rung, level, reduced));
    }
    // An unprinted can stands where the card's own will, so the turn only has to dress it.
    let [x, y, box_w, box_h] = can_box(w, h);
    back = back
        .child(reveal_wash(CardFx::Back, Rarity::Common, 1.0, reduced))
        .child(
            can3d::mystery(rung, level, reduced)
                .absolute(x, y)
                .size(box_w, box_h)
                .a11y_hidden(),
        );
    if let Some(rung) = rung.filter(|rung| *rung >= Rarity::Rare) {
        back = back.child(reveal_wash(CardFx::RevealSparkles, rung, level, reduced));
    }
    if let Some(rung) = rung.filter(|rung| *rung >= Rarity::Mythic) {
        back = back
            .child(reveal_wash(CardFx::RevealElectric, rung, level, reduced))
            // Mythic and up run for seconds, so say they can be cut short. They've already
            // shown their colour by now, so this gives nothing away.
            .child(
                el().absolute(0.0, h - 44.0).w_full().center().child(
                    text("Click or press Esc to skip")
                        .font(ty::CAPTION)
                        .color(Color::WHITE.alpha(0.7)),
                ),
            );
    }
    back
}

/// A card shown large on the cabinet, or `empty` in its place.
fn large_card(can: Option<&Can>, reveal: Option<&Reveal>, empty: &str, reduced: bool) -> El {
    let frame = cabinet()
        .size(300.0, LARGE_CARD * CARD_RATIO + 56.0)
        .center();
    let Some(can) = can else {
        return frame.key("monster-showcase").child(
            slot(LARGE_CARD).center().child(
                text(empty.to_owned())
                    .font(ty::SMALL)
                    .color(Color::WHITE.alpha(0.6)),
            ),
        );
    };
    let identity = can.identity();
    let shell = el()
        .key(format!("showcase-{identity}"))
        .role("img")
        .transition(Transition::scale(0.9))
        .motion(motion::SETTLE);
    let Some(reveal) = reveal.filter(|reveal| reveal.identity == identity) else {
        return frame.key("monster-showcase").child(
            shell
                .label(card_label(can))
                .cursor(rune::Cursor::Grab)
                .help("Drag to spin the can")
                .on_drag(|s: &mut App, drag: &mut Drag, tx: &mut Tx| {
                    tx.instant();
                    s.ui.can_spin.drag(drag);
                })
                .child(card(can, LARGE_CARD, SHOWCASE, reduced)),
        );
    };
    let frame = frame.key("monster-showcase");
    frame.child(
        shell
            .label(if reveal.beat().face_up() {
                card_label(can)
            } else {
                "Opening a card".to_owned()
            })
            .cursor(rune::Cursor::Pointer)
            .on_press_tx(send(Message::MonsterRevealSkip))
            .child(revealing(reveal, can, reduced)),
    )
}

/// The outline of a card that isn't there.
fn slot(w: f32) -> El {
    el().size(w, w * CARD_RATIO)
        .radius(w * 0.075)
        .border(1.0, Color::WHITE.alpha(0.14))
}

fn card_label(can: &Can) -> String {
    format!(
        "{} {}, can {}",
        can.rarity().name(),
        can.flavour().name,
        can.number()
    )
}

// ---------------------------------------------------------------- adding a can

fn add_panel(s: &App, t: &Theme, k: &Kit) -> El {
    let tracker = &s.monster;
    let open = tracker.load_error.is_none()
        && tracker.reveal.is_none()
        && !tracker.adding
        && !tracker.loading;
    let searching = !tracker.query.trim().is_empty();
    let order = monster::favourites(&tracker.cans, &tracker.query);
    let favourite_count = if searching {
        0
    } else {
        order
            .iter()
            .take(FAVOURITES)
            .take_while(|(_, count)| *count > 0)
            .count()
    };
    let (favourites, rest) = order.split_at(favourite_count);
    let mut panel = col()
        .key("monster-add")
        .grow(1.0)
        .min_w(320.0)
        .gap(8.0)
        .child(heading(t, "Add a Can"))
        .child(muted(
            t,
            "Pick the flavour you just drank. It's saved with today's date and time.",
        ))
        .children(
            tracker
                .loading
                .then(|| faint(t, "Loading your collection…").key("monster-loading")),
        )
        .child(
            el().key("monster-search").w_full().child(field(
                k,
                &s.ui.monster_search,
                |s: &mut App| &mut s.ui.monster_search,
                "Search flavours",
                Some(Message::MonsterSearch),
                // Enter takes the first match, so a search and Enter is the quickest way in.
                order
                    .first()
                    .filter(|_| searching && open)
                    .map(|(index, _)| Message::MonsterAdd(*index)),
            )),
        );
    if !favourites.is_empty() {
        panel = panel
            .child(group_title(t, "Your Favourites").mt(4.0))
            .child(flavour_buttons(t, favourites, open));
    }
    if rest.is_empty() {
        if searching {
            panel = panel.child(muted(t, "No flavour by that name. Try “Other”.").mt(4.0));
        }
    } else {
        let label = if searching {
            "Matches"
        } else if favourites.is_empty() {
            "Flavours"
        } else {
            "Other Flavours"
        };
        panel = panel
            .child(group_title(t, label).mt(4.0))
            .child(flavour_buttons(t, rest, open));
    }
    panel = panel.child(notice(s, t));
    if let Some(error) = &tracker.load_error {
        panel = panel.child(error_text(
            t,
            format!("{error}. Fix or remove the file to add cans."),
        ));
    }
    let face_down = tracker.hidden_can().is_some();
    if let Some(can) = tracker.featured().filter(|_| !face_down) {
        panel = panel.child(featured(t, can));
    }
    panel.child(odds(s, t))
}

/// How many of the most-drunk flavours lead the list.
const FAVOURITES: usize = 6;

/// One button for each flavour: tapping it records a can of it, drunk now. `open` is false
/// while a card is being opened, or the collection can't be saved.
fn flavour_buttons(t: &Theme, flavours: &[(usize, u32)], open: bool) -> El {
    let mut wrap = row().w_full().gap(6.0).flex_wrap();
    for &(index, count) in flavours {
        let flavour = &FLAVOURS[index];
        let [_, main, high, _] = flavour.palette;
        let mut button = row()
            .key(format!("monster-flavour-{}", flavour.id))
            .role("button")
            .label(format!("Add a can of {}", flavour.name))
            .h(size::CONTROL)
            .pl(8.0)
            .pr(10.0)
            .gap(7.0)
            .center_y()
            .radius(radius::CONTROL)
            .fill(t.recess)
            .border(1.0, t.hairline)
            .child(
                el().size(12.0, 16.0)
                    .radius(3.0)
                    .fill(rgb(main))
                    .border(1.0, rgb(high).alpha(0.6))
                    .a11y_hidden(),
            )
            .child(
                text(flavour.name)
                    .font(ty::LABEL)
                    .color(t.text)
                    .a11y_hidden(),
            );
        if count > 0 {
            button = button.child(
                text(format!("×{count}"))
                    .font(ty::CAPTION)
                    .color(t.faint)
                    .a11y_hidden(),
            );
        }
        button = if open {
            button
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
                .hover(t.wash(0.06))
                .press_scale(0.97)
                .motion(motion::QUICK)
                .on_press_tx(send(Message::MonsterAdd(index)))
        } else {
            button.disabled(true).dim(0.5)
        };
        wrap = wrap.child(button);
    }
    wrap
}

fn notice(s: &App, t: &Theme) -> El {
    let Some(notice) = &s.monster.notice else {
        return el();
    };
    // Saying what it is would give the reveal away.
    if matches!(notice, Notice::Added(_)) && s.monster.hidden_can().is_some() {
        return el();
    }
    match notice {
        Notice::Added(rarity) => row()
            .key("monster-notice")
            .gap(6.0)
            .center_y()
            .child(dot(rgb(rarity.color()), 8.0))
            .child(
                text(format!("{} card added", rarity.name()))
                    .font(ty::SMALL)
                    .color(t.text),
            ),
        Notice::Removed => muted(t, "Can removed").key("monster-notice"),
        Notice::Failed(error) => error_text(t, error.clone()).key("monster-notice"),
    }
}

/// Facts about the card on show, and the way to take a mistyped can back out.
fn featured(t: &Theme, can: &Can) -> El {
    let rarity = can.rarity();
    row()
        .key("monster-featured")
        .w_full()
        .mt(6.0)
        .pt(10.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hairline)
        .gap(size::GAP)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(
                    row()
                        .gap(6.0)
                        .center_y()
                        .child(dot(rgb(rarity.color()), 8.0))
                        .child(
                            text(format!("{} {}", rarity.name(), can.flavour().name))
                                .font(ty::BODY_STRONG)
                                .color(t.text)
                                .truncate(),
                        ),
                )
                .child(faint(t, format!("{} · {}", can.number(), when(can.at))).truncate()),
        )
        .child(icon_button(
            t,
            "monster-replay",
            &icons::PLAY,
            "Replay the reveal",
            Variant::Ghost,
            Some(Message::MonsterReplay(can.identity())),
        ))
        .child(icon_button(
            t,
            "monster-remove",
            &icons::TRASH,
            "Remove this can",
            Variant::Ghost,
            Some(Message::MonsterRemove(can.identity())),
        ))
}

fn dot(color: Color, diameter: f32) -> El {
    el().size(diameter, diameter)
        .radius(diameter / 2.0)
        .fill(color)
        .a11y_hidden()
}

/// The odds, and how many of each you've pulled.
fn odds(s: &App, t: &Theme) -> El {
    let mut counts = [0u32; 5];
    let face_down = s.monster.hidden_can();
    for can in &s.monster.cans {
        if face_down != Some(can.identity().as_str()) {
            counts[can.rarity().index()] += 1;
        }
    }
    let mut tally = row()
        .key("monster-odds")
        .w_full()
        .mt(6.0)
        .pt(10.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], t.hairline)
        .gap(14.0)
        .flex_wrap();
    for rarity in Rarity::ALL {
        let percent = rarity.permille() as f32 / 10.0;
        tally = tally.child(
            col()
                .gap(1.0)
                .help(format!("{percent}% of cans are {}", rarity.name()))
                .child(
                    row()
                        .gap(5.0)
                        .center_y()
                        .child(dot(rgb(rarity.color()), 7.0))
                        .child(text(rarity.name()).font(ty::SMALL).color(t.muted)),
                )
                .child(
                    text(format!("{} · {percent}%", counts[rarity.index()]))
                        .font(ty::NUMERIC)
                        .color(t.text),
                ),
        );
    }
    tally
}

// ---------------------------------------------------------------- the board

fn profile(s: &App, t: &Theme, heading: &str, board: &Board, today: NaiveDate) -> El {
    col()
        .key("monster-board")
        .w_full()
        .gap(14.0)
        .child(title(t, heading))
        .child(stats(t, board))
        .child(heatmap(s, t, board, today))
        .child(flavours(t, board))
}

fn days(n: u32) -> String {
    if n == 1 {
        "1 day".into()
    } else {
        format!("{n} days")
    }
}

fn stat(t: &Theme, label: &str, value: String) -> El {
    col()
        .gap(2.0)
        .child(text(value).font(ty::BIG_VALUE).color(t.text))
        .child(muted(t, label))
}

fn stats(t: &Theme, board: &Board) -> El {
    let stat = |label: &str, value: String| stat(t, label, value).grow(1.0).min_w(104.0);
    row()
        .key("monster-stats")
        .w_full()
        .gap(16.0)
        .flex_wrap()
        .child(stat("Cans", board.total.to_string()))
        .child(stat(
            "Flavours",
            format!("{} / {}", board.flavours, FLAVOURS.len()),
        ))
        .child(stat("Current Streak", days(board.current_streak)))
        .child(stat("Longest Streak", days(board.longest_streak)))
        .child(stat("Best Day", board.best_day.to_string()))
        .child(stat("Drunk", format!("{:.1} L", board.litres)))
        .child(stat("Caffeine", format!("{:.1} g", board.caffeine_g)))
}

fn format_date(date: NaiveDate) -> String {
    date.format("%b %-d, %Y").to_string()
}

/// When a can was drunk, as people say it: "today at 14:32", or a date and time.
fn when(at: i64) -> String {
    let Some(moment) = Local.timestamp_opt(at, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let time = moment.format("%H:%M");
    match (today - moment.date_naive()).num_days() {
        0 => format!("today at {time}"),
        1 => format!("yesterday at {time}"),
        _ => format!("{} at {time}", format_date(moment.date_naive())),
    }
}

/// A year of squares, one for each day, as on a GitHub profile: weeks run left to right,
/// Sunday at the top, today in the last column. A narrow window shows fewer weeks.
fn heatmap(s: &App, t: &Theme, board: &Board, today: NaiveDate) -> El {
    let step = CELL + CELL_GAP;
    // The page's padding on both sides, the well's, and the weekday labels.
    let room = (s.window_size.0.min(1100.0) - size::PAGE * 2.0 - 28.0 - WEEKDAY_COLUMN).max(0.0);
    let weeks = (((room + CELL_GAP) / step) as usize).clamp(MIN_WEEKS, MAX_WEEKS);
    let this_sunday = today - Duration::days(i64::from(today.weekday().num_days_from_sunday()));
    let first = this_sunday - Duration::weeks(weeks as i64 - 1);

    let mut grid = row().gap(CELL_GAP);
    let mut months: Vec<(u32, usize)> = Vec::new();
    for week in 0..weeks {
        let sunday = first + Duration::weeks(week as i64);
        match months.last_mut() {
            Some((month, run)) if *month == sunday.month() => *run += 1,
            _ => months.push((sunday.month(), 1)),
        }
        let mut column = col().gap(CELL_GAP);
        for offset in 0..7 {
            let day = sunday + Duration::days(offset);
            column = column.child(if day > today {
                el().size(CELL, CELL).a11y_hidden()
            } else {
                day_cell(t, day, board.days.get(&day).copied().unwrap_or(0))
            });
        }
        grid = grid.child(column);
    }

    let mut month_row = row().pl(WEEKDAY_COLUMN);
    for (month, run) in months {
        let width = run as f32 * step;
        let mut slot = el().w(width).h(14.0);
        if run >= 2 {
            let name = NaiveDate::from_ymd_opt(today.year(), month, 1)
                .map_or_else(String::new, |d| d.format("%b").to_string());
            slot = slot.child(text(name).font(ty::CAPTION).color(t.faint));
        }
        month_row = month_row.child(slot);
    }

    let mut weekdays = col().w(WEEKDAY_COLUMN).gap(CELL_GAP).a11y_hidden();
    for label in ["", "Mon", "", "Wed", "", "Fri", ""] {
        weekdays = weekdays.child(
            el().h(CELL)
                .center_y()
                .child(text(label).font(TextStyle::new(10.0, 400.0)).color(t.faint)),
        );
    }

    let mut legend = row()
        .gap(CELL_GAP)
        .center_y()
        .child(faint(t, "Less").mr(4.0));
    for level in 0..=4 {
        legend = legend.child(
            el().size(CELL, CELL)
                .radius(2.0)
                .fill(level_color(t, level)),
        );
    }
    legend = legend.child(faint(t, "More").ml(4.0));

    let caption = match board.past_year {
        1 => "1 can in the last year".to_owned(),
        n => format!("{n} cans in the last year"),
    };
    well(t)
        .key("monster-heatmap")
        .w_full()
        .padding(14.0)
        .gap(6.0)
        .child(
            row()
                .w_full()
                .center_y()
                .child(heading(t, caption).grow(1.0))
                .child(legend),
        )
        .child(month_row)
        .child(row().child(weekdays).child(grid))
}

fn level_color(t: &Theme, level: u32) -> Color {
    match level {
        0 => t.wash(0.07),
        1 => t.accent.alpha(0.32),
        2 => t.accent.alpha(0.55),
        3 => t.accent.alpha(0.78),
        _ => t.accent,
    }
}

fn day_cell(t: &Theme, day: NaiveDate, cans: u32) -> El {
    let label = match cans {
        0 => format!("No cans on {}", format_date(day)),
        1 => format!("1 can on {}", format_date(day)),
        n => format!("{n} cans on {}", format_date(day)),
    };
    el().size(CELL, CELL)
        .radius(2.0)
        .fill(level_color(t, cans.min(4)))
        .help(label)
}

/// What you drink most: one bar in flavour colours, like a repository's languages.
fn flavours(t: &Theme, board: &Board) -> El {
    let total = board.total.max(1) as f32;
    let shown = board.by_flavour.len().min(LEGEND_FLAVOURS);
    let others: u32 = board.by_flavour[shown..].iter().map(|(_, n)| n).sum();
    let mut bar = row()
        .w_full()
        .h(8.0)
        .radius(4.0)
        .clip_rounded(4.0)
        .fill(t.wash(0.07))
        .gap(1.0)
        .a11y_hidden();
    let mut legend = row().w_full().gap(14.0).flex_wrap();
    let mut entry = |key: &str, name: &str, color: Color, count: u32, target, bar_ref: &mut El| {
        *bar_ref =
            std::mem::replace(bar_ref, el()).child(el().grow(count as f32).h_full().fill(color));
        legend = std::mem::replace(&mut legend, el()).child(
            row()
                .key(format!("legend-{key}"))
                .role("button")
                .label(name.to_owned())
                .gap(5.0)
                .px(4.0)
                .py(2.0)
                .center_y()
                .radius(radius::CONTROL)
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(t.wash(0.06))
                .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
                .on_press_tx(send(Message::MonsterShow(target)))
                .child(dot(color, 8.0))
                .child(text(name.to_owned()).font(ty::SMALL).color(t.text))
                .child(faint(t, format!("{:.1}%", count as f32 / total * 100.0))),
        );
    };
    for (flavour, count) in &board.by_flavour[..shown] {
        let index = monster::flavour_by_id(flavour.id).map_or(0, |(index, _)| index);
        entry(
            flavour.id,
            flavour.name,
            rgb(flavour.palette[1]),
            *count,
            Section::Flavour(index),
            &mut bar,
        );
    }
    if others > 0 {
        entry(
            "others",
            "Others",
            t.faint,
            others,
            Section::Flavours,
            &mut bar,
        );
    }
    let mut section = col()
        .key("monster-flavours")
        .w_full()
        .gap(8.0)
        .child(heading(t, "Most Drunk Flavours"))
        .child(bar);
    if board.total == 0 {
        section = section.child(faint(t, "Nothing yet. Your first can starts the bar."));
    } else {
        section = section.child(legend);
    }
    section
}

// ---------------------------------------------------------------- the collection

fn collection(s: &App, t: &Theme, reduced: bool) -> El {
    let tracker = &s.monster;
    let section = col()
        .key("monster-collection")
        .w_full()
        .gap(10.0)
        .child(group_title(
            t,
            format!("Collection · {}", tracker.cans.len()),
        ));
    if tracker.cans.is_empty() {
        return section.child(muted(t, "Cards you collect are kept here."));
    }
    let selected = tracker.featured().map(Can::identity);
    section
        .child(cards(
            t,
            // A card still face down would give itself away here.
            tracker
                .cans
                .iter()
                .rev()
                .filter(|can| tracker.hidden_can() != Some(can.identity().as_str())),
            tracker.shown,
            selected.as_deref(),
            s.ui.can_hover.as_deref(),
            true,
            reduced,
        ))
        .children(more(t, tracker.cans.len(), tracker.shown))
}

/// A cabinet of the first `shown` of these cards.
fn cards<'a>(
    t: &Theme,
    cans: impl Iterator<Item = &'a Can>,
    shown: usize,
    selected: Option<&str>,
    hover: Option<&str>,
    // False for someone else's cards, which can't be featured or removed.
    pressable: bool,
    reduced: bool,
) -> El {
    let mut wrap = row().w_full().gap(10.0).flex_wrap().justify_center();
    for can in cans.take(shown) {
        wrap = wrap.child(tile(
            t,
            format!("card-{}", can.identity()),
            can,
            selected == Some(can.identity().as_str()),
            hover,
            pressable,
            reduced,
        ));
    }
    cabinet().w_full().padding(16.0).child(wrap)
}

fn more(t: &Theme, total: usize, shown: usize) -> Option<El> {
    (total > shown).then(|| {
        row().w_full().justify_center().child(button(
            t,
            "monster-more",
            "Show More",
            None,
            Variant::Secondary,
            Some(Message::MonsterMore),
        ))
    })
}

/// A card in the grid. Pressing it brings it up large.
fn tile(
    t: &Theme,
    key: String,
    can: &Can,
    selected: bool,
    hover: Option<&str>,
    pressable: bool,
    reduced: bool,
) -> El {
    let look = if hover == Some(key.as_str()) {
        HOVERED
    } else {
        0
    };
    let tile = el()
        .key(key.clone())
        .on_hover_change(hovering(key))
        .label(card_label(can))
        .help_when_truncated(can.flavour().name)
        .padding(3.0);
    if !pressable {
        return tile.child(card(can, SMALL_CARD, look, reduced));
    }
    tile.role("button")
        .checked(selected)
        .radius(SMALL_CARD * 0.075 + 3.0)
        .border(
            2.0,
            if selected {
                t.accent
            } else {
                Color::TRANSPARENT
            },
        )
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
        .press_scale(0.97)
        .motion(motion::QUICK)
        .on_press_tx(send(Message::MonsterSelect(can.identity())))
        .child(card(can, SMALL_CARD, look, reduced))
}

/// Remembers which card the pointer is over, so its can can lean toward it.
fn hovering(key: String) -> impl Fn(&mut App, bool, &mut Tx) {
    move |s: &mut App, inside: bool, _: &mut Tx| {
        if inside {
            s.ui.can_hover = Some(key.clone());
        } else if s.ui.can_hover.as_deref() == Some(key.as_str()) {
            s.ui.can_hover = None;
        }
    }
}

// ---------------------------------------------------------------- flavours

/// Every flavour, the ones you drink most first and the ones you've never found last.
fn flavour_list(s: &App, t: &Theme, reduced: bool) -> Vec<El> {
    let mut groups: Vec<(usize, Vec<Can>)> = (0..FLAVOURS.len())
        .map(|index| (index, s.monster.cans_of(index)))
        .collect();
    // Stable, so flavours with the same count stay in the catalogue's order.
    groups.sort_by_key(|(_, cans)| std::cmp::Reverse(cans.len()));
    let found = groups.iter().filter(|(_, cans)| !cans.is_empty()).count();
    let mut wrap = row().w_full().gap(10.0).flex_wrap().justify_center();
    for (index, cans) in &groups {
        wrap = wrap.child(flavour_tile(
            t,
            *index,
            &FLAVOURS[*index],
            cans,
            s.ui.can_hover.as_deref(),
            reduced,
        ));
    }
    vec![
        col()
            .key("monster-flavour-list")
            .w_full()
            .gap(10.0)
            .child(group_title(
                t,
                format!("Found {found} of {}", FLAVOURS.len()),
            ))
            .child(cabinet().w_full().padding(16.0).child(wrap)),
    ]
}

/// A flavour's best card with its count, and a dot for each rarity you've pulled.
fn flavour_tile(
    t: &Theme,
    index: usize,
    flavour: &Flavour,
    cans: &[Can],
    hover: Option<&str>,
    reduced: bool,
) -> El {
    let key = format!("flavour-{}", flavour.id);
    let look = if hover == Some(key.as_str()) {
        HOVERED
    } else {
        0
    };
    let face = match monster::best(cans) {
        Some(can) => el()
            .size(SMALL_CARD, SMALL_CARD * CARD_RATIO)
            .child(card(can, SMALL_CARD, look, reduced))
            .child(
                row()
                    .absolute(0.0, 0.0)
                    .w(SMALL_CARD)
                    .justify_end()
                    .pt(10.5)
                    .pr(10.5)
                    .child(
                        el().h(18.0)
                            .px(7.0)
                            .center_y()
                            .radius(radius::PILL)
                            .fill(Color::BLACK.alpha(0.66))
                            .child(
                                text(format!("×{}", cans.len()))
                                    .font(TextStyle::new(10.0, 600.0).features("'tnum' 1"))
                                    .color(Color::WHITE),
                            ),
                    ),
            ),
        None => missing(SMALL_CARD, flavour),
    };
    let mut pulled = [false; 5];
    for can in cans {
        pulled[can.rarity().index()] = true;
    }
    let mut rarities = row().gap(5.0).center_y().a11y_hidden();
    for rarity in Rarity::ALL {
        rarities = rarities.child(dot(
            if pulled[rarity.index()] {
                rgb(rarity.color())
            } else {
                Color::WHITE.alpha(0.16)
            },
            7.0,
        ));
    }
    col()
        .key(key.clone())
        .on_hover_change(hovering(key))
        .role("button")
        .label(match cans.len() {
            0 => format!("{}, not found yet", flavour.name),
            1 => format!("{}, 1 can", flavour.name),
            n => format!("{}, {n} cans", flavour.name),
        })
        .help_when_truncated(flavour.name)
        .gap(7.0)
        .center()
        .padding(3.0)
        .radius(SMALL_CARD * 0.075 + 3.0)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
        .press_scale(0.97)
        .motion(motion::QUICK)
        .on_press_tx(send(Message::MonsterShow(Section::Flavour(index))))
        .child(face)
        .child(rarities)
}

/// The slot of a flavour you haven't found: its colours and name, and nothing more.
fn missing(w: f32, flavour: &Flavour) -> El {
    let mut swatches = row().gap(4.0).a11y_hidden();
    for color in flavour.palette {
        swatches = swatches.child(dot(rgb(color), 9.0));
    }
    slot(w).child(
        col()
            .w_full()
            .h_full()
            .padding(14.0)
            .gap(6.0)
            .child(swatches)
            .child(el().grow(1.0))
            .child(
                text(flavour.name)
                    .font(ty::SMALL_STRONG)
                    .color(Color::WHITE.alpha(0.82))
                    .wrap(),
            )
            .child(
                text("Not found yet")
                    .font(ty::CAPTION)
                    .color(Color::WHITE.alpha(0.5)),
            ),
    )
}

/// One flavour's page: its best card, what you've pulled of it, when, and every card.
fn flavour_page(s: &App, t: &Theme, index: usize, today: NaiveDate, reduced: bool) -> Vec<El> {
    let Some(flavour) = FLAVOURS.get(index) else {
        return Vec::new();
    };
    let tracker = &s.monster;
    let cans = tracker.cans_of(index);
    let board = Board::new(&cans, today, |at| local_date(at).unwrap_or(today));
    // The card you pressed, if it's one of these; otherwise the best of them.
    let hero = tracker
        .selected
        .as_deref()
        .and_then(|identity| cans.iter().find(|can| can.identity() == identity))
        .or_else(|| monster::best(&cans));
    let selected = hero.map(Can::identity);
    let selected = selected.as_deref();

    let mut facts = col()
        .key("flavour-facts")
        .grow(1.0)
        .min_w(320.0)
        .gap(14.0)
        .child(muted(t, flavour.line))
        .child(flavour_stats(t, s, &cans, &board));
    if let Some(can) = hero {
        facts = facts.child(featured(t, can));
    }

    let mut all = col()
        .key("flavour-cards")
        .w_full()
        .gap(10.0)
        .child(group_title(t, format!("All Cards · {}", cans.len())));
    all = if cans.is_empty() {
        all.child(muted(
            t,
            "Add a can of this flavour and its card shows up here.",
        ))
    } else {
        all.child(cards(
            t,
            cans.iter().rev(),
            tracker.shown,
            selected,
            s.ui.can_hover.as_deref(),
            true,
            reduced,
        ))
        .children(more(t, cans.len(), tracker.shown))
    };

    vec![
        row()
            .w_full()
            .gap(20.0)
            .flex_wrap()
            .child(large_card(
                hero,
                tracker.reveal.as_ref(),
                "None of these yet",
                reduced,
            ))
            .child(facts),
        rarity_breakdown(
            t,
            &cans,
            &board,
            selected,
            s.ui.can_hover.as_deref(),
            reduced,
        ),
        heatmap(s, t, &board, today),
        all,
    ]
}

fn flavour_stats(t: &Theme, s: &App, cans: &[Can], board: &Board) -> El {
    let date = |can: Option<&Can>| {
        can.and_then(|can| local_date(can.at))
            .map_or_else(|| "None yet".to_owned(), format_date)
    };
    // A fixed width, so dates fit and the columns line up however the row wraps.
    let stat = |label: &str, value: String| stat(t, label, value).w(136.0);
    let everything = s.monster.cans.len().max(1) as f32;
    let best = monster::best(cans).map_or("None yet", |can| can.rarity().name());
    row()
        .key("flavour-stats")
        .w_full()
        .gap(16.0)
        .flex_wrap()
        .child(stat("Cans", board.total.to_string()))
        .child(stat(
            "Of All Your Cans",
            format!("{:.1}%", board.total as f32 / everything * 100.0),
        ))
        .child(stat("Best Card", best.to_owned()))
        .child(stat("Longest Streak", days(board.longest_streak)))
        .child(stat("First Can", date(cans.first())))
        .child(stat("Latest Can", date(cans.last())))
        .child(stat("Drunk", format!("{:.1} L", board.litres)))
        .child(stat("Caffeine", format!("{:.1} g", board.caffeine_g)))
}

/// How often each card came up against its odds, under the newest of each.
fn rarity_breakdown(
    t: &Theme,
    cans: &[Can],
    board: &Board,
    selected: Option<&str>,
    hover: Option<&str>,
    reduced: bool,
) -> El {
    let total = board.total.max(1) as f32;
    let mut wrap = row().w_full().gap(12.0).flex_wrap().justify_center();
    for rarity in Rarity::ALL {
        let count = board.by_rarity[rarity.index()];
        let newest = cans.iter().rev().find(|can| can.rarity() == rarity);
        let face = match newest {
            Some(can) => tile(
                t,
                format!("variant-{}", rarity.name()),
                can,
                selected == Some(can.identity().as_str()),
                hover,
                true,
                reduced,
            ),
            None => el()
                .key(format!("variant-empty-{}", rarity.name()))
                .padding(5.0)
                .child(
                    slot(SMALL_CARD).center().child(
                        text("None yet")
                            .font(ty::SMALL)
                            .color(Color::WHITE.alpha(0.5)),
                    ),
                ),
        };
        wrap = wrap.child(
            col()
                .gap(3.0)
                .center()
                .child(face)
                .child(
                    row()
                        .gap(5.0)
                        .center_y()
                        .child(dot(rgb(rarity.color()), 7.0))
                        .child(
                            text(rarity.name())
                                .font(ty::SMALL_STRONG)
                                .color(Color::WHITE),
                        ),
                )
                .child(
                    text(format!("×{count} · {:.1}%", count as f32 / total * 100.0))
                        .font(ty::NUMERIC)
                        .color(Color::WHITE.alpha(0.85)),
                )
                .child(
                    text(format!("Odds {}%", rarity.permille() as f32 / 10.0))
                        .font(ty::CAPTION)
                        .color(Color::WHITE.alpha(0.55)),
                ),
        );
    }
    col()
        .key("flavour-rarities")
        .w_full()
        .gap(10.0)
        .child(group_title(t, "How Often You Got Each Card"))
        .child(cabinet().w_full().padding(16.0).child(wrap))
}
