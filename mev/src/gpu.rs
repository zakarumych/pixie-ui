//! GPU-side data layouts and shader arguments.
//!
//! All instance structs are built exclusively from 16-byte `vec4` slots so that
//! `#[derive(mev::AutoDeviceRepr)]` never has to reason about mixed alignment /
//! padding between scalar and vector fields (see `mev`'s `AutoDeviceRepr` derive,
//! which panics at const-eval time if implicit padding would be required).

use bytemuck::{Pod, Zeroable};

pub const RECT_WGSL: &str = include_str!("shaders/rect.wgsl");
pub const TEXT_WGSL: &str = include_str!("shaders/text.wgsl");

/// Small uniform buffer with the target image size, shared by both pipelines.
#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod, mev::AutoDeviceRepr)]
pub struct Globals {
    pub width: u32,
    pub height: u32,
}

/// One instance of a filled or stroked rectangle.
#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod, mev::AutoDeviceRepr)]
pub struct RectInstanceGpu {
    /// `(outer_lt.x, outer_lt.y, outer_rb.x, outer_rb.y)` - the quad actually rasterized.
    pub geom0: mev::vec4<f32>,
    /// `(inner_lt.x, inner_lt.y, inner_rb.x, inner_rb.y)` - hole cut out for strokes.
    pub geom1: mev::vec4<f32>,
    /// `(is_stroke, brush_kind, 0, 0)`.
    pub flags: mev::vec4<u32>,
    pub color0: mev::vec4<f32>,
    pub color1: mev::vec4<f32>,
    /// `(grad_start.x, grad_start.y, grad_end.x, grad_end.y)`.
    pub grad: mev::vec4<f32>,
    /// `(uv_scale.x, uv_scale.y, uv_bias.x, uv_bias.y)`.
    pub uvparam: mev::vec4<f32>,
}

/// One instance of a single glyph quad.
#[repr(C)]
#[derive(Clone, Copy, Debug, Zeroable, Pod, mev::AutoDeviceRepr)]
pub struct GlyphInstanceGpu {
    /// `(lt.x, lt.y, rb.x, rb.y)` placement of the glyph quad in pixel space.
    pub geom: mev::vec4<f32>,
    /// `(u0, v0, u1, v1)` coverage rect within the font atlas.
    pub atlas_uv: mev::vec4<f32>,
    /// `(brush_kind, 0, 0, 0)`.
    pub flags: mev::vec4<u32>,
    pub color0: mev::vec4<f32>,
    pub color1: mev::vec4<f32>,
    pub grad: mev::vec4<f32>,
    pub uvparam: mev::vec4<f32>,
}

/// Shader arguments for the rect pipeline (fills and strokes).
#[derive(mev::Arguments)]
pub struct RectArguments {
    #[mev(uniform)]
    #[mev(vertex)]
    #[mev(fragment)]
    pub globals: mev::Buffer,

    #[mev(storage)]
    #[mev(vertex)]
    #[mev(fragment)]
    pub instances: mev::Buffer,

    #[mev(fragment)]
    pub tex: mev::Image,

    #[mev(fragment)]
    pub samp: mev::Sampler,
}

/// Shader arguments for the text pipeline.
#[derive(mev::Arguments)]
pub struct TextArguments {
    #[mev(uniform)]
    #[mev(vertex)]
    #[mev(fragment)]
    pub globals: mev::Buffer,

    #[mev(storage)]
    #[mev(vertex)]
    #[mev(fragment)]
    pub instances: mev::Buffer,

    #[mev(fragment)]
    pub atlas: mev::Image,

    #[mev(fragment)]
    pub atlas_samp: mev::Sampler,

    #[mev(fragment)]
    pub tex: mev::Image,

    #[mev(fragment)]
    pub samp: mev::Sampler,
}
