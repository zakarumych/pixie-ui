use std::borrow::Cow;

use crate::{
    align::Align2,
    color::Color,
    font::FontId,
    math::{Pos, Rect, Vec},
    text::Glyph,
    texture::TextureId,
};

/// An enum representing the different ways to handle texture coordinates that fall outside its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressMode {
    /// The texture should be repeated in the corresponding direction.
    /// Effective pixel coordinates are calculated as `coord % size`.
    Repeat,

    /// The texture should be repeated mirrored in the corresponding direction.
    /// Effective pixel coordinates are calculated as `abs((coord + size) % (2 * size) - size)`.
    Mirrored,

    /// The texture should be clamped to the edge in the corresponding direction.
    /// Effective pixel coordinates are calculated as `min(max(coord, 0), size - 1)`.
    Edge,
}

/// A struct representing the address mode for both horizontal and vertical directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AddressMode2 {
    /// The address mode for the X axis.
    pub x: AddressMode,

    /// The address mode for the Y axis.
    pub y: AddressMode,
}

impl From<AddressMode> for AddressMode2 {
    #[inline]
    fn from(value: AddressMode) -> Self {
        AddressMode2 { x: value, y: value }
    }
}

/// An enum representing the different ways to scale a texture when placing it into a rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureScale {
    /// The texture should be drawn at its original size.
    Original,

    /// The texture should be stretched in both dimensions to fill the entire area, regardless of its aspect ratio.
    Stretch,

    /// The texture should be scaled uniformly to fill the entire area while maintaining its aspect ratio.
    /// One dimenstion of the texture will be equal to the corresponding dimension of the area,
    /// and the other dimension will be greater than or equal to the corresponding dimension of the area.
    Span,

    /// The texture should be scaled uniformly to fit within the area while maintaining its aspect ratio.
    /// One dimenstion of the texture will be equal to the corresponding dimension of the area,
    /// and the other dimension will be less than or equal to the corresponding dimension of the area.
    /// Thus that dimension of the texture will be placed according to the specified alignment.
    Fit,
}

/// Brush applies a color to pixels in the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Brush {
    /// A solid color brush.
    Solid(Color),

    /// A linear gradient brush.
    LinearGradient {
        start: (Pos, Color),
        end: (Pos, Color),
    },

    /// A texture brush.
    Texture {
        /// The texture to use for the brush.
        texture: TextureId,

        /// The scale mode to use for the texture.
        scale: TextureScale,

        /// The alignment to be used for the texture.
        align: Align2,

        /// The address mode to be used for pixels outside the bounds of the texture.
        mode: AddressMode2,
    },
}

/// A stroke style for drawing shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Stroke {
    /// The brush to be used for the stroke.
    pub brush: Brush,

    /// The width of the stroke in pixels.
    pub width: u32,

    /// The offset of the stroke from the shape's edge in pixels.
    /// Negative values will move stroke inward,
    /// while positive values will move it outward.
    pub offset: i32,
}

/// A draw command that can be used to render graphics on the screen.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Draw<'a> {
    /// Draw a rectangle with the specified geometry, fill, and stroke.
    Rect {
        /// The geometry of the rectangle to be drawn.
        geometry: Rect,

        /// The fill style to be used for the rectangle.
        /// If `None`, the rectangle will not be filled.
        fill: Option<Brush>,

        /// The stroke style to be used for the rectangle.
        /// If `None`, the rectangle will not be stroked.
        stroke: Option<Stroke>,
    },

    Text {
        /// The geometry of the rectangle in which the text will be drawn.
        start: Vec,

        /// The font to be used for the text.
        font: FontId,

        /// The glyphs from the font for the text to be drawn.
        glyphs: Cow<'a, [Glyph]>,

        /// The brush to be used for the text pixels.
        brush: Brush,
    },
}

impl<'a> Draw<'a> {
    /// Converts the `Draw` command into a static lifetime version.
    pub fn into_static(self) -> Draw<'static> {
        match self {
            Draw::Rect {
                geometry,
                fill,
                stroke,
            } => Draw::Rect {
                geometry,
                fill,
                stroke,
            },
            Draw::Text {
                start,
                font,
                glyphs,
                brush,
            } => Draw::Text {
                start,
                font,
                glyphs: Cow::Owned(glyphs.into_owned()),
                brush,
            },
        }
    }
}
