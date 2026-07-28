use crate::math::{Rect, Size, Vec};

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

impl Align {
    /// Returns the offset needed to align an element of `length` within a container of `available` space.
    pub fn offset(self, available: i32, length: i32) -> i32 {
        match self {
            Align::Start => 0,
            Align::Center => (available - length) / 2,
            Align::End => available - length,
        }
    }
}

/// Alignment option for both horizontal and vertical axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Align2 {
    /// The alignment for the X axis.
    pub x: Align,

    /// The alignment for the Y axis.
    pub y: Align,
}

impl Align2 {
    pub const CENTER: Align2 = Align2 {
        x: Align::Center,
        y: Align::Center,
    };

    pub const LEFT: Align2 = Align2 {
        x: Align::Start,
        y: Align::Center,
    };

    pub const RIGHT: Align2 = Align2 {
        x: Align::End,
        y: Align::Center,
    };

    pub const TOP: Align2 = Align2 {
        x: Align::Center,
        y: Align::Start,
    };

    pub const BOTTOM: Align2 = Align2 {
        x: Align::Center,
        y: Align::End,
    };

    pub const TOP_LEFT: Align2 = Align2 {
        x: Align::Start,
        y: Align::Start,
    };

    pub const TOP_RIGHT: Align2 = Align2 {
        x: Align::End,
        y: Align::Start,
    };

    pub const BOTTOM_LEFT: Align2 = Align2 {
        x: Align::Start,
        y: Align::End,
    };

    pub const BOTTOM_RIGHT: Align2 = Align2 {
        x: Align::End,
        y: Align::End,
    };

    /// Returns the offset needed to align an element of `size` within a container of `available` space.
    pub fn offset(self, available: Size, size: Size) -> Vec {
        Vec {
            x: self.x.offset(available.w as i32, size.w as i32),
            y: self.y.offset(available.h as i32, size.h as i32),
        }
    }

    /// Returns the offset needed to align a rectangle within an available rectangle.
    pub fn rect_offset(self, available: Rect, rect: Rect) -> Vec {
        let target_offset = self.offset(available.size(), rect.size());
        Vec::from(available.lt) + target_offset - Vec::from(rect.lt)
    }

    /// Returns a rectangle of specified size aligned within an available rectangle.
    pub fn in_rect(self, available: Rect, size: Size) -> Rect {
        let offset = self.offset(available.size(), size);
        Rect::from_pos_size(available.lt + offset, size)
    }
}

impl From<Align> for Align2 {
    #[inline]
    fn from(value: Align) -> Self {
        Align2 { x: value, y: value }
    }
}
