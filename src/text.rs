use edict::component::Component;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Glyph(pub u32);

/// Text is a component for a widget to display text.
#[derive(Component)]
pub struct Text {
    pub string: String,
}
