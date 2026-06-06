use cranpose_ui_graphics::{GraphicsLayer, Rect};

use crate::layer_transform::layer_uniform_scale;

const MIN_LAYER_SHADOW_SCALE: f32 = 0.1;
const MIN_LAYER_SHADOW_SPREAD: f32 = 0.8;
const MIN_AMBIENT_BLUR_RADIUS: f32 = 0.5;
const MIN_SPOT_BLUR_RADIUS: f32 = 0.5;
const AMBIENT_SPREAD_FACTOR: f32 = 0.24;
const SPOT_OFFSET_X_FACTOR: f32 = 0.18;
const SPOT_OFFSET_Y_FACTOR: f32 = 0.62;
const AMBIENT_BLUR_FACTOR: f32 = 0.95;
const SPOT_BLUR_FACTOR: f32 = 0.72;
const SPOT_SPREAD_FACTOR: f32 = 0.72;
const AMBIENT_ALPHA_FACTOR: f32 = 0.72;
const SPOT_ALPHA_FACTOR: f32 = 0.96;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerShadowPass {
    pub rect: Rect,
    pub blur_radius: f32,
    pub alpha: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerShadowGeometry {
    pub ambient: Option<LayerShadowPass>,
    pub spot: Option<LayerShadowPass>,
}

pub fn layer_shadow_geometry(
    layer: &GraphicsLayer,
    transformed_bounds: Rect,
) -> LayerShadowGeometry {
    if layer.shadow_elevation <= 0.0 {
        return LayerShadowGeometry::default();
    }

    let scale = layer_uniform_scale(layer).max(MIN_LAYER_SHADOW_SCALE);
    let elevation = layer.shadow_elevation * scale;
    let spread = (elevation * AMBIENT_SPREAD_FACTOR).max(MIN_LAYER_SHADOW_SPREAD);
    let ambient_alpha = (layer.ambient_shadow_color.a() * AMBIENT_ALPHA_FACTOR).clamp(0.0, 1.0);
    let spot_alpha = (layer.spot_shadow_color.a() * SPOT_ALPHA_FACTOR).clamp(0.0, 1.0);

    let ambient = (ambient_alpha > f32::EPSILON).then_some(LayerShadowPass {
        rect: Rect {
            x: transformed_bounds.x - spread,
            y: transformed_bounds.y - spread,
            width: transformed_bounds.width + spread * 2.0,
            height: transformed_bounds.height + spread * 2.0,
        },
        blur_radius: (elevation * AMBIENT_BLUR_FACTOR).max(MIN_AMBIENT_BLUR_RADIUS),
        alpha: ambient_alpha,
    });

    let spot_spread = spread * SPOT_SPREAD_FACTOR;
    let spot = (spot_alpha > f32::EPSILON).then_some(LayerShadowPass {
        rect: Rect {
            x: transformed_bounds.x + elevation * SPOT_OFFSET_X_FACTOR - spot_spread,
            y: transformed_bounds.y + elevation * SPOT_OFFSET_Y_FACTOR - spot_spread,
            width: transformed_bounds.width + spot_spread * 2.0,
            height: transformed_bounds.height + spot_spread * 2.0,
        },
        blur_radius: (elevation * SPOT_BLUR_FACTOR).max(MIN_SPOT_BLUR_RADIUS),
        alpha: spot_alpha,
    });

    LayerShadowGeometry { ambient, spot }
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::Color;

    use super::*;

    #[test]
    fn layer_shadow_geometry_returns_none_for_zero_elevation() {
        let geometry = layer_shadow_geometry(
            &GraphicsLayer::default(),
            Rect::from_size(Default::default()),
        );
        assert!(geometry.ambient.is_none());
        assert!(geometry.spot.is_none());
    }

    #[test]
    fn layer_shadow_geometry_matches_shadow_model() {
        let layer = GraphicsLayer {
            shadow_elevation: 10.0,
            ambient_shadow_color: Color(0.2, 0.3, 0.4, 0.8),
            spot_shadow_color: Color(0.7, 0.6, 0.5, 0.9),
            ..GraphicsLayer::default()
        };
        let transformed_bounds = Rect {
            x: 20.0,
            y: 30.0,
            width: 40.0,
            height: 12.0,
        };
        let geometry = layer_shadow_geometry(&layer, transformed_bounds);

        let ambient = geometry.ambient.expect("ambient shadow pass");
        assert_eq!(
            ambient.rect,
            Rect {
                x: 17.6,
                y: 27.6,
                width: 44.8,
                height: 16.8,
            }
        );
        assert!((ambient.blur_radius - 9.5).abs() < 1e-6);
        assert!((ambient.alpha - 0.576).abs() < 1e-6);

        let spot = geometry.spot.expect("spot shadow pass");
        assert_eq!(
            spot.rect,
            Rect {
                x: 20.071999,
                y: 34.472,
                width: 43.456,
                height: 15.455999,
            }
        );
        assert!((spot.blur_radius - 7.2).abs() < 1e-6);
        assert!((spot.alpha - 0.864).abs() < 1e-6);
    }
}
