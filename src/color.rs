/// Exact `round(x / 255)` for `x` in `0..=65025` (i.e. `255*255`), computed
/// without division.
#[inline]
const fn div255(x: u32) -> u32 {
    ((x + 128) * 257) >> 16
}

/// Premultiplied-alpha color. Invariant: `r,g,b <= a`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn from_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    /// Premultiplies unmultiplied (unassociated) alpha channels: `c' = round(c * a / 255)`.
    pub const fn from_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        let a32 = a as u32;
        Color {
            r: div255(r as u32 * a32) as u8,
            g: div255(g as u32 * a32) as u8,
            b: div255(b as u32 * a32) as u8,
            a,
        }
    }

    /// Un-premultiplies back to unmultiplied alpha. Lossy when `a < 255`. `a == 0` yields all zeros.
    pub const fn unmultiplied_components(self) -> (u8, u8, u8, u8) {
        if self.a == 0 {
            return (0, 0, 0, 0);
        }
        let a32 = self.a as u32;
        let r = ((self.r as u32 * 255 + a32 / 2) / a32) as u8;
        let g = ((self.g as u32 * 255 + a32 / 2) / a32) as u8;
        let b = ((self.b as u32 * 255 + a32 / 2) / a32) as u8;
        (r, g, b, self.a)
    }

    /// Rescales premultiplied channels to a new alpha, keeping the same unmultiplied color.
    pub const fn with_alpha(self, a: u8) -> Self {
        if self.a == 0 {
            return Color {
                r: 0,
                g: 0,
                b: 0,
                a,
            };
        }
        let a32 = self.a as u32;
        let new_a32 = a as u32;
        Color {
            r: div255(self.r as u32 * new_a32 * 255 / a32 / 255) as u8,
            g: div255(self.g as u32 * new_a32 * 255 / a32 / 255) as u8,
            b: div255(self.b as u32 * new_a32 * 255 / a32 / 255) as u8,
            a,
        }
    }

    pub const fn under(self, dst: Color) -> Color {
        dst.over(self)
    }

    /// Premultiplied "src over dst" compositing.
    pub const fn over(self, dst: Color) -> Color {
        let inv_a = 255 - self.a as u32;
        Color {
            r: (self.r as u32 + div255(dst.r as u32 * inv_a)) as u8,
            g: (self.g as u32 + div255(dst.g as u32 * inv_a)) as u8,
            b: (self.b as u32 + div255(dst.b as u32 * inv_a)) as u8,
            a: (self.a as u32 + div255(dst.a as u32 * inv_a)) as u8,
        }
    }

    /// Integer linear interpolation, `t` in `0..=255` (0 = self, 255 = other).
    pub const fn lerp(self, other: Color, t: u8) -> Color {
        let t32 = t as u32;
        let inv_t = 255 - t32;
        Color {
            r: div255(self.r as u32 * inv_t + other.r as u32 * t32) as u8,
            g: div255(self.g as u32 * inv_t + other.g as u32 * t32) as u8,
            b: div255(self.b as u32 * inv_t + other.b as u32 * t32) as u8,
            a: div255(self.a as u32 * inv_t + other.a as u32 * t32) as u8,
        }
    }
}
