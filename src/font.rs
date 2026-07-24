use foldhash::fast::RandomState;
use hashbrown::HashMap;

use crate::math::{Rect, Size, Vec};

mod mono5x7;
mod var5x7;

pub use self::{mono5x7::mono5x7, var5x7::var5x7};

pub struct Bitmap {
    /// The size of each glyph in pixels within the bitmap.
    /// All glyphs in the font must have the same size.
    pub glyph_size: Size,

    /// The bitmap data for each glyph in the font.
    /// Each glyph occupies round_up(glyph_size.w * glyph_size.h, 32) bits in the bitmap.
    pub bitmap: Box<[u8]>,
}

pub struct Metrics {
    /// The horizontal and vertical advance of the glyph in pixels.
    pub advance: Size,

    /// The bounding box of the glyph in pixels with respect to the glyph's origin.
    ///
    /// When reading bitmap, the size of this rect is used to determine the size of the glyph in the bitmap.
    pub bbox: Rect,
}

/// Font is a collection of glyphs and their associated metrics. It is used to render text in a user interface.
pub struct Font {
    /// The name of the font.
    pub name: Box<str>,

    /// The size of the glyphs in pixels.
    /// Although glyphs have their specific size,
    /// the general size is used for reasoning about lines.
    ///
    /// For example in case of horizontal text, the line height is determined by the font size,
    /// while line width is determined by the sum of the advance widths of the glyphs in the line.
    pub size: Size,

    /// The gap required between lines of text in pixels.
    /// Negative values will cause lines to overlap, while positive values will cause lines to be spaced apart.
    pub gap: Vec,

    /// The metrics for each glyph in the font.
    pub glyph_metrics: Box<[Metrics]>,

    /// The bitmap containing the glyphs for the font.
    pub bitmap: Bitmap,

    /// The mapping from characters to glyph indices in the font.
    pub mapping: HashMap<char, u32, RandomState>,
}

/// A unique identifier for a font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub u32);
