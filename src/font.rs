use foldhash::fast::RandomState;
use hashbrown::HashMap;

use crate::{
    math::{Pos, Rect, Size, Vec},
    text::Glyph,
};

mod mono5x7;
mod var5x7;

pub use self::{mono5x7::mono5x7, var5x7::var5x7};

pub struct Bitmap {
    /// The size of each glyph in pixels within the bitmap.
    /// All glyphs in the font must have the same size.
    pub glyph_size: Size,

    /// The bitmap data for each glyph in the font.
    /// Each glyph occupies round_up(glyph_size.w * glyph_size.h, 32) bits in the bitmap,
    /// packed in column-major order: bit index within a glyph is `x * glyph_size.h + y`
    /// (column `x`, row `y`), rounded up to a whole 32-bit chunk per glyph.
    pub bitmap: Box<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Metrics {
    /// The horizontal and vertical advance of the glyph in pixels.
    pub advance: Size,

    /// Offset from the pen position to `bbox`'s top-left corner when drawing on screen
    /// (e.g. a left/top bearing). Zero draws `bbox` flush at the pen. Kept separate from
    /// `bbox` itself since `bbox` lives in bitmap-cell coordinates (see below) — needed
    /// for glyphs whose ink isn't meant to start exactly at the pen, e.g. italic overhang
    /// or other bearings a more complex font might have.
    pub offset: Vec,

    /// The sub-rectangle of the glyph's fixed-size bitmap cell that actually holds ink,
    /// in bitmap-cell pixel coordinates (`(0, 0)` is the cell's own top-left corner, not
    /// the pen/cursor position). Used both as the rendered glyph's on-screen size and,
    /// via its `lt` offset, to locate that sub-rectangle within a packed atlas cell.
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

impl Font {
    /// Returns the bounding rectangle of the text when rendered with this font.
    pub fn text_bbox(&self, text: &str) -> Rect {
        let mut rect = Rect::ZERO;
        let mut cursor = Pos::ZERO;

        for c in text.chars() {
            if let Some(&glyph) = self.mapping.get(&c) {
                let metrics = &self.glyph_metrics[glyph as usize];
                let mut bbox = metrics.bbox;
                bbox.lt.x += cursor.x;
                bbox.lt.y += cursor.y;
                bbox.rb.x += cursor.x;
                bbox.rb.y += cursor.y;
                rect = rect.union(&bbox);
                cursor.x += metrics.advance.w as i32;
            }
        }
        rect
    }

    /// Returns the bounding rectangle of the text when rendered with this font.
    pub fn glyphs_bbox(&self, glyphs: &[Glyph]) -> Rect {
        let mut rect = Rect::ZERO;
        let mut cursor = Pos::ZERO;

        for &glyph in glyphs {
            if let Some(metrics) = self.glyph_metrics.get(glyph.0 as usize) {
                let mut bbox = metrics.bbox;
                bbox.lt.x += cursor.x;
                bbox.lt.y += cursor.y;
                bbox.rb.x += cursor.x;
                bbox.rb.y += cursor.y;
                rect = rect.union(&bbox);
                cursor.x += metrics.advance.w as i32;
            }
        }
        rect
    }

    pub fn metrics(&self, glyph: Glyph) -> Option<Metrics> {
        self.glyph_metrics.get(glyph.0 as usize).copied()
    }
}

/// A unique identifier for a font.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub u32);
