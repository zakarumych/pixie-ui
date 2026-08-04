use std::borrow::Cow;

use edict::{
    entity::EntityId,
    query::{Cpy, Entities},
    world::World,
};

use crate::{
    align::{Align, Align2},
    color::Color,
    font::FontId,
    layout::{Arranged, shrink_cross_inset},
    math::{Pos, Rect, Size, Vec},
    style::{LayoutAttributes, PaintAttributes},
    text::{Glyph, Text, TextInput},
    texture::TextureId,
    ui::Ui,
    widget::{Container, Widget},
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

impl Stroke {
    /// Creates a new stroke with the specified brush, width, and offset.
    pub fn new(brush: Brush, width: u32) -> Self {
        Stroke {
            brush,
            width,
            offset: -(width as i32),
        }
    }
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

/// Per-widget cascade of the paint-related attributes that inherit down the widget tree
/// (analogous to CSS inherited properties): a widget's own [`crate::style::Attributes`] field,
/// if `None`, falls back to its parent's already-resolved value for that field. Root widgets
/// fall back to [`Ui`]'s corresponding `default_*` value, as if `Ui` were their "parent".
#[derive(Clone, Copy)]
struct InheritedPaint {
    fg_brush: Brush,
    bg_brush: Brush,
    stroke: Option<Stroke>,
    font: FontId,
}

/// Walks every [`Widget`] entity and emits [`Draw`] commands describing how to render it,
/// in painter's-algorithm order (a parent's own background is emitted before its children's).
///
/// # Preconditions
///
/// Every `Widget` entity that should be drawn must already have a [`Arranged`] component
/// (i.e. this must run after [`crate::layout::resolve_rects`]) and a [`FinalAttributes`]
/// component with theme/fallback merging applied (i.e. this must run after
/// [`crate::style::Style::resolve_attributes`]). Widgets without a `Arranged` (and, by
/// construction from `resolve_rects`, their entire subtree) are silently skipped.
pub(crate) fn draw_ui<'a>(world: &mut World, commands: &mut impl Extend<Draw<'a>>) {
    let Some(ui) = world.remove_resource::<Ui>() else {
        return;
    };

    let roots: std::vec::Vec<EntityId> = world
        .view::<(Entities, &Widget)>()
        .into_iter()
        .filter(|(_, w)| w.parent.is_none())
        .map(|(e, _)| e.id())
        .collect();

    let root_inherited = InheritedPaint {
        fg_brush: Brush::Solid(ui.default_fg_color()),
        bg_brush: Brush::Solid(ui.default_bg_color()),
        stroke: None,
        font: ui.default_font(),
    };

    for root in roots {
        draw_widget(world, &ui, root, root_inherited, commands);
    }

    world.insert_resource(ui);
}

/// Draws `id` and, recursively, its subtree, given the [`InheritedPaint`] cascaded down from
/// its parent (or from `Ui`'s defaults, for a root widget).
///
/// `world`'s borrow (`'w`) is deliberately a separate lifetime from the `Draw` data (`'a`):
/// nothing drawn here ever borrows from `world` (all emitted `Draw` values own their data, e.g.
/// `Cow::Owned` glyphs), so tying the two lifetimes together would force `world` to be borrowed
/// for all of `'a` at every call site, making recursion/looping impossible.
fn draw_widget<'w, 'a>(
    world: &'w mut World,
    ui: &Ui,
    id: EntityId,
    inherited: InheritedPaint,
    commands: &mut impl Extend<Draw<'a>>,
) {
    let Ok(rect) = world.get::<&Arranged>(id).map(|r| r.rect) else {
        // Unresolved widget: by construction from `resolve_rects`, its whole subtree is
        // also unresolved, so there's nothing further to draw here.
        return;
    };

    let Ok((layout, paint)) = world.get::<(Cpy<LayoutAttributes>, Cpy<PaintAttributes>)>(id) else {
        return;
    };

    let resolved = InheritedPaint {
        fg_brush: paint.fg_brush.unwrap_or(inherited.fg_brush),
        bg_brush: paint.bg_brush.unwrap_or(inherited.bg_brush),
        stroke: paint.stroke.or(inherited.stroke),
        font: layout.font.unwrap_or(inherited.font),
    };

    commands.extend(std::iter::once(Draw::Rect {
        geometry: rect,
        fill: Some(resolved.bg_brush),
        stroke: resolved.stroke,
    }));

    if let Ok((Some(text), text_input)) = world.get::<(Option<&Text>, Option<&TextInput>)>(id)
        && let Some(font) = ui.font(resolved.font)
    {
        let inner_margin = layout.inner_margin.unwrap_or(ui.default_inner_margin());

        let content_align = layout.content_align.unwrap_or(Align::Start.into());
        let text_rect = font.text_bbox(&text.string);

        // Inset the text by `inner_margin`, shrinking smoothly per axis (never a discrete
        // jump to zero margin) as the text grows to fill the widget's own rect — reuses the
        // exact same shrink used for cross-axis insets during layout, just applied here at
        // draw time against the widget's final rect and its own text content.
        let rect_size = rect.size();
        let text_size = text_rect.size();
        let (left_inset, right_inset) = shrink_cross_inset(
            inner_margin.left as i32,
            inner_margin.right as i32,
            rect_size.w,
            text_size.w,
        );
        let (top_inset, bottom_inset) = shrink_cross_inset(
            inner_margin.top as i32,
            inner_margin.bottom as i32,
            rect_size.h,
            text_size.h,
        );
        let content_rect = Rect {
            lt: Pos {
                x: rect.lt.x + left_inset,
                y: rect.lt.y + top_inset,
            },
            rb: Pos {
                x: rect.rb.x - right_inset,
                y: rect.rb.y - bottom_inset,
            },
        };

        let text_offset = content_align.rect_offset(content_rect, text_rect);

        match text_input {
            None => {
                let glyphs: std::vec::Vec<Glyph> = text
                    .string
                    .chars()
                    .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                    .collect();

                commands.extend(std::iter::once(Draw::Text {
                    start: text_offset,
                    font: resolved.font,
                    glyphs: Cow::Owned(glyphs),
                    brush: resolved.fg_brush,
                }));
            }
            Some(text_input) => {
                let before_selection = &text.string[..text_input.selection.start];
                let selection = &text.string[text_input.selection.clone()];
                let after_selection = &text.string[text_input.selection.end..];

                let mut text_offset = text_offset;

                if !before_selection.is_empty() {
                    let glyphs: std::vec::Vec<Glyph> = before_selection
                        .chars()
                        .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                        .collect();

                    let mut advance = 0;
                    for &g in glyphs.iter() {
                        advance += font.metrics(g).map_or(0, |m| m.advance.w as i32);
                    }

                    commands.extend(std::iter::once(Draw::Text {
                        start: text_offset,
                        font: resolved.font,
                        glyphs: Cow::Owned(glyphs),
                        brush: resolved.fg_brush,
                    }));

                    text_offset.x += advance;
                }

                if text_input.selection.is_empty()
                    && ui.focused() == Some(id)
                    && (ui.elapsed().as_millis() % 1000 < 500)
                {
                    let x_offset = text_offset.x - 1;
                    let y_offset = content_rect.lt.y
                        + content_align
                            .y
                            .offset(content_rect.size().h, font.size.h + 2);

                    commands.extend(std::iter::once(Draw::Rect {
                        geometry: Rect::from_pos_size(
                            Pos {
                                x: x_offset,
                                y: y_offset,
                            },
                            Size {
                                w: 1,
                                h: font.size.h + 2,
                            },
                        ),

                        fill: Some(Brush::Solid(Color::from_premultiplied(100, 100, 100, 0))),
                        stroke: None,
                    }));
                }

                if !selection.is_empty() {
                    let glyphs: std::vec::Vec<Glyph> = selection
                        .chars()
                        .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                        .collect();

                    let mut advance = 0;
                    for &g in glyphs.iter() {
                        advance += font.metrics(g).map_or(0, |m| m.advance.w as i32);
                    }

                    commands.extend(std::iter::once(Draw::Text {
                        start: text_offset,
                        font: resolved.font,
                        glyphs: Cow::Owned(glyphs),
                        brush: resolved.fg_brush,
                    }));

                    text_offset.x += advance;
                }

                if !after_selection.is_empty() {
                    let glyphs: std::vec::Vec<Glyph> = after_selection
                        .chars()
                        .filter_map(|c| font.mapping.get(&c).map(|&idx| Glyph(idx)))
                        .collect();

                    commands.extend(std::iter::once(Draw::Text {
                        start: text_offset,
                        font: resolved.font,
                        glyphs: Cow::Owned(glyphs),
                        brush: resolved.fg_brush,
                    }));
                }
            }
        }
    }

    let children = world
        .get::<Option<&Container>>(id)
        .ok()
        .flatten()
        .map(|c| c.children.clone());

    let child_paint = InheritedPaint {
        fg_brush: resolved.fg_brush,
        bg_brush: inherited.bg_brush,
        stroke: inherited.stroke,
        font: resolved.font,
    };

    if let Some(children) = children {
        for child in children {
            draw_widget(world, ui, child, child_paint, commands);
        }
    }
}
