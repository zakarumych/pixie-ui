//! Lazily-built glyph coverage atlas for a single `pixie_ui::font::Font`.

use pixie_ui::font::Font;

/// A single-channel (R8Unorm) coverage atlas holding every glyph of a font,
/// packed into a uniform grid of `glyph_w x glyph_h` cells.
pub struct Atlas {
    pub image: mev::Image,
    cols: u32,
    glyph_w: u32,
    glyph_h: u32,
    atlas_w: u32,
    atlas_h: u32,
}

impl Atlas {
    /// Returns the UV rect (u0, v0, u1, v1) covering the `visible_w x visible_h` sub-rectangle
    /// of the glyph's cell starting at `(offset_x, offset_y)` from the cell's top-left corner
    /// (i.e. the ink's own bounding-box origin within the cell — glyphs whose ink doesn't start
    /// flush at the cell edge, e.g. a trimmed proportional font, need this to sample the right
    /// sub-rectangle instead of always starting at the cell's own origin).
    pub fn glyph_uv(
        &self,
        glyph_index: u32,
        offset_x: u32,
        offset_y: u32,
        visible_w: u32,
        visible_h: u32,
    ) -> (f32, f32, f32, f32) {
        let col = glyph_index % self.cols;
        let row = glyph_index / self.cols;
        let offset_x = offset_x.min(self.glyph_w);
        let offset_y = offset_y.min(self.glyph_h);
        let x0 = col * self.glyph_w + offset_x;
        let y0 = row * self.glyph_h + offset_y;
        let visible_w = visible_w.min(self.glyph_w - offset_x);
        let visible_h = visible_h.min(self.glyph_h - offset_y);

        let atlas_w = self.atlas_w as f32;
        let atlas_h = self.atlas_h as f32;
        let u0 = x0 as f32 / atlas_w;
        let v0 = y0 as f32 / atlas_h;
        let u1 = (x0 + visible_w) as f32 / atlas_w;
        let v1 = (y0 + visible_h) as f32 / atlas_h;
        (u0, v0, u1, v1)
    }
}

#[inline]
fn round_up_32(bits: usize) -> usize {
    (bits + 31) & !31
}

/// Builds an atlas texture for `font`, uploading it synchronously (creates its own
/// one-off command buffer, submits, and waits for completion).
pub fn build_atlas(queue: &mut mev::Queue, font: &Font) -> Result<Atlas, crate::Error> {
    let glyph_w = font.bitmap.glyph_size.w.max(1) as u32;
    let glyph_h = font.bitmap.glyph_size.h.max(1) as u32;
    let num_glyphs = (font.glyph_metrics.len() as u32).max(1);

    let cols = (num_glyphs as f64).sqrt().ceil().max(1.0) as u32;
    let rows = num_glyphs.div_ceil(cols).max(1);
    let atlas_w = cols * glyph_w;
    let atlas_h = rows * glyph_h;

    let mut pixels = vec![0u8; (atlas_w * atlas_h) as usize];

    let glyph_bits = glyph_w as usize * glyph_h as usize;
    let glyph_stride_bits = round_up_32(glyph_bits);
    let bitmap = &font.bitmap.bitmap;

    for gi in 0..font.glyph_metrics.len() as u32 {
        let col = gi % cols;
        let row = gi / cols;
        let base_bit = gi as usize * glyph_stride_bits;

        for y in 0..glyph_h {
            for x in 0..glyph_w {
                let bit_index = base_bit + (x * glyph_h + y) as usize;
                let byte_index = bit_index / 8;
                let bit_in_byte = bit_index % 8;
                let bit = bitmap
                    .get(byte_index)
                    .map(|byte| (byte >> bit_in_byte) & 1)
                    .unwrap_or(0);

                let px = col * glyph_w + x;
                let py = row * glyph_h + y;
                pixels[(py * atlas_w + px) as usize] = if bit != 0 { 255 } else { 0 };
            }
        }
    }

    let image = queue.new_image(mev::ImageDesc::new_d2_texture(
        atlas_w,
        atlas_h,
        mev::PixelFormat::R8Unorm,
    ));

    let staging = queue.new_buffer_init(mev::BufferInitDesc {
        data: &pixels,
        usage: mev::BufferUsage::TRANSFER_SRC,
        name: "pixie-ui-mev-glyph-atlas-staging",
    });

    let mut encoder = queue.new_command_encoder();
    encoder.init_image(
        mev::PipelineStages::empty(),
        mev::PipelineStages::TRANSFER,
        &image,
    );
    {
        let mut copy = encoder.copy();
        copy.copy_buffer_to_image(
            &staging,
            0,
            atlas_w as usize,
            (atlas_w as usize) * (atlas_h as usize),
            &image,
            mev::Offset3::ZERO,
            mev::Extent3::new(atlas_w, atlas_h, 1),
            0..1,
            0,
        );
        copy.barrier(
            mev::PipelineStages::TRANSFER,
            mev::PipelineStages::FRAGMENT_SHADER,
        );
    }
    let cbuf = encoder.finish();
    queue.submit_checkpoint([cbuf])?;
    queue.wait_idle()?;

    Ok(Atlas {
        image,
        cols,
        glyph_w,
        glyph_h,
        atlas_w,
        atlas_h,
    })
}
