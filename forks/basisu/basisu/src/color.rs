//! `Color32`: a 32-bit RGBA color stored as the four bytes `c` in r, g, b, a
//! order, with a u32 view `m()` over those same bytes. The setters use C-style
//! truncating casts, so assigning a value wider than a byte wraps mod 256. All
//! transcoder targets are little-endian, so `m()` reads the bytes with
//! `from_ne_bytes` (equal to `from_le_bytes` there).

/// 32-bit RGBA color; `c` holds the bytes in r, g, b, a order.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Color32 {
    pub c: [u8; 4],
}

/// Clamp an int to `[0, 255]`.
#[inline]
fn clamp255(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

impl Color32 {
    /// Truncating set: each argument is cast to `u8`, so a value wider than a
    /// byte wraps mod 256.
    #[inline]
    pub fn new(vr: u32, vg: u32, vb: u32, va: u32) -> Self {
        Self {
            c: [vr as u8, vg as u8, vb as u8, va as u8],
        }
    }

    /// Each component clamped into `[0, 255]`.
    #[inline]
    pub fn new_clamped(vr: i32, vg: i32, vb: i32, va: i32) -> Self {
        Self {
            c: [clamp255(vr), clamp255(vg), clamp255(vb), clamp255(va)],
        }
    }

    /// Red channel (`c[0]`).
    #[inline]
    pub fn r(&self) -> u8 {
        self.c[0]
    }
    /// Green channel (`c[1]`).
    #[inline]
    pub fn g(&self) -> u8 {
        self.c[1]
    }
    /// Blue channel (`c[2]`).
    #[inline]
    pub fn b(&self) -> u8 {
        self.c[2]
    }
    /// Alpha channel (`c[3]`).
    #[inline]
    pub fn a(&self) -> u8 {
        self.c[3]
    }

    /// The four color bytes read back as one 32-bit word (native-endian, so
    /// little-endian on every target platform).
    #[inline]
    pub fn m(&self) -> u32 {
        u32::from_ne_bytes(self.c)
    }

    /// Truncating assignment: each argument is cast to `u8`, wrapping mod 256.
    #[inline]
    pub fn set(&mut self, vr: u32, vg: u32, vb: u32, va: u32) {
        self.c = [vr as u8, vg as u8, vb as u8, va as u8];
    }

    /// Truncating assignment of the RGB channels; alpha is left unchanged.
    #[inline]
    pub fn set_noclamp_rgb(&mut self, vr: u32, vg: u32, vb: u32) {
        self.c[0] = vr as u8;
        self.c[1] = vg as u8;
        self.c[2] = vb as u8;
    }

    /// Per-component minimum of two colors.
    #[inline]
    pub fn comp_min(a: Color32, b: Color32) -> Color32 {
        Color32 {
            c: [
                a.c[0].min(b.c[0]),
                a.c[1].min(b.c[1]),
                a.c[2].min(b.c[2]),
                a.c[3].min(b.c[3]),
            ],
        }
    }

    /// Per-component maximum of two colors.
    #[inline]
    pub fn comp_max(a: Color32, b: Color32) -> Color32 {
        Color32 {
            c: [
                a.c[0].max(b.c[0]),
                a.c[1].max(b.c[1]),
                a.c[2].max(b.c[2]),
                a.c[3].max(b.c[3]),
            ],
        }
    }
}

impl core::ops::Index<usize> for Color32 {
    type Output = u8;
    /// Byte access by channel index: 0 = r, 1 = g, 2 = b, 3 = a.
    #[inline]
    fn index(&self, i: usize) -> &u8 {
        &self.c[i]
    }
}

impl core::ops::IndexMut<usize> for Color32 {
    /// Mutable byte access by channel index: 0 = r, 1 = g, 2 = b, 3 = a.
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut u8 {
        &mut self.c[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncating_set_matches_static_cast() {
        // u8 truncation wraps mod 256.
        assert_eq!(Color32::new(256, 257, 0xFFF, 1).c, [0, 1, 255, 1]);
    }

    #[test]
    fn clamped_set_clamps() {
        assert_eq!(
            Color32::new_clamped(-5, 300, 128, 255).c,
            [0, 255, 128, 255]
        );
    }

    #[test]
    fn m_is_native_union_view() {
        let c = Color32::new(0x11, 0x22, 0x33, 0x44);
        assert_eq!(c.m(), u32::from_ne_bytes([0x11, 0x22, 0x33, 0x44]));
    }

    #[test]
    fn comp_min_max() {
        let a = Color32::new(10, 200, 30, 255);
        let b = Color32::new(50, 100, 30, 0);
        assert_eq!(Color32::comp_min(a, b).c, [10, 100, 30, 0]);
        assert_eq!(Color32::comp_max(a, b).c, [50, 200, 30, 255]);
    }
}
