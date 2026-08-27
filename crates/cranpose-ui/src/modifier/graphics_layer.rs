use std::rc::Rc;

use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, CompositingStrategy, Dp, GradientBlurDirection,
    GradientCutMaskSpec, GradientFadeMaskSpec, LayerShape, RenderEffect, RoundedCornerShape,
    RuntimeShader, TransformOrigin, gradient_blur_effect, gradient_cut_mask_effect,
    gradient_fade_dst_out_effect, rounded_alpha_mask_effect,
};

use super::{GraphicsLayer, Modifier, inspector_metadata};
use crate::modifier_nodes::{GraphicsLayerElement, LazyGraphicsLayerElement};

fn backdrop_blur_layer(radius: Dp, shape: LayerShape) -> GraphicsLayer {
    let radius_px = radius
        .to_px(crate::render_state::current_density())
        .max(0.0);
    GraphicsLayer {
        backdrop_effect: (radius_px > 0.0).then(|| RenderEffect::blur(radius_px)),
        shape,
        clip: true,
        ..Default::default()
    }
}

fn backdrop_gradient_blur_layer(
    start_radius: Dp,
    end_radius: Dp,
    direction: GradientBlurDirection,
) -> GraphicsLayer {
    let density = crate::render_state::current_density();
    let start_px = start_radius.to_px(density).max(0.0);
    let end_px = end_radius.to_px(density).max(0.0);
    GraphicsLayer {
        backdrop_effect: (start_px > 0.0 || end_px > 0.0)
            .then(|| gradient_blur_effect(start_px, end_px, direction)),
        shape: LayerShape::Rectangle,
        clip: true,
        ..Default::default()
    }
}

impl Modifier {
    /// Apply a lazily evaluated graphics layer.
    ///
    /// The closure is evaluated during scene building, not composition, which lets
    /// layer properties update without forcing recomposition.
    ///
    /// Example:
    /// `Modifier::empty().graphics_layer(|| GraphicsLayer { alpha: 0.5, ..Default::default() })`
    ///
    /// # A drag that drives the translation must not be read inside it
    ///
    /// [`PointerEvent::position`](crate::modifier::PointerEvent::position) is
    /// in the receiving node's own space, so a node this layer translates
    /// reports a finger that has moved only as far as it OUTRAN the
    /// translation. Feed that position back into `translation_x` and the
    /// gesture measures itself:
    ///
    /// ```text
    /// reported = finger - offset          offset = reported - start
    ///         => offset = (finger - start) / 2
    /// ```
    ///
    /// The offset converges on half the finger's travel and can never exceed
    /// it, so a swipe-to-dismiss whose threshold is half the width never
    /// crosses it however far the finger goes -- silently, with the content
    /// sliding convincingly the whole time.
    ///
    /// Two ways out, and [`SwipeToDismiss`](crate::widgets::SwipeToDismiss)
    /// uses both:
    ///
    /// - put `pointer_input` on a node OUTSIDE the translated one, which is
    ///   also how Compose's own `SwipeToDismissBox` is built -- the drag is
    ///   detected on the box, the content carries the layer;
    /// - or read
    ///   [`PointerEvent::global_position`](crate::modifier::PointerEvent::global_position),
    ///   which no layer transform touches.
    pub fn graphics_layer(self, layer: impl Fn() -> GraphicsLayer + 'static) -> Self {
        let modifier = Self::with_element(LazyGraphicsLayerElement::new(Rc::new(layer)))
            .with_inspector_metadata(inspector_metadata("graphicsLayer", |info| {
                info.add_property("lazy", "true");
            }));
        self.then(modifier)
    }

    /// Apply a concrete graphics layer snapshot.
    ///
    /// This is useful for parameter-style wrappers that already build a fixed
    /// [`GraphicsLayer`] value.
    pub fn graphics_layer_value(self, layer: GraphicsLayer) -> Self {
        let inspector_values = layer.clone();
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("graphicsLayer", move |info| {
                info.add_property("alpha", inspector_values.alpha.to_string());
                info.add_property("scale", inspector_values.scale.to_string());
                info.add_property("scaleX", inspector_values.scale_x.to_string());
                info.add_property("scaleY", inspector_values.scale_y.to_string());
                info.add_property("rotationX", inspector_values.rotation_x.to_string());
                info.add_property("rotationY", inspector_values.rotation_y.to_string());
                info.add_property("rotationZ", inspector_values.rotation_z.to_string());
                info.add_property(
                    "cameraDistance",
                    inspector_values.camera_distance.to_string(),
                );
                info.add_property(
                    "transformOrigin",
                    format!(
                        "{},{}",
                        inspector_values.transform_origin.pivot_fraction_x,
                        inspector_values.transform_origin.pivot_fraction_y
                    ),
                );
                info.add_property("translationX", inspector_values.translation_x.to_string());
                info.add_property("translationY", inspector_values.translation_y.to_string());
                info.add_property(
                    "shadowElevation",
                    inspector_values.shadow_elevation.to_string(),
                );
                info.add_property("shape", format!("{:?}", inspector_values.shape));
                info.add_property("clip", inspector_values.clip.to_string());
                info.add_property(
                    "ambientShadowColor",
                    format!("{:?}", inspector_values.ambient_shadow_color),
                );
                info.add_property(
                    "spotShadowColor",
                    format!("{:?}", inspector_values.spot_shadow_color),
                );
                info.add_property(
                    "compositingStrategy",
                    format!("{:?}", inspector_values.compositing_strategy),
                );
                info.add_property("blendMode", format!("{:?}", inspector_values.blend_mode));
                if let Some(filter) = inspector_values.color_filter {
                    info.add_property("colorFilter", format!("{filter:?}"));
                }
            }));
        self.then(modifier)
    }

    /// Compose-compatible parameter-style graphics layer entry point.
    ///
    /// This mirrors `Modifier.graphicsLayer(...)` style APIs and maps directly to
    /// [`GraphicsLayer`] fields currently implemented by the renderer stack.
    #[allow(clippy::too_many_arguments)]
    pub fn graphics_layer_params(
        self,
        scale_x: f32,
        scale_y: f32,
        alpha: f32,
        translation_x: f32,
        translation_y: f32,
        shadow_elevation: f32,
        rotation_x: f32,
        rotation_y: f32,
        rotation_z: f32,
        camera_distance: f32,
        transform_origin: TransformOrigin,
        shape: LayerShape,
        clip: bool,
        render_effect: Option<RenderEffect>,
        ambient_shadow_color: Color,
        spot_shadow_color: Color,
        compositing_strategy: CompositingStrategy,
        blend_mode: BlendMode,
        color_filter: Option<ColorFilter>,
    ) -> Self {
        self.graphics_layer_value(GraphicsLayer {
            alpha,
            scale: 1.0,
            scale_x,
            scale_y,
            rotation_x,
            rotation_y,
            rotation_z,
            camera_distance,
            transform_origin,
            translation_x,
            translation_y,
            shadow_elevation,
            ambient_shadow_color,
            spot_shadow_color,
            shape,
            clip,
            compositing_strategy,
            blend_mode,
            color_filter,
            render_effect,
            backdrop_effect: None,
        })
    }

    /// Compose-compatible block-style graphics layer entry point.
    ///
    /// Example:
    /// `Modifier::empty().graphics_layer_block(|layer| { layer.alpha = 0.5; layer.scale_x = 1.2; })`
    pub fn graphics_layer_block(self, configure: impl Fn(&mut GraphicsLayer) + 'static) -> Self {
        self.graphics_layer(move || {
            let mut layer = GraphicsLayer::default();
            configure(&mut layer);
            layer
        })
    }

    /// Compose-style elevation shadow convenience.
    ///
    /// This mirrors `Modifier.shadow(elevation)` defaults:
    /// rectangle shape, black ambient/spot colors, and clipping enabled when
    /// elevation is positive.
    pub fn shadow(self, elevation: f32) -> Self {
        self.shadow_with(
            elevation,
            LayerShape::Rectangle,
            elevation > 0.0,
            Color::BLACK,
            Color::BLACK,
        )
    }

    /// Compose-style shadow API with explicit shape/clip/colors.
    pub fn shadow_with(
        self,
        elevation: f32,
        shape: LayerShape,
        clip: bool,
        ambient_color: Color,
        spot_color: Color,
    ) -> Self {
        let clamped_elevation = elevation.max(0.0);
        if clamped_elevation == 0.0 && !clip {
            return self;
        }

        self.graphics_layer_value(GraphicsLayer {
            shadow_elevation: clamped_elevation,
            ambient_shadow_color: ambient_color,
            spot_shadow_color: spot_color,
            shape,
            clip,
            ..Default::default()
        })
    }

    /// Apply a backdrop effect to content behind this composable's bounds.
    pub fn backdrop_effect(self, effect: RenderEffect) -> Self {
        let layer = GraphicsLayer {
            backdrop_effect: Some(effect),
            ..Default::default()
        };
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("backdropEffect", |info| {
                info.add_property("enabled", "true");
            }));
        self.then(modifier)
    }

    /// Blur content behind this composable, clipped to the composable bounds.
    ///
    /// `radius` is expressed in Dp and converted to px using the current render
    /// density when modifier slices are evaluated.
    pub fn backdrop_blur(self, radius: Dp) -> Self {
        if radius.0 <= 0.0 {
            return self.clip_to_bounds();
        }

        let modifier = Self::with_element(LazyGraphicsLayerElement::new(Rc::new(move || {
            backdrop_blur_layer(radius, LayerShape::Rectangle)
        })))
        .with_inspector_metadata(inspector_metadata("backdropBlur", move |info| {
            info.add_property("radius", radius.0.to_string());
        }));
        self.then(modifier)
    }

    /// Blur content behind this composable with a radius that changes across
    /// its bounds. This is a true spatial blur gradient: the sampling kernel
    /// interpolates from `start_radius` to `end_radius`; it is not an opacity
    /// gradient over a uniformly blurred layer.
    pub fn backdrop_gradient_blur(
        self,
        start_radius: Dp,
        end_radius: Dp,
        direction: GradientBlurDirection,
    ) -> Self {
        if start_radius.0 <= 0.0 && end_radius.0 <= 0.0 {
            return self.clip_to_bounds();
        }
        let modifier = Self::with_element(LazyGraphicsLayerElement::new(Rc::new(move || {
            backdrop_gradient_blur_layer(start_radius, end_radius, direction)
        })))
        .with_inspector_metadata(inspector_metadata(
            "backdropGradientBlur",
            move |info| {
                info.add_property("startRadius", start_radius.0.to_string());
                info.add_property("endRadius", end_radius.0.to_string());
                info.add_property("direction", format!("{direction:?}"));
            },
        ));
        self.then(modifier)
    }

    /// Apply a frosted glass material: backdrop blur, clipped shape, and tint.
    pub fn glass_material(self, material: GlassMaterial) -> Self {
        let blur_radius = material.blur_radius;
        let shape = material.shape;
        let modifier = Self::with_element(LazyGraphicsLayerElement::new(Rc::new(move || {
            backdrop_blur_layer(blur_radius, LayerShape::Rounded(shape))
        })))
        .with_inspector_metadata(inspector_metadata("glassMaterial", move |info| {
            info.add_property("blurRadius", blur_radius.0.to_string());
            info.add_property("shape", format!("{shape:?}"));
        }));

        self.then(modifier)
            .rounded_corner_shape(material.shape)
            .background(material.tint)
    }

    /// Convenience alias for applying a backdrop shader effect.
    pub fn shader_background(self, shader: RuntimeShader) -> Self {
        self.backdrop_effect(RenderEffect::runtime_shader(shader))
    }

    /// Apply a color filter to this composable's graphics layer output.
    ///
    /// This mirrors Compose's `graphicsLayer(colorFilter = ...)` capability.
    pub fn color_filter(self, filter: ColorFilter) -> Self {
        let layer = GraphicsLayer {
            color_filter: Some(filter),
            ..Default::default()
        };
        let modifier = Self::with_element(GraphicsLayerElement::new(layer))
            .with_inspector_metadata(inspector_metadata("colorFilter", |info| {
                info.add_property("enabled", "true");
            }));
        self.then(modifier)
    }

    /// Convenience color filter that tints layer output.
    pub fn tint(self, tint: Color) -> Self {
        self.color_filter(ColorFilter::tint(tint))
    }

    /// Configures how the layer is composited into its parent.
    pub fn compositing_strategy(self, strategy: CompositingStrategy) -> Self {
        self.graphics_layer_value(GraphicsLayer {
            compositing_strategy: strategy,
            ..Default::default()
        })
    }

    /// Configures blend mode for this layer output.
    ///
    /// Runtime support is backend-dependent. Current renderers fully support
    /// `SrcOver` and `DstOut`; unsupported modes fall back to `SrcOver`.
    pub fn layer_blend_mode(self, blend_mode: BlendMode) -> Self {
        self.graphics_layer_value(GraphicsLayer {
            blend_mode,
            ..Default::default()
        })
    }

    /// Apply a directional gradient cut mask to this composable output.
    ///
    /// This masks the rendered layer with rounded corners and a feathered edge.
    pub fn gradient_cut_mask(
        self,
        area_width: f32,
        area_height: f32,
        spec: GradientCutMaskSpec,
    ) -> Self {
        let layer = GraphicsLayer {
            render_effect: Some(gradient_cut_mask_effect(&spec, area_width, area_height)),
            ..Default::default()
        };
        self.graphics_layer_value(layer)
    }

    /// Apply a rounded alpha mask to this composable output.
    ///
    /// Useful to constrain a preceding render effect (for example blur) to a
    /// rounded shape while preserving a soft edge transition.
    pub fn rounded_alpha_mask(
        self,
        area_width: f32,
        area_height: f32,
        corner_radius: f32,
        edge_feather: f32,
    ) -> Self {
        let has_radius = corner_radius.is_finite() && corner_radius > 0.0;
        let has_feather = edge_feather.is_finite() && edge_feather > 0.0;
        if !has_radius && !has_feather {
            return self;
        }
        let layer = GraphicsLayer {
            render_effect: Some(rounded_alpha_mask_effect(
                area_width,
                area_height,
                corner_radius,
                edge_feather,
            )),
            ..Default::default()
        };
        self.graphics_layer_value(layer)
    }

    /// Apply a directional gradient fade mask with destination-out semantics.
    ///
    /// This mirrors Compose's `drawWithContent + drawRect(..., BlendMode.DstOut)`
    /// pattern for fading content to transparent along one axis.
    pub fn gradient_fade_dst_out(
        self,
        area_width: f32,
        area_height: f32,
        spec: GradientFadeMaskSpec,
    ) -> Self {
        let layer = GraphicsLayer {
            render_effect: Some(gradient_fade_dst_out_effect(&spec, area_width, area_height)),
            ..Default::default()
        };
        self.graphics_layer_value(layer)
    }
}

/// Visual parameters for [`Modifier::glass_material`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassMaterial {
    pub blur_radius: Dp,
    pub tint: Color,
    pub shape: RoundedCornerShape,
}

impl GlassMaterial {
    pub fn new(blur_radius: Dp, tint: Color, shape: RoundedCornerShape) -> Self {
        Self {
            blur_radius,
            tint,
            shape,
        }
    }
}
