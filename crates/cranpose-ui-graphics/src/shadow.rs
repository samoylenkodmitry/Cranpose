//! Shadow configuration models used by drop/inner shadow modifiers.

use crate::{BlendMode, Brush, Color, Dp, Point};

/// Density-independent offset for shadows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DpOffset {
    pub x: Dp,
    pub y: Dp,
}

impl DpOffset {
    pub const fn new(x: Dp, y: Dp) -> Self {
        Self { x, y }
    }

    pub const ZERO: DpOffset = DpOffset {
        x: Dp(0.0),
        y: Dp(0.0),
    };

    pub fn to_px(self, density: f32) -> Point {
        Point::new(self.x.to_px(density), self.y.to_px(density))
    }
}

impl Default for DpOffset {
    fn default() -> Self {
        Self::ZERO
    }
}

/// Static shadow configuration.
///
/// This mirrors Compose's static `Shadow` object where radius/spread/offset
/// are provided in density-independent units and converted to pixels at draw time.
#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub radius: Dp,
    pub spread: Dp,
    pub offset: DpOffset,
    pub color: Color,
    pub brush: Option<Brush>,
    pub alpha: f32,
    pub blend_mode: BlendMode,
    /// Knocks the unoffset, unspread shape out of the shadow silhouette
    /// before blurring, leaving shadow only OUTSIDE the element. Required
    /// for translucent surfaces (glass): a backdrop-sampling material must
    /// not refract its own shadow.
    pub cutout: bool,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            radius: Dp(0.0),
            spread: Dp(0.0),
            offset: DpOffset::ZERO,
            color: Color::BLACK,
            brush: None,
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            cutout: false,
        }
    }
}

impl Shadow {
    pub fn to_scope(&self, density: f32) -> ShadowScope {
        ShadowScope {
            radius: self.radius.to_px(density),
            spread: self.spread.to_px(density),
            offset: self.offset.to_px(density),
            color: self.color,
            brush: self.brush.clone(),
            alpha: self.alpha,
            blend_mode: self.blend_mode,
            cutout: self.cutout,
        }
    }
}

/// Pixel-space shadow scope used by block-based APIs.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadowScope {
    pub radius: f32,
    pub spread: f32,
    pub offset: Point,
    pub color: Color,
    pub brush: Option<Brush>,
    pub alpha: f32,
    pub blend_mode: BlendMode,
    /// See [`Shadow::cutout`]: knock the element's own shape out of the
    /// silhouette so the shadow exists only outside it.
    pub cutout: bool,
}

impl Default for ShadowScope {
    fn default() -> Self {
        Self {
            radius: 0.0,
            spread: 0.0,
            offset: Point::ZERO,
            color: Color::BLACK,
            brush: None,
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            cutout: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dp_offset_converts_to_px() {
        let offset = DpOffset::new(Dp(4.0), Dp(-2.5));
        let px = offset.to_px(2.0);
        assert_eq!(px, Point::new(8.0, -5.0));
    }

    #[test]
    fn shadow_default_matches_black_src_over() {
        let shadow = Shadow::default();
        assert_eq!(shadow.radius, Dp(0.0));
        assert_eq!(shadow.spread, Dp(0.0));
        assert_eq!(shadow.offset, DpOffset::ZERO);
        assert_eq!(shadow.color, Color::BLACK);
        assert_eq!(shadow.brush, None);
        assert!((shadow.alpha - 1.0).abs() < 1e-6);
        assert_eq!(shadow.blend_mode, BlendMode::SrcOver);
    }

    #[test]
    fn shadow_to_scope_uses_density() {
        let shadow = Shadow {
            radius: Dp(5.0),
            spread: Dp(2.0),
            offset: DpOffset::new(Dp(-1.0), Dp(3.0)),
            color: Color::from_rgba_u8(10, 20, 30, 255),
            brush: None,
            alpha: 0.7,
            blend_mode: BlendMode::Overlay,
            cutout: false,
        };
        let scope = shadow.to_scope(2.0);
        assert!((scope.radius - 10.0).abs() < 1e-6);
        assert!((scope.spread - 4.0).abs() < 1e-6);
        assert_eq!(scope.offset, Point::new(-2.0, 6.0));
        assert_eq!(scope.color, Color::from_rgba_u8(10, 20, 30, 255));
        assert!((scope.alpha - 0.7).abs() < 1e-6);
        assert_eq!(scope.blend_mode, BlendMode::Overlay);
    }
}
