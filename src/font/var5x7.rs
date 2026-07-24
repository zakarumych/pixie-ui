use crate::math::{Pos, Rect, Size, Vec};

use super::{
    Font, Metrics,
    mono5x7::{FONT_MONO_5X7, GLYPH_H, GLYPH_W, mono5x7_bitmap, mono5x7_mapping},
};

/// Space has no ink to measure a width from, so it gets a fixed narrow width.
const SPACE_WIDTH: u8 = 3;

/// Width of a glyph's actual ink, trimming blank trailing columns from the fixed 5-wide source.
fn ink_rect(glyph: &[u8; 7]) -> Rect {
    let used_cols: u8 = glyph.iter().fold(0u8, |acc, &row| acc | row);

    if used_cols == 0 {
        return Rect {
            lt: Pos { x: 0, y: 0 },
            rb: Pos {
                x: SPACE_WIDTH as i32,
                y: GLYPH_H as i32,
            },
        };
    }

    Rect {
        lt: Pos {
            x: used_cols.leading_zeros() as i32 - (8 - GLYPH_W as i32),
            y: 0,
        },
        rb: Pos {
            x: GLYPH_W as i32 - used_cols.trailing_zeros() as i32,
            y: GLYPH_H as i32,
        },
    }
}

/// Same glyph source and bitmap layout as mono5x7, but each glyph's bbox/advance
/// are trimmed to its actual ink width, producing proportional text.
pub fn var5x7() -> Font {
    let glyph_metrics: Box<[Metrics]> = FONT_MONO_5X7
        .iter()
        .map(|glyph| {
            let bbox = ink_rect(glyph);
            Metrics {
                advance: Size {
                    w: (bbox.rb.x - bbox.lt.x) as u32 + 1,
                    h: (bbox.rb.y - bbox.lt.y) as u32 + 1,
                },
                bbox: bbox,
            }
        })
        .collect();

    Font {
        name: "var5x7".into(),
        size: Size {
            w: GLYPH_W,
            h: GLYPH_H,
        },
        gap: Vec { x: 0, y: 1 },
        glyph_metrics,
        bitmap: mono5x7_bitmap(),
        mapping: mono5x7_mapping(),
    }
}
