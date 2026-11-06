//! First-run setup: five steps in one centred card, with fixed Back / Continue controls.

use rune::{Cx, El, Transition, col, el, row, text};
use rune_kit::Kit;

use super::settings;
use super::theme::{Theme, motion, ty};
use super::widgets::{
    Variant, button, error_text, faint, field, muted, overline, scroll_page, title,
};
use crate::{App, Message, onboarding::Step};

fn stepper(t: &Theme, step: Step) -> El {
    let mut steps = row().w_full().gap(6.0);
    for item in Step::ALL {
        let done = item.index() < step.index();
        let current = item == step;
        let color = if current || done { t.accent } else { t.faint };
        steps = steps.child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(6.0)
                .label(format!("Step {} of 5: {}", item.index() + 1, item.title()))
                .child(
                    el().w_full()
                        .h(3.0)
                        .radius(1.5)
                        .fill(if current || done {
                            t.accent
                        } else {
                            t.wash(0.08)
                        })
                        .motion(motion::SETTLE)
                        .a11y_hidden(),
                )
                .child(
                    text(format!("{}. {}", item.index() + 1, item.title()))
                        .font(ty::CAPTION)
                        .color(color)
                        .truncate_fade()
                        .a11y_hidden(),
                ),
        );
    }
    steps
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>, step: Step) -> El {
    let busy = s.session.is_some() || s.permissions.pending.is_some();
    let fields = match step {
        Step::Username => col()
            .w_full()
            .gap(14.0)
            .child(super::widgets::body(t, "What should other people call you?"))
            .child(field(
                k,
                &s.ui.nickname,
                |s| &mut s.ui.nickname,
                "Username",
                Some(Message::Nickname),
                Some(Message::SetupNext),
            )),
        Step::Server => col()
            .w_full()
            .gap(14.0)
            .child(super::widgets::body(
                t,
                "Save the server you want to connect to. You can edit it later in Settings.",
            ))
            .child(field(
                k,
                &s.ui.address,
                |s| &mut s.ui.address,
                "voice.example.org:9987",
                Some(Message::Address),
                Some(Message::SetupNext),
            ))
            .child(faint(
                t,
                "Server password (optional · kept only for this session)",
            ))
            .child(field(
                k,
                &s.ui.password,
                |s| &mut s.ui.password,
                "Server password",
                Some(Message::Password),
                Some(Message::SetupNext),
            )),
        Step::Permissions => settings::permissions(s, t),
        Step::Devices => col()
            .w_full()
            .gap(16.0)
            .child(super::widgets::body(
                t,
                "Choose your microphone and playback device, then test them locally. No server connection is made.",
            ))
            .child(settings::audio(s, cx, t, k, layer))
            .child(muted(
                t,
                if s.loopback {
                    s.status.as_str()
                } else {
                    "Stop the local test before continuing. You can also test devices later in Settings."
                },
            )),
        Step::Hotkeys => settings::shortcut_settings(s, t, k),
    };
    let mut content = col()
        .w_full()
        .h_full()
        .gap(16.0)
        .child(overline(t, "Welcome to Flexspeak"))
        .child(super::widgets::display(t, "Let’s get you talking"))
        .child(stepper(t, step))
        .child(
            title(t, step.title())
                .key(format!("step-title-{}", step.index()))
                .transition(Transition::opacity().combined(Transition::offset(12.0, 0.0))),
        )
        .child(scroll_page(
            k,
            "setup-scroll",
            col()
                .key(format!("step-{}", step.index()))
                .w_full()
                .pr(12.0)
                .transition(Transition::opacity().combined(Transition::offset(16.0, 0.0)))
                .child(fields),
        ));
    if let Some(error) = &s.onboarding_error {
        content = content.child(
            error_text(t, error.clone())
                .key("setup-error")
                .transition(Transition::opacity()),
        );
    }
    let last = step == Step::Hotkeys;
    content = content.child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(button(
                t,
                "back",
                "Back",
                None,
                Variant::Secondary,
                (!busy && step.previous().is_some()).then_some(Message::SetupBack),
            ))
            .child(el().grow(1.0))
            .child(
                button(
                    t,
                    "continue",
                    if last { "Finish Setup" } else { "Continue" },
                    None,
                    Variant::Primary,
                    (!busy).then_some(Message::SetupNext),
                )
                .default_button(),
            ),
    );
    // A setup assistant: the steps sit on the window itself, not on a floating card.
    col()
        .w_full()
        .h_full()
        .padding(24.0)
        .pt(44.0)
        .center()
        .child(
            col()
                .key("setup")
                .w_full()
                .max_w(640.0)
                .h_full()
                .max_h(680.0)
                .child(content),
        )
}
