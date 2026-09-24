//! Color representation and color space utilities

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self(r, g, b, 1.0)
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self(r, g, b, a)
    }

    pub const fn from_rgba_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    pub const fn from_rgb_u8(r: u8, g: u8, b: u8) -> Self {
        Self::from_rgba_u8(r, g, b, 255)
    }

    pub fn r(&self) -> f32 {
        self.0
    }

    pub fn g(&self) -> f32 {
        self.1
    }

    pub fn b(&self) -> f32 {
        self.2
    }

    pub fn a(&self) -> f32 {
        self.3
    }

    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self(self.0, self.1, self.2, alpha)
    }

    /// This colour as the platform's colour type actually holds it: **eight
    /// bits per channel**.
    ///
    /// `androidx.compose.ui.graphics.Color` is a float-shaped API over an
    /// 8-bit value. Building one in the sRGB space snaps every channel on the
    /// spot — the bytecode of `ColorKt.Color(float, float, float, float,
    /// ColorSpace)` is `coerceIn(0f, 1f)`, `* 255.0f`, `+ 0.5f`, `f2i`, packed
    /// into an ARGB int — so a colour an app computes in float is already a
    /// whole channel value before anything paints with it, and the rasterizer
    /// never sees the fraction.
    ///
    /// A renderer that carries the fraction to the framebuffer instead leaves
    /// the rounding to whatever converts float to unorm there. That agrees
    /// nearly everywhere and disagrees on an exact half. It is not a rare
    /// shape: a theme that lerps between two byte colours lands on one
    /// routinely — `mix(rail, background, 0.55)` puts a Wear settings capsule
    /// at exactly 22.5/255 on green, where this rule gives 23 and a converter
    /// that breaks ties to even gives 22. One level, and then the row's layer
    /// alpha multiplies it and keeps it.
    ///
    /// Ties go **up**, which is what `(int)(x + 0.5f)`, Skia's
    /// `SkScalarRoundToInt` and Rust's `f32::round` all give for a channel
    /// clamped into 0..=1.
    pub fn srgb_8bit(self) -> Self {
        Self(
            srgb_channel_8bit(self.0),
            srgb_channel_8bit(self.1),
            srgb_channel_8bit(self.2),
            srgb_channel_8bit(self.3),
        )
    }

    pub const BLACK: Color = Color(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Color = Color(1.0, 1.0, 1.0, 1.0);
    pub const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
    pub const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);
    pub const BLUE: Color = Color(0.0, 0.0, 1.0, 1.0);
    pub const TRANSPARENT: Color = Color(0.0, 0.0, 0.0, 0.0);
}

/// One channel snapped to the 8-bit value an sRGB colour holds.
fn srgb_channel_8bit(channel: f32) -> f32 {
    (channel.clamp(0.0, 1.0) * 255.0).round() / 255.0
}

#[cfg(test)]
#[path = "tests/color_tests.rs"]
mod tests;
