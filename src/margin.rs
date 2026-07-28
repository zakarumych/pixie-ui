use crate::math::{Pos, Rect, Size};

/// Margin for a widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Margin {
    /// The top margin of the widget.
    pub top: u8,

    /// The left margin of the widget.
    pub left: u8,

    /// The bottom margin of the widget.
    pub bottom: u8,

    /// The right margin of the widget.
    pub right: u8,
}

impl Margin {
    /// Zero margin (all sides are 0).
    pub const ZERO: Self = Margin {
        top: 0,
        left: 0,
        bottom: 0,
        right: 0,
    };

    pub const fn new(top: u8, left: u8, bottom: u8, right: u8) -> Self {
        Margin {
            top,
            left,
            bottom,
            right,
        }
    }

    pub const fn uniform(margin: u8) -> Self {
        Margin {
            top: margin,
            left: margin,
            bottom: margin,
            right: margin,
        }
    }

    pub const fn symmetric(horizontal: u8, vertical: u8) -> Self {
        Margin {
            top: vertical,
            left: horizontal,
            bottom: vertical,
            right: horizontal,
        }
    }

    pub const fn size(&self) -> Size {
        Size {
            w: self.left as i32 + self.right as i32,
            h: self.top as i32 + self.bottom as i32,
        }
    }

    pub fn shrink(&self, rect: Rect) -> Rect {
        Rect {
            lt: Pos {
                x: rect.lt.x + self.left as i32,
                y: rect.lt.y + self.top as i32,
            },
            rb: Pos {
                x: rect.rb.x - self.right as i32,
                y: rect.rb.y - self.bottom as i32,
            },
        }
    }
}

impl From<u8> for Margin {
    fn from(margin: u8) -> Self {
        Margin::uniform(margin)
    }
}

impl From<(u8, u8)> for Margin {
    fn from((horizontal, vertical): (u8, u8)) -> Self {
        Margin::symmetric(horizontal, vertical)
    }
}
