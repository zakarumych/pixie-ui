/// Alignment option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    /// Align to the start of the container.
    /// For horizontal alignment, this means left-aligned.
    /// For vertical alignment, this means top-aligned.
    Start,

    /// Align to the center of the container.
    Center,

    /// Align to the end of the container.
    /// For horizontal alignment, this means right-aligned.
    /// For vertical alignment, this means bottom-aligned.
    End,
}

/// Alignment option for both horizontal and vertical axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Align2 {
    /// The alignment for the X axis.
    pub x: Align,

    /// The alignment for the Y axis.
    pub y: Align,
}

impl From<Align> for Align2 {
    #[inline]
    fn from(value: Align) -> Self {
        Align2 { x: value, y: value }
    }
}
