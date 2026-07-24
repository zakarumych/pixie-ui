//! Resolves a pixie-ui `Brush` into the flat set of numbers the shaders consume.

use pixie_ui::{
    align::Align,
    color::Color,
    draw::{AddressMode2, Brush, TextureScale},
    math::Rect,
    texture::TextureId,
};

/// A brush resolved to GPU-ready fields.
pub struct ResolvedBrush {
    /// 0 = solid, 1 = linear gradient, 2 = texture.
    pub kind: u32,
    pub color0: [f32; 4],
    pub color1: [f32; 4],
    pub grad_start: [f32; 2],
    pub grad_end: [f32; 2],
    /// `uv = pixel_pos * uv_scale + uv_bias`, in absolute pixel space.
    pub uv_scale: [f32; 2],
    pub uv_bias: [f32; 2],
    /// Set when `kind == 2`.
    pub texture: Option<(TextureId, AddressMode2)>,
}

fn color_to_f32(c: Color) -> [f32; 4] {
    [
        c.r as f32 / 255.0,
        c.g as f32 / 255.0,
        c.b as f32 / 255.0,
        c.a as f32 / 255.0,
    ]
}

fn align_offset(container: f32, content: f32, align: Align) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center => (container - content) * 0.5,
        Align::End => container - content,
    }
}

/// Resolves `brush` against `rect` (the geometry / text bounding rect the brush is applied to).
///
/// `tex_size` should return the pixel size of the registered texture, if known.
pub fn resolve_brush(
    brush: &Brush,
    rect: Rect,
    tex_size: impl FnOnce(TextureId) -> Option<(f32, f32)>,
) -> ResolvedBrush {
    match *brush {
        Brush::Solid(color) => ResolvedBrush {
            kind: 0,
            color0: color_to_f32(color),
            color1: [0.0; 4],
            grad_start: [0.0; 2],
            grad_end: [0.0; 2],
            uv_scale: [0.0; 2],
            uv_bias: [0.0; 2],
            texture: None,
        },
        Brush::LinearGradient { start, end } => ResolvedBrush {
            kind: 1,
            color0: color_to_f32(start.1),
            color1: color_to_f32(end.1),
            grad_start: [start.0.x as f32, start.0.y as f32],
            grad_end: [end.0.x as f32, end.0.y as f32],
            uv_scale: [0.0; 2],
            uv_bias: [0.0; 2],
            texture: None,
        },
        Brush::Texture {
            texture,
            scale,
            align,
            mode,
        } => {
            let rect_w = ((rect.rb.x - rect.lt.x) as f32).max(1.0);
            let rect_h = ((rect.rb.y - rect.lt.y) as f32).max(1.0);
            let (tex_w, tex_h) = tex_size(texture).unwrap_or((rect_w, rect_h));
            let tex_w = tex_w.max(1.0);
            let tex_h = tex_h.max(1.0);

            let (eff_w, eff_h) = match scale {
                TextureScale::Original => (tex_w, tex_h),
                TextureScale::Stretch => (rect_w, rect_h),
                TextureScale::Span => {
                    let s = (rect_w / tex_w).max(rect_h / tex_h);
                    (tex_w * s, tex_h * s)
                }
                TextureScale::Fit => {
                    let s = (rect_w / tex_w).min(rect_h / tex_h);
                    (tex_w * s, tex_h * s)
                }
            };
            let eff_w = eff_w.max(1e-6);
            let eff_h = eff_h.max(1e-6);

            let off_x = align_offset(rect_w, eff_w, align.x);
            let off_y = align_offset(rect_h, eff_h, align.y);

            let scale_x = 1.0 / eff_w;
            let scale_y = 1.0 / eff_h;

            // uv = (pixel_pos - rect.lt - offset) * scale
            let bias_x = (-off_x - rect.lt.x as f32) * scale_x;
            let bias_y = (-off_y - rect.lt.y as f32) * scale_y;

            ResolvedBrush {
                kind: 2,
                color0: [0.0; 4],
                color1: [0.0; 4],
                grad_start: [0.0; 2],
                grad_end: [0.0; 2],
                uv_scale: [scale_x, scale_y],
                uv_bias: [bias_x, bias_y],
                texture: Some((texture, mode)),
            }
        }
    }
}
