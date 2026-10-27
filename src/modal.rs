//! Iced overlays with a non-interactive base; focus and input stay out of covered fields.
use iced::{Element, Length, Renderer, Theme};
use iced_runtime::core::{
    self, Event, Layout, Rectangle, Shell, Size, Widget, layout, mouse, renderer,
    widget::{Operation, Tree, tree},
};

pub fn shield<'a, Message: 'a>(
    content: Element<'a, Message>,
    blocked: bool,
) -> Element<'a, Message> {
    Element::new(Shield { content, blocked })
}
struct Shield<'a, Message> {
    content: Element<'a, Message>,
    blocked: bool,
}
impl<Message> Widget<Message, Theme, Renderer> for Shield<'_, Message> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }
    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }
    fn children(&self) -> Vec<Tree> {
        self.content.as_widget().children()
    }
    fn diff(&self, tree: &mut Tree) {
        self.content.as_widget().diff(tree);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            tree,
            renderer,
            theme,
            style,
            layout,
            if self.blocked {
                mouse::Cursor::Unavailable
            } else {
                cursor
            },
            viewport,
        );
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if !self.blocked {
            self.content
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn core::Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.blocked && blocked_input(event) {
            shell.capture_event();
            return;
        }
        self.content.as_widget_mut().update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.blocked {
            mouse::Interaction::None
        } else {
            self.content
                .as_widget()
                .mouse_interaction(tree, layout, cursor, viewport, renderer)
        }
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: core::Vector,
    ) -> Option<core::overlay::Element<'a, Message, Theme, Renderer>> {
        if self.blocked {
            None
        } else {
            self.content
                .as_widget_mut()
                .overlay(tree, layout, renderer, viewport, translation)
        }
    }
}
fn blocked_input(event: &Event) -> bool {
    matches!(
        event,
        Event::Keyboard(_) | Event::Mouse(_) | Event::Touch(_) | Event::InputMethod(_)
    )
}

pub fn escape_released(event: &Event) -> bool {
    matches!(event, Event::Keyboard(core::keyboard::Event::KeyReleased { key, physical_key, .. })
        if *key == core::keyboard::Key::Named(core::keyboard::key::Named::Escape)
            || *physical_key == core::keyboard::key::Physical::Code(core::keyboard::key::Code::Escape))
}

pub fn dismiss_released(event: &Event) -> bool {
    escape_released(event)
        || matches!(event, Event::Keyboard(core::keyboard::Event::KeyReleased { key, .. })
        if *key == core::keyboard::Key::Named(core::keyboard::key::Named::Enter))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_backdrop_blocks_scroll_and_typing_without_stopping_redraws() {
        assert!(blocked_input(&Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }
        })));
        assert!(blocked_input(&Event::Keyboard(
            core::keyboard::Event::ModifiersChanged(core::keyboard::Modifiers::SHIFT)
        )));
        assert!(!blocked_input(&Event::Window(
            core::window::Event::RedrawRequested(std::time::Instant::now())
        )));
    }

    #[test]
    fn modal_keyboard_closes_once_on_release_and_accepts_physical_escape() {
        use iced_runtime::core::keyboard::{
            Event as KeyEvent, Key, Location, Modifiers,
            key::{Code, Named, Physical},
        };
        let release = |key, code| {
            Event::Keyboard(KeyEvent::KeyReleased {
                key,
                modified_key: Key::Unidentified,
                physical_key: Physical::Code(code),
                location: Location::Standard,
                modifiers: Modifiers::default(),
            })
        };
        assert!(dismiss_released(&release(
            Key::Named(Named::Escape),
            Code::Escape
        )));
        assert!(dismiss_released(&release(Key::Unidentified, Code::Escape)));
        assert!(dismiss_released(&release(
            Key::Named(Named::Enter),
            Code::Enter
        )));
        assert!(!dismiss_released(&release(
            Key::Named(Named::Tab),
            Code::Tab
        )));
        assert!(!escape_released(&release(
            Key::Named(Named::Enter),
            Code::Enter
        )));
    }
}
