use pixie_ui::{
    event::{Key, PixieEvent},
    math::Pos,
};
use winit::keyboard::NamedKey;

/// Converts a winit window event into a pixie-ui event, or `None` if pixie-ui
/// doesn't care about it. Cursor movement, left-mouse-button press/release, and
/// key presses (matching [`pixie_ui::event::Key`]'s coverage) are handled.
///
/// There's no winit `WindowEvent` for clipboard paste — winit doesn't do clipboard
/// access at all — so [`PixieEvent::Paste`] has no conversion here; callers wanting
/// paste support have to source it themselves (e.g. via a clipboard crate on Ctrl+V)
/// and construct [`PixieEvent::Paste`] directly.
pub fn convert_event(event: &winit::event::WindowEvent) -> Option<PixieEvent> {
    match event {
        winit::event::WindowEvent::CursorMoved { position, .. } => Some(PixieEvent::CursorMoved {
            pos: Pos {
                x: position.x as i32,
                y: position.y as i32,
            },
        }),
        winit::event::WindowEvent::MouseInput {
            state,
            button: winit::event::MouseButton::Left,
            ..
        } => Some(match state {
            winit::event::ElementState::Pressed => PixieEvent::ButtonPressed,
            winit::event::ElementState::Released => PixieEvent::ButtonReleased,
        }),
        winit::event::WindowEvent::KeyboardInput {
            event: key_event, ..
        } if key_event.state == winit::event::ElementState::Pressed => {
            convert_key(&key_event.logical_key).map(PixieEvent::KeyPressed)
        }
        _ => None,
    }
}

/// Maps a winit logical key to a [`Key`], or `None` for anything pixie-ui doesn't react to
/// (modifier keys, function keys, dead keys, multi-char IME commits, etc).
fn convert_key(key: &winit::keyboard::Key) -> Option<Key> {
    match key {
        winit::keyboard::Key::Named(NamedKey::Tab) => Some(Key::Tab),
        winit::keyboard::Key::Named(NamedKey::Backspace) => Some(Key::Backspace),
        winit::keyboard::Key::Named(NamedKey::Delete) => Some(Key::Delete),
        winit::keyboard::Key::Named(NamedKey::ArrowLeft) => Some(Key::ArrowLeft),
        winit::keyboard::Key::Named(NamedKey::ArrowRight) => Some(Key::ArrowRight),
        winit::keyboard::Key::Named(NamedKey::Home) => Some(Key::Home),
        winit::keyboard::Key::Named(NamedKey::End) => Some(Key::End),
        winit::keyboard::Key::Named(NamedKey::Space) => Some(Key::Char(' ')),
        // `Character` is a `SmolStr`, not a single `char` — most keystrokes produce exactly one,
        // but e.g. some IME commits or dead-key combinations can produce more than pixie-ui's
        // `Key::Char` can carry, so those are dropped here rather than truncated.
        winit::keyboard::Key::Character(s) => s
            .chars()
            .next()
            .filter(|_| s.chars().count() == 1)
            .map(Key::Char),
        _ => None,
    }
}
