use iced::{
    Border, Color, Element, Length, Theme, color,
    theme::{Palette, palette},
    widget::{
        button, checkbox, container, overlay::menu, pick_list, progress_bar, scrollable, slider,
        text, text_input,
    },
};

// Surface levels, from the window canvas to controls and floating details.
pub const CANVAS: Color = color!(0x10141c);
pub const SIDEBAR: Color = color!(0x171e29);
pub const SURFACE: Color = color!(0x1e2836);
pub const RAISED: Color = color!(0x29364a);
pub const OUTLINE: Color = color!(0x334258);
pub const TEXT: Color = color!(0xe6edf7);
pub const MUTED: Color = color!(0x9baac1);
pub const ACCENT: Color = color!(0x85a7ff);
pub const CONTROL_HEIGHT: u32 = 36;
pub const COMPACT_HEIGHT: u32 = 28;
pub const CONTROL_TEXT: u32 = 14;
pub const COMPACT_TEXT: u32 = 12;
pub const CONTROL_PADDING: u16 = 12;
pub const COMPACT_PADDING: u16 = 8;
pub const CONTROL_GAP: u32 = 8;
pub const NAV_ICON_SIZE: u32 = 18;
pub const CONNECTION_CHART_MIN_WIDTH: f32 = 110.0;
pub const CONNECTION_CHART_GAP: u32 = 12;
pub const CHANNEL_ROW_HEIGHT: u32 = 32;

pub fn theme() -> Theme {
    Theme::custom_with_fn(
        "Flexspeak",
        Palette {
            background: CANVAS,
            text: TEXT,
            primary: ACCENT,
            success: color!(0x73d297),
            warning: color!(0xeabb63),
            danger: color!(0xf4778b),
        },
        |palette| {
            let mut extended = palette::Extended::generate(palette);
            extended.background.weak = palette::Pair::new(SURFACE, TEXT);
            extended.background.strong = palette::Pair::new(OUTLINE, TEXT);
            extended.secondary.base = palette::Pair::new(SURFACE, TEXT);
            extended.secondary.strong = palette::Pair::new(RAISED, TEXT);
            extended
        },
    )
}

pub fn chrome(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR.into()),
        ..Default::default()
    }
}

pub fn card(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SURFACE.into()),
        border: Border {
            color: OUTLINE,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub fn inset(_: &Theme) -> container::Style {
    container::Style {
        background: Some(SIDEBAR.into()),
        border: Border {
            radius: 0.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn floating(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(RAISED.into()),
        ..card(theme)
    }
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::secondary(theme, status);
    style.border = Border {
        color: OUTLINE,
        width: 1.0,
        radius: 0.0.into(),
    };
    style
}

pub fn nav(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::text(theme, status);
    style.text_color = MUTED;
    style.border.radius = 0.0.into();
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        style.background = Some(SURFACE.into());
        style.text_color = TEXT;
    }
    style
}

pub fn active_nav(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(RAISED.into()),
        text_color: ACCENT,
        ..nav(theme, status)
    }
}

pub fn channel_row(theme: &Theme, status: button::Status, current: bool) -> button::Style {
    let mut style = nav(theme, status);
    style.border = Border {
        width: 1.0,
        color: Color::TRANSPARENT,
        radius: 0.0.into(),
    };
    if current {
        style.background = Some(SURFACE.into());
        style.text_color = ACCENT;
        style.border.color = ACCENT.scale_alpha(0.4);
    }
    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        style.border.color = ACCENT;
        style.text_color = TEXT;
    }
    if status == button::Status::Disabled && !current {
        style.text_color = MUTED.scale_alpha(0.6);
    }
    style
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let mut style = text_input::default(theme, status);
    style.placeholder = MUTED;
    style.value = if matches!(status, text_input::Status::Disabled) {
        MUTED
    } else {
        TEXT
    };
    style.background = if matches!(status, text_input::Status::Disabled) {
        SIDEBAR
    } else {
        CANVAS
    }
    .into();
    style.border.radius = 0.0.into();
    style
}

pub fn primary_button<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> button::Button<'a, Message> {
    button(container(content).center_y(Length::Fill))
        .height(CONTROL_HEIGHT)
        .padding([0, CONTROL_PADDING])
        .style(primary)
}

pub fn label_button<'a, Message: 'a>(
    label: impl text::IntoFragment<'a>,
) -> button::Button<'a, Message> {
    primary_button(text(label).size(CONTROL_TEXT))
}

pub fn icon_button<'a, Message: 'a>(
    icon: crate::icons::Icon,
    label: impl text::IntoFragment<'a>,
) -> button::Button<'a, Message> {
    primary_button(crate::icons::label(icon, label, CONTROL_TEXT))
}

pub fn compact_button<'a, Message: 'a>(
    label: impl text::IntoFragment<'a>,
) -> button::Button<'a, Message> {
    primary_button(text(label).size(COMPACT_TEXT))
        .height(COMPACT_HEIGHT)
        .padding([0, COMPACT_PADDING])
}

pub fn icon_only_button<Message: 'static>(
    icon: crate::icons::Icon,
) -> button::Button<'static, Message> {
    primary_button(container(icon.view(NAV_ICON_SIZE)).center_x(Length::Fill))
        .width(CONTROL_HEIGHT)
        .padding(0)
        .style(nav)
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::primary(theme, status);
    style.border.radius = 0.0.into();
    style.border.width = 1.0;
    style.border.color = OUTLINE;
    style
}

pub fn danger(theme: &Theme, status: button::Status) -> button::Style {
    let mut style = button::danger(theme, status);
    style.border.radius = 0.0.into();
    style.border.width = 1.0;
    style.border.color = OUTLINE;
    style
}

pub fn check(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let mut style = checkbox::primary(theme, status);
    style.border.radius = 0.0.into();
    style
}

pub fn device_picker(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let mut style = pick_list::default(theme, status);
    style.border.radius = 0.0.into();
    style
}

pub fn device_menu(theme: &Theme) -> menu::Style {
    let mut style = menu::default(theme);
    style.border.radius = 0.0.into();
    style.background = RAISED.into();
    style
}

pub fn volume(theme: &Theme, status: slider::Status) -> slider::Style {
    let mut style = slider::default(theme, status);
    style.rail.border.radius = 0.0.into();
    style.handle.shape = slider::HandleShape::Rectangle {
        width: 10,
        border_radius: 0.0.into(),
    };
    style
}

pub fn meter(theme: &Theme) -> progress_bar::Style {
    let mut style = progress_bar::primary(theme);
    style.border.radius = 0.0.into();
    style
}

pub fn scroll_area<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
) -> scrollable::Scrollable<'a, Message> {
    scrollable(content).style(scrollbar)
}

fn scrollbar(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let mut style = scrollable::default(theme, status);
    style.container.border.radius = 0.0.into();
    style.auto_scroll.border.radius = 0.0.into();
    for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
        rail.border.radius = 0.0.into();
        rail.scroller.border.radius = 0.0.into();
    }
    style
}
