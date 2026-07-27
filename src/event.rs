use crate::math::Pos;

/// Keys pixie-ui reacts to. Deliberately minimal for now — more can be added later
/// without breaking this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Tab,
}

/// Input events pixie-ui reacts to. Deliberately minimal for now — just enough
/// to drive hover/press/focus state. More event kinds (scroll, etc.) can be
/// added later without breaking this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixieEvent {
    CursorMoved { pos: Pos },
    ButtonPressed,
    ButtonReleased,
    KeyPressed(Key),
}
