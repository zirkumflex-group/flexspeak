//! The secret Monster page: add a can by its unique number, get a card, watch the board fill.
//!
//! A card is a stack of rune-fx layers (see `fx::CardFx`) around a can built from more of them.
//! Cards sit on a fixed dark cabinet in every theme, because their glow and fire are light.

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone, Timelike};
use rune::{
    Color, Cx, Drag, El, LineCap, Shape, TextStyle, Transition, Tx, canvas, col, el, icon, row,
    text,
};
use rune_kit::{Kit, Switch};

use super::can3d::{self, HOVERED, REDUCED, SHOWCASE};
use super::charts::color_key;
use super::fx::{CardFx, card_fx, card_layers};
use super::theme::{Theme, motion, radius, rgb, shadow, size, ty};
use super::widgets::{
    Range, Variant, button, divider, error_text, faint, field, form_row, group_note, group_title,
    heading, icon_button, muted, scroll_page, slider, title, well,
};
use super::{icons, send};
use crate::monster::{
    self, Action, Beat, Board, CARDS, Can, FLAVOURS, Flavour, Gate, Notice, Ranking, Rarity,
    Reveal, Section, Standing,
};
use crate::{App, Message, settings};

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
        Section::Overview => ("overview".to_owned(), overview(s, cx, t, k, today, reduced)),
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
        .child(tabs(s, t, section))
        .children(sync_choice(s, t))
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
                        "This build has no collection server, so there is no leaderboard.",
                    )
                    .grow(1.0),
                ),
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
                    "Anyone on the leaderboard can open your board (your stats, heatmap and cards) and see your caffeine beside your name.",
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
        profile(s, t, &format!("{name}'s Board"), &board, today, None),
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
                .children(sync_status(s).map(|line| faint(t, line).key("monster-sync-status")))
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

/// How settings sync stands, beside the account; nothing while signed out or choosing.
fn sync_status(s: &App) -> Option<String> {
    use crate::settings_sync::{Status, ago};
    let signed_in = s.monster.online() && s.monster.session.is_some();
    Some(match s.settings_sync.status(signed_in) {
        Status::SignedOut | Status::Choose => return None,
        Status::Syncing => "Syncing settings…".into(),
        Status::UpToDate(Some(at)) => format!("Settings synced {}", ago(at.elapsed())),
        Status::UpToDate(None) => "Settings synced".into(),
        Status::Offline => "Settings sync can't reach the server".into(),
    })
}

/// Settings sync is on whenever you're signed in. The first time this computer and the account
/// have different settings, nothing changes until you pick which to keep, here under the tabs.
fn sync_choice(s: &App, t: &Theme) -> Option<El> {
    use crate::settings_sync::{Choice, Status};
    let signed_in = s.monster.online() && s.monster.session.is_some();
    if s.settings_sync.status(signed_in) != Status::Choose {
        return None;
    }
    Some(
        well(t)
            .key("settings-sync-choice")
            .padding(size::GAP * 2.0)
            .gap(size::GAP)
            .child(
                text("Your account has other settings")
                    .font(ty::HEADING)
                    .color(t.text),
            )
            .child(muted(
                t,
                "Your servers, nickname and preferences on this account differ from this \
                 computer's. Nothing changes until you choose.",
            ))
            .child(
                row()
                    .gap(size::GAP)
                    .flex_wrap()
                    .child(button(
                        t,
                        "settings-sync-account",
                        "Use My Account's Settings",
                        None,
                        Variant::Primary,
                        Some(Message::SettingsSyncChoice(Choice::Account)),
                    ))
                    .child(button(
                        t,
                        "settings-sync-computer",
                        "Keep This Computer's",
                        None,
                        Variant::Secondary,
                        Some(Message::SettingsSyncChoice(Choice::ThisComputer)),
                    )),
            ),
    )
}

fn overview(
    s: &App,
    cx: &mut Cx<App>,
    t: &Theme,
    k: &Kit,
    today: NaiveDate,
    reduced: bool,
) -> Vec<El> {
    let board = Board::new(&s.monster.cans, today, |at| local_date(at).unwrap_or(today));
    vec![
        row()
            .w_full()
            .gap(20.0)
            .flex_wrap()
            .child(showcase(s, reduced))
            .child(add_panel(s, t, k)),
        profile(
            s,
            t,
            "Your Board",
            &board,
            today,
            Some(caffeine(s, cx, t, k)),
        ),
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
    // Until a search or Show All, only the common flavours follow the favourites.
    let all = searching || s.ui.monster_all_flavours;
    let offered: Vec<(usize, u32)> = rest
        .iter()
        .copied()
        .filter(|(index, _)| all || monster::common(*index))
        .collect();
    let folded = rest.len() - offered.len();
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
                &format!("Search all {} flavours", FLAVOURS.len()),
                Some(Message::MonsterSearch),
                // Enter takes the first match, so a search and Enter is the quickest way in.
                order
                    .first()
                    .filter(|_| searching && open)
                    .map(|(index, _)| Message::MonsterConfirm(Some(*index))),
            )),
        );
    if !favourites.is_empty() {
        panel = panel
            .child(group_title(t, "Your Favourites").mt(4.0))
            .child(flavour_buttons(t, favourites, open));
    }
    if offered.is_empty() {
        if searching {
            panel = panel.child(muted(t, "No flavour by that name. Try “Other”.").mt(4.0));
        }
    } else {
        let label = if searching {
            "Matches"
        } else if !all {
            "Popular Flavours"
        } else if favourites.is_empty() {
            "Flavours"
        } else {
            "Other Flavours"
        };
        panel = panel
            .child(group_title(t, label).mt(4.0))
            .child(flavour_buttons(t, &offered, open));
    }
    if !searching {
        let (label, glyph) = if all {
            ("Show Fewer".to_owned(), &icons::CHEVRON_UP)
        } else {
            (format!("Show {folded} More"), &icons::CHEVRON_DOWN)
        };
        panel = panel.child(row().child(button(
            t,
            "monster-all-flavours",
            label,
            Some(glyph),
            Variant::Secondary,
            Some(Message::MonsterAllFlavours(!all)),
        )));
    }
    panel = panel.child(notice(s, t));
    if let Some(error) = &tracker.load_error {
        panel = panel.child(error_text(
            t,
            format!("{error}. Fix or remove the file to add cans."),
        ));
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
        let aka = flavour.also_known_as();
        let label = match &aka {
            Some(aka) => format!("Add a can of {}. {aka}", flavour.name),
            None => format!("Add a can of {}", flavour.name),
        };
        let mut button = row()
            .key(format!("monster-flavour-{}", flavour.id))
            .role("button")
            .label(label)
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
        if let Some(aka) = aka {
            button = button.child(aliases_info(t, flavour, aka));
        }
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
                .on_press_tx(send(Message::MonsterConfirm(Some(index))))
        } else {
            button.disabled(true).dim(0.5)
        };
        wrap = wrap.child(button);
    }
    wrap
}

/// A small "i" whose tooltip names the other names a flavour is sold under.
fn aliases_info(t: &Theme, flavour: &Flavour, aka: String) -> El {
    el().key(format!("monster-aka-{}", flavour.id))
        .size(14.0, 16.0)
        .center()
        .help(aka)
        .child(icon(&icons::INFO, 12.0).color(t.faint).a11y_hidden())
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
        Notice::Failed(error) => error_text(t, error.clone()).key("monster-notice"),
    }
}

/// Facts about the card on show, and the way to take a mistyped can back out.
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

/// `now` is the caffeine chart, on your own board only: it says how you are, not what you own.
fn profile(
    s: &App,
    t: &Theme,
    heading: &str,
    board: &Board,
    today: NaiveDate,
    now: Option<El>,
) -> El {
    col()
        .key("monster-board")
        .w_full()
        .gap(14.0)
        .child(title(t, heading))
        .child(stats(s, t, board))
        .children(now)
        .child(heatmap(s, t, board, today))
        .child(flavours(t, board))
}

/// The caffeine chart's canvas, whose width layout decides.
const CAFFEINE_CHART: &str = "monster-caffeine-chart";
const CAFFEINE_HEIGHT: f32 = 96.0;
/// How far the chart looks back and ahead, in seconds, and how far apart its points are.
const CAFFEINE_BEFORE: i64 = 6 * 3600;
const CAFFEINE_AFTER: i64 = 18 * 3600;
const CAFFEINE_STEP: i64 = 300;
/// How far an arrow key moves the reading along the chart.
const CAFFEINE_KEY_STEP: i64 = 15 * 60;
/// The reading that follows the pointer: its width, and its height of two lines.
const CAFFEINE_TIP: (f32, f32) = (136.0, 38.0);

/// A moment on the chart as the clock shows it, with the day when it isn't today's.
fn clock(at: i64, now: i64) -> String {
    let (Some(time), Some(today)) = (
        Local.timestamp_opt(at, 0).earliest(),
        Local.timestamp_opt(now, 0).earliest(),
    ) else {
        return String::new();
    };
    let hour = time.format("%H:%M");
    match (time.date_naive() - today.date_naive()).num_days() {
        0 => hour.to_string(),
        1 => format!("{hour} tomorrow"),
        -1 => format!("{hour} yesterday"),
        _ => time.format("%H:%M on %A").to_string(),
    }
}

/// How far a moment is from now, in words: "in 3 h 5 min", "40 min ago", "now".
fn from_now(at: i64, now: i64) -> String {
    let minutes = (at - now).abs() / 60;
    let span = match (minutes / 60, minutes % 60) {
        (0, 0) => return "now".to_owned(),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    };
    if at > now {
        format!("in {span}")
    } else {
        format!("{span} ago")
    }
}

/// Caffeine in your body from six hours ago to eighteen ahead, estimated from when each can
/// was added: the past as a line over a fill, what's still to come fainter. The pointer, or
/// the arrow keys, read off the level at any moment along it. It follows the clock a minute
/// at a time and never animates.
fn caffeine(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit) -> El {
    let now = chrono::Utc::now().timestamp() / 60 * 60;
    let caffeine = s.own_caffeine(now);
    let (start, end) = (now - CAFFEINE_BEFORE, now + CAFFEINE_AFTER);
    let samples: Vec<(i64, f32)> = (start..=end)
        .step_by(CAFFEINE_STEP as usize)
        .map(|at| (at, caffeine.mg(at)))
        .collect();
    let (peak_at, peak) =
        samples.iter().copied().fold(
            (now, 0.0),
            |best, sample| {
                if sample.1 > best.1 { sample } else { best }
            },
        );
    let about = format!(
        "An estimate: {} mg a can, drunk over {} minutes and taken up within the hour, then \
         halving every {}. Sleep-ready is under {} mg. People vary.",
        monster::CAFFEINE_MG_PER_CAN,
        monster::DRINKING_MINUTES,
        match s.caffeine_weight {
            Some(_) => format!("{:.1} hours at your weight", caffeine.half_life_hours()),
            None => format!("{} hours", monster::CAFFEINE_HALF_LIFE_HOURS),
        },
        monster::CAFFEINE_SLEEP_MG
    );
    let head = row()
        .gap(6.0)
        .center_y()
        .grow(1.0)
        .child(heading(t, "Caffeine in Your System"))
        .child(
            el().key("monster-caffeine-about")
                .size(14.0, 16.0)
                .center()
                .help(about)
                .child(icon(&icons::INFO, 12.0).color(t.faint).a11y_hidden()),
        );
    let frame = well(t).key("monster-caffeine").w_full();
    // The switches stay folded behind this until asked for: the chart is what the well is for.
    let open = s.ui.caffeine_options;
    let gear = icon_button(
        t,
        "monster-caffeine-options-toggle",
        &icons::SETTINGS,
        if open {
            "Hide Caffeine Options"
        } else {
            "Caffeine Options"
        },
        if open {
            Variant::Active
        } else {
            Variant::Ghost
        },
        Some(Message::CaffeineOptionsOpen(!open)),
    )
    .expanded(open)
    .mt(-6.0)
    .mb(-6.0)
    .mr(-6.0);
    let options = open.then(|| caffeine_options(s, t, k));
    if peak < monster::CAFFEINE_TRACE_MG {
        return frame
            .label("Caffeine in your system: none from your cans right now")
            .child(
                row()
                    .w_full()
                    .px(14.0)
                    .py(10.0)
                    .gap(size::GAP)
                    .center_y()
                    .child(head)
                    .child(faint(t, "None from your cans right now"))
                    .child(gear),
            )
            .children(options);
    }
    let level = caffeine.mg(now);
    let low = s.caffeine.low();
    let low_at = caffeine.falls_below(low as f32, now);
    // When what's left stops getting in the way of sleep, as far as the cans so far go.
    let sleep_at = caffeine.falls_below(monster::CAFFEINE_SLEEP_MG as f32, now);
    // A round top, so the scale reads at a glance: 200 mg holds one can, then it grows by hundreds.
    let top = ((peak * 1.1 / 100.0).ceil() * 100.0).max(200.0);
    let w = cx
        .layout_size(CAFFEINE_CHART)
        .map_or(size::CHART_MIN_WIDTH, |[w, _]| w);
    let h = CAFFEINE_HEIGHT;
    let span = (w - 2.0).max(1.0);
    let x = move |at: i64| 1.0 + span * (at - start) as f32 / (end - start) as f32;
    let y = move |mg: f32| 2.0 + (h - 4.0) * (1.0 - (mg / top).clamp(0.0, 1.0));
    let (ink, grid, marker) = (t.accent, t.wash(0.05), t.wash(0.16));
    // The level the notice goes by is drawn only while the notice is on.
    let line = s.caffeine.low_notice.then_some(low);
    let dashes = t.wash(0.26);
    let curve = rune::memo(
        "chart-monster-caffeine",
        (
            now,
            &caffeine,
            w.to_bits(),
            line,
            color_key(ink),
            color_key(grid),
            color_key(marker),
            color_key(dashes),
        ),
        || {
            let mut shape = Shape::new();
            for mg in [0.0, top / 2.0, top] {
                shape = shape.line([0.0, y(mg)], [w, y(mg)], 1.0, LineCap::Butt, grid);
            }
            if let Some(low) = line {
                let at = y(low as f32);
                let mut from = 0.0;
                while from < w {
                    let to = (from + 4.0).min(w);
                    shape = shape.line([from, at], [to, at], 1.0, LineCap::Butt, dashes);
                    from += 8.0;
                }
            }
            shape = shape.line([x(now), 0.0], [x(now), h], 1.0, LineCap::Butt, marker);
            let point = |(at, mg): &(i64, f32)| [x(*at), y(*mg)];
            let past: Vec<[f32; 2]> = samples
                .iter()
                .filter(|(at, _)| *at <= now)
                .map(point)
                .collect();
            let ahead: Vec<[f32; 2]> = samples
                .iter()
                .filter(|(at, _)| *at >= now)
                .map(point)
                .collect();
            let mut area = vec![[x(start), y(0.0)]];
            area.extend_from_slice(&past);
            area.push([x(now), y(0.0)]);
            shape = shape
                .fill(area, ink.alpha(0.12))
                .stroke(past, 1.6, ink)
                .stroke(ahead, 1.6, ink.alpha(0.45));
            // A tick on the baseline where each can was opened, which is where a rise starts.
            for opened in caffeine.opened().iter().filter(|at| **at >= start) {
                let at = x(*opened);
                shape = shape.line([at, h - 1.0], [at, h - 7.0], 2.0, LineCap::Round, ink);
            }
            let here = [x(now), y(level)];
            canvas(shape.line(here, here, 6.0, LineCap::Round, ink))
        },
    )
    .absolute(0.0, 0.0)
    .w(w)
    .h(h)
    .a11y_hidden();

    // The moment read off the chart, while it is still on it: the clock moves the chart on.
    let read = s.ui.caffeine_hover.filter(|at| (start..=end).contains(at));
    let to_time = move |at_x: f32| {
        let share = ((at_x - 1.0) / span).clamp(0.0, 1.0);
        let at = start + (share * (end - start) as f32) as i64;
        // Close to the marker, it is now that is meant.
        if (at_x - x(now)).abs() <= 4.0 {
            now
        } else {
            (at + 30) / 60 * 60
        }
    };
    let mut chart = el()
        .key(CAFFEINE_CHART)
        .grow(1.0)
        .min_w(0.0)
        .h(h)
        .role("slider")
        .label("Caffeine at a moment")
        .a11y_value(match read {
            Some(at) => format!(
                "About {:.0} milligrams at {}, {}",
                caffeine.mg(at),
                clock(at, now),
                from_now(at, now)
            ),
            None => format!("About {level:.0} milligrams now"),
        })
        .focusable(true)
        .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
        .cursor(rune::Cursor::Crosshair)
        .on_pointer_move_local(move |s: &mut App, p: rune::PointerEvent, tx: &mut Tx| {
            let at = Some(to_time(p.x));
            if s.ui.caffeine_hover == at {
                tx.live_only();
            }
            s.ui.caffeine_hover = at;
        })
        .on_hover_change(|s: &mut App, inside, _| {
            if !inside {
                s.ui.caffeine_hover = None;
            }
        })
        .on_key(move |s: &mut App, key: &rune::KeyEvent, _: &mut Tx| {
            let from = s.ui.caffeine_hover.unwrap_or(now);
            s.ui.caffeine_hover = match key.key {
                rune::Key::Left => Some((from - CAFFEINE_KEY_STEP).max(start)),
                rune::Key::Right => Some((from + CAFFEINE_KEY_STEP).min(end)),
                rune::Key::Home => Some(now),
                rune::Key::Escape if s.ui.caffeine_hover.is_some() => None,
                _ => return false,
            };
            true
        })
        .child(curve);
    if let Some(at) = read {
        let mg = caffeine.mg(at);
        let (at_x, at_y) = (x(at), y(mg));
        let (tip_w, tip_h) = CAFFEINE_TIP;
        // Beside the line, on whichever side has room, and clear of the point it reads.
        let tip_x = if at_x + 8.0 + tip_w <= w {
            at_x + 8.0
        } else {
            (at_x - 8.0 - tip_w).max(0.0)
        };
        let tip_y = if at_y < h / 2.0 { h - tip_h - 2.0 } else { 2.0 };
        chart = chart
            .child(
                el().absolute(at_x - 0.5, 0.0)
                    .w(1.0)
                    .h(h)
                    .fill(t.wash(0.38))
                    .inert(),
            )
            .child(
                el().absolute(at_x - 4.5, at_y - 4.5)
                    .size(9.0, 9.0)
                    .radius(4.5)
                    .fill(ink)
                    .border(2.0, t.recess)
                    .inert(),
            )
            .child(
                col()
                    .key("monster-caffeine-reading")
                    .absolute(tip_x, tip_y)
                    .w(tip_w)
                    .h(tip_h)
                    .px(8.0)
                    .center_y()
                    .gap(1.0)
                    .radius(radius::CONTROL)
                    .fill(t.raised)
                    .border_gradient(1.0, t.bevel_top, t.bevel_bottom)
                    .box_shadow(shadow(0.0, 4.0, 12.0, Color::BLACK.alpha(0.3)))
                    .child(
                        row()
                            .gap(4.0)
                            .center_y()
                            .child(
                                text(format!("≈ {}", monster::caffeine_text(mg)))
                                    .font(ty::NUMERIC)
                                    .color(t.text),
                            )
                            .child(
                                text(clock(at, now).split(' ').next().unwrap_or("").to_owned())
                                    .font(ty::NUMERIC)
                                    .color(t.muted),
                            ),
                    )
                    .child(text(from_now(at, now)).font(ty::CAPTION).color(t.faint))
                    .a11y_hidden()
                    .inert(),
            );
    }

    let small = TextStyle::new(10.0, 400.0).features("'tnum' 1");
    let mut scale = el().size(WEEKDAY_COLUMN, h).a11y_hidden();
    let marks = [(top, 0.0), (top / 2.0, h / 2.0 - 6.0), (0.0, h - 12.0)];
    for (mg, at) in marks {
        scale = scale.child(
            text(format!("{mg:.0}"))
                .font(small)
                .color(t.faint)
                .absolute(0.0, at),
        );
    }
    // The notice's level is named on the scale too, where the round marks leave it room.
    if let Some(low) = line {
        let at = y(low as f32) - 6.0;
        if marks.iter().all(|(_, mark)| (mark - at).abs() >= 11.0) {
            scale = scale.child(
                text(low.to_string())
                    .font(small)
                    .color(t.muted)
                    .absolute(0.0, at),
            );
        }
    }
    // Clock hours under the chart, every three, or every six when there's little room.
    let every = if w < 420.0 { 6 } else { 3 };
    let mut times = el().grow(1.0).min_w(0.0).h(14.0).a11y_hidden();
    let label = |at: i64, name: String, color: Color| {
        el().absolute(x(at) - 24.0, 0.0)
            .w(48.0)
            .center()
            .child(text(name).font(small).color(color))
    };
    // From the first whole hour on the local clock, which a half-hour time zone puts off UTC's.
    let into_hour = Local
        .timestamp_opt(start, 0)
        .earliest()
        .map_or(0, |time| i64::from(time.minute() * 60 + time.second()));
    // A moon on the hours where sleep comes easily again, when that is on the chart and
    // clear of Now.
    let moon =
        sleep_at.filter(|at| *at < end && (x(*at) - x(now)).abs() >= 30.0 && x(*at) <= w - 8.0);
    let mut hour = start + (3600 - into_hour) % 3600;
    while hour < end {
        let clear = (x(hour) - x(now)).abs() >= 34.0
            && x(hour) >= 20.0
            && x(hour) <= w - 20.0
            && moon.is_none_or(|at| (x(hour) - x(at)).abs() >= 26.0);
        if let Some(time) = Local.timestamp_opt(hour, 0).earliest()
            && clear
            && time.hour() % every == 0
            && time.minute() == 0
        {
            times = times.child(label(hour, time.format("%H:%M").to_string(), t.faint));
        }
        hour += 3600;
    }
    times = times.child(label(now, "Now".to_owned(), t.muted));
    if let Some(at) = moon {
        times = times.child(
            el().key("monster-caffeine-sleep")
                .absolute(x(at) - 7.0, 0.0)
                .size(14.0, 14.0)
                .center()
                .help(format!(
                    "Sleep-ready {}: under {} mg",
                    from_now(at, now),
                    monster::CAFFEINE_SLEEP_MG
                ))
                .child(icon(&icons::MOON, 11.0).color(t.muted)),
        );
    }

    // What the curve comes to, in words: its top, and when it runs low.
    let mut facts = vec![format!(
        "{} ≈ {} at {}",
        if peak_at > now { "Peaks" } else { "Peaked" },
        monster::caffeine_text(peak),
        clock(peak_at, now)
    )];
    if let Some(at) = low_at.filter(|at| Some(*at) != sleep_at) {
        facts.push(format!("Under {low} mg at about {}", clock(at, now)));
    }
    facts.push(match sleep_at {
        Some(at) => format!(
            "Sleep-ready {}, around {}",
            from_now(at, now),
            clock(at, now)
        ),
        None => "Sleep-ready now".to_owned(),
    });
    let facts = facts.join("  ·  ");

    frame
        .label(format!(
            "Caffeine in your system: about {level:.0} milligrams now. {facts}"
        ))
        .child(
            col()
                .w_full()
                .padding(14.0)
                .gap(6.0)
                .child(
                    row()
                        .w_full()
                        .gap(size::GAP)
                        .center_y()
                        .child(head)
                        .child(
                            row()
                                .key("monster-caffeine-now")
                                .gap(4.0)
                                .center_y()
                                .child(
                                    text(format!("≈ {}", monster::caffeine_text(level)))
                                        .font(ty::NUMERIC)
                                        .color(t.text),
                                )
                                .child(faint(t, "now")),
                        )
                        .child(gear),
                )
                .child(row().w_full().child(scale).child(chart))
                .child(
                    row()
                        .w_full()
                        .child(el().w(WEEKDAY_COLUMN).h(14.0))
                        .child(times),
                )
                .child(
                    faint(t, facts)
                        .key("monster-caffeine-facts")
                        .ml(WEEKDAY_COLUMN)
                        .a11y_hidden(),
                ),
        )
        .children(options)
}

/// The rows the well's gear opens under the chart: your weight, and what the estimate does outside
/// this page. They live here, not in Settings, which doesn't know the page.
fn caffeine_options(s: &App, t: &Theme, k: &Kit) -> El {
    let options = s.caffeine;
    let signed_in = s.monster.online() && s.monster.session.is_some();
    let beside = if signed_in {
        "Yours, and anyone's who isn't hidden from the leaderboard."
    } else if s.monster.online() {
        "Yours. Other people's show once you're signed in."
    } else {
        "Yours, from the cans you add here."
    };
    let switch = |key: &'static str, label: &str, on: bool, next: settings::Caffeine| {
        Switch::new(on)
            .label(label)
            .hide_label()
            .on_toggle_tx(move |s: &mut App, tx| {
                super::dispatch(s, tx, Message::CaffeineOptions(next));
            })
            .build(k)
            .key(key)
    };
    let low = options.low();
    let mut warn = row().gap(12.0).center_y();
    if options.low_notice {
        warn = warn.child(
            slider(
                t,
                "monster-caffeine-low",
                "Low caffeine level",
                low as i32,
                format!("{low} milligrams"),
                Range {
                    min: *monster::CAFFEINE_LOW_RANGE.start() as i32,
                    max: *monster::CAFFEINE_LOW_RANGE.end() as i32,
                    step: monster::CAFFEINE_LOW_STEP as i32,
                    key_step: monster::CAFFEINE_LOW_STEP as i32,
                },
                |s| s.caffeine.low() as i32,
                move |value| {
                    Message::CaffeineOptions(settings::Caffeine {
                        low_mg: value.max(0) as u32,
                        ..options
                    })
                },
            )
            .w(160.0),
        );
    }
    warn = warn.child(switch(
        "monster-caffeine-warn",
        "Warn when caffeine runs low",
        options.low_notice,
        settings::Caffeine {
            low_notice: !options.low_notice,
            ..options
        },
    ));
    // Your weight, for a half-life nearer your own. Off, the estimate is for 70 kg.
    let weight = s.caffeine_weight;
    let mut weigh = row().gap(12.0).center_y();
    if let Some(kg) = weight {
        weigh = weigh.child(
            slider(
                t,
                "monster-caffeine-weight",
                "Your weight",
                kg as i32,
                format!("{kg} kilograms"),
                Range {
                    min: *monster::CAFFEINE_WEIGHT_RANGE.start() as i32,
                    max: *monster::CAFFEINE_WEIGHT_RANGE.end() as i32,
                    step: 1,
                    key_step: 1,
                },
                |s| s.caffeine_weight.unwrap_or(monster::CAFFEINE_WEIGHT_KG) as i32,
                |value| Message::CaffeineWeight(Some(value.max(0) as u32)),
            )
            .w(160.0),
        );
    }
    weigh = weigh.child(
        Switch::new(weight.is_some())
            .label("Use my weight")
            .hide_label()
            .on_toggle_tx(move |s: &mut App, tx| {
                let next = match weight {
                    Some(_) => None,
                    None => Some(monster::CAFFEINE_WEIGHT_KG),
                };
                super::dispatch(s, tx, Message::CaffeineWeight(next));
            })
            .build(k)
            .key("monster-caffeine-weigh"),
    );
    let weighing = match weight {
        Some(kg) => format!(
            "{kg} kg: half is gone every {:.1} hours. Kept on this computer.",
            monster::caffeine_half_life_hours(weight)
        ),
        None => "A heavier body holds caffeine a little longer. Kept on this computer.".to_owned(),
    };
    col()
        .key("monster-caffeine-options")
        .w_full()
        .child(divider(t))
        .child(form_row(
            t,
            "Show caffeine beside names",
            Some(beside),
            switch(
                "monster-caffeine-names",
                "Show caffeine beside names",
                options.beside_names,
                settings::Caffeine {
                    beside_names: !options.beside_names,
                    ..options
                },
            ),
        ))
        .child(el().h(1.0).w_full().px(size::CARD_PAD).child(divider(t)))
        .child(form_row(
            t,
            "Warn when caffeine runs low",
            Some(&format!("A notice once you fall under {low} mg.")),
            warn,
        ))
        .child(el().h(1.0).w_full().px(size::CARD_PAD).child(divider(t)))
        .child(form_row(t, "Use my weight", Some(&weighing), weigh))
}

fn days(n: u32) -> String {
    if n == 1 {
        "1 day".into()
    } else {
        format!("{n} days")
    }
}

/// A whole number with its thousands parted, as a total reads: 12,480.
fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (place, digit) in digits.chars().enumerate() {
        if place > 0 && (digits.len() - place).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn stat(t: &Theme, label: &str, value: String) -> El {
    col()
        .gap(2.0)
        .child(text(value).font(ty::BIG_VALUE).color(t.text))
        .child(muted(t, label))
}

/// The board's ten figures in even columns: two rows of five (the habit over what it added
/// up to), or pairs in a narrow window. Every column is as wide as the next, so a wrapped
/// row lines up under the one above.
fn stats(s: &App, t: &Theme, board: &Board) -> El {
    const GAP: f32 = 16.0;
    let room = (s.window_size.0.min(1100.0) - size::PAGE * 2.0).max(0.0);
    // A column holds the widest figure a board comes to, such as "≈ 163,550".
    let columns = if room >= 5.0 * 124.0 + 4.0 * GAP {
        5.0
    } else {
        2.0
    };
    // What is counted is exact; what is worked out from labels is about, unless it is nothing.
    let about = |n: u32| match n {
        0 => "0".to_owned(),
        n => format!("≈ {}", thousands(n)),
    };
    let w = ((room - (columns - 1.0) * GAP) / columns).floor().max(1.0);
    let stat = |label: &str, value: String| stat(t, label, value).w(w);
    row()
        .key("monster-stats")
        .w_full()
        .gap(GAP)
        .flex_wrap()
        .child(stat("Cans", board.total.to_string()))
        .child(stat(
            "Flavours",
            format!("{} / {}", board.flavours, FLAVOURS.len()),
        ))
        .child(stat("Current Streak", days(board.current_streak)))
        .child(stat("Longest Streak", days(board.longest_streak)))
        .child(stat("Longest Break", days(board.longest_break)))
        .child(stat("Best Day", board.best_day.to_string()))
        .child(stat("Drunk", format!("{:.1} L", board.litres)))
        .child(stat("Caffeine", format!("{:.1} g", board.caffeine_g)))
        .child(
            stat("Sugar Cubes", about(board.sugar_g / monster::SUGAR_CUBE_G))
                .key("monster-stat-sugar")
                .help(format!(
                    "About {:.1} kg of sugar, at {} g a cube. What is typical for each flavour's \
                 line; recipes vary by country.",
                    board.sugar_g as f32 / 1000.0,
                    monster::SUGAR_CUBE_G
                )),
        )
        .child(stat("Kilocalories", about(board.kcal)))
}

fn format_date(date: NaiveDate) -> String {
    date.format("%b %-d, %Y").to_string()
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

    let facts = col()
        .key("flavour-facts")
        .grow(1.0)
        .min_w(320.0)
        .gap(14.0)
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(muted(t, flavour.line))
                .children(
                    flavour
                        .also_known_as()
                        .map(|aka| aliases_info(t, flavour, aka)),
                ),
        )
        .child(flavour_stats(t, s, &cans, &board));

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

/// The wash behind a row in the channel tree lit by the card its person just opened: the
/// rarity's colour sweeping in from the left, pulsing for the rare ones, fading out at the end.
pub fn unboxed_glow(t: &Theme, member: u16, unboxed: &monster::Unboxed, level: f32) -> El {
    let ink = rgb(unboxed.rarity.color());
    el().key(format!("unboxed-glow-{member}"))
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .paint_order(-1)
        .radius(radius::ROW)
        .linear_gradient(
            90.0,
            [
                (0.0, ink.alpha(if t.dark { 0.34 } else { 0.24 })),
                (0.65, ink.alpha(0.07)),
                (1.0, ink.alpha(0.0)),
            ],
        )
        .opacity(level)
        .opacity_motion(motion::SETTLE)
        .transition(Transition::opacity().combined(Transition::offset(-40.0, 0.0)))
        .motion(motion::SETTLE)
        .a11y_hidden()
}

/// The card's can at the end of the row, in the rarity's colour; it names the card on hover.
pub fn unboxed_chip(_t: &Theme, unboxed: &monster::Unboxed) -> El {
    let help = format!(
        "Opened a {} card · {} #{}",
        unboxed.rarity.name(),
        unboxed.flavour,
        unboxed.number
    );
    el().key("unboxed-chip")
        .help(help.clone())
        .label(help)
        .child(icon(&icons::CAN, 13.0).color(rgb(unboxed.rarity.color())))
}

/// An opened card as a chat line shows it, in place of the words it was sent as.
pub fn unboxed_card(t: &Theme, unboxed: &monster::Unboxed) -> El {
    let ink = rgb(unboxed.rarity.color());
    row()
        .w_full()
        .mt(3.0)
        .gap(10.0)
        .pl(10.0)
        .pr(12.0)
        .py(8.0)
        .center_y()
        .radius(8.0)
        .linear_gradient(
            90.0,
            [
                (0.0, ink.alpha(if t.dark { 0.20 } else { 0.14 })),
                (1.0, ink.alpha(0.04)),
            ],
        )
        .border(1.0, ink.alpha(0.35))
        .child(
            el().size(30.0, 30.0)
                .center()
                .radius(7.0)
                .fill(ink.alpha(0.22))
                .child(icon(&icons::CAN, 18.0).color(ink).a11y_hidden()),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(
                    text(format!("Opened a {} card", unboxed.rarity.name()))
                        .font(ty::SMALL_STRONG)
                        .color(ink),
                )
                .child(
                    text(format!("{} · #{}", unboxed.flavour, unboxed.number))
                        .font(ty::BODY)
                        .color(t.text)
                        .truncate_fade(),
                ),
        )
}
