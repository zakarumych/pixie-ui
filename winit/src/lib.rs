use pixie_ui::{event::PixieEvent, math::Pos};

/// Converts a winit window event into a pixie-ui event, or `None` if pixie-ui
/// doesn't care about it. Only cursor movement and left-mouse-button press/release
/// are handled for now.
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
        _ => None,
    }
}
