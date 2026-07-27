use crate::math::Pos;

/// Input events pixie-ui reacts to. Deliberately minimal for now — just enough
/// to drive hover/press state. More event kinds (keyboard, scroll, etc.) can be
/// added later without breaking this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixieEvent {
    CursorMoved { pos: Pos },
    ButtonPressed,
    ButtonReleased,
}
