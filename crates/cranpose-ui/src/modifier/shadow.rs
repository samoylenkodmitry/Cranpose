use std::rc::Rc;

use cranpose_ui_graphics::{
    Density, DrawPrimitive, DrawScope as _, DrawScopeDefault, ShadowPrimitive,
};

use super::{
    Brush, Color, DrawCommand, LayerShape, Modifier, Point, Rect, Shadow, ShadowScope, Size,
    inspector_metadata,
};
use crate::modifier_nodes::DrawCommandElement;

impl Modifier {
    /// Draws a drop shadow behind the current content.
    ///
    /// This mirrors Compose 1.9's `dropShadow(shape) { ... }`.
    ///
    /// Backend note: the `pixels` renderer currently draws the shadow geometry
    /// without Gaussian blur; `wgpu` applies the requested blur radius.
    pub fn drop_shadow(
        self,
        shape: LayerShape,
        block: impl Fn(&mut ShadowScope) + 'static,
    ) -> Self {
        let block = Rc::new(block);
        let draw = Rc::new(move |scope: &mut DrawScopeDefault| {
            let mut shadow = ShadowScope::default();
            block(&mut shadow);
            let primitives = build_drop_shadow_primitives(scope.size(), shape, &shadow);
            scope.push_recorded(primitives);
        });
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Behind(draw)))
            .with_inspector_metadata(inspector_metadata("dropShadow", move |info| {
                info.add_property("shape", format!("{shape:?}"));
                info.add_property("shadowKind", "block");
            }));
        self.then(modifier)
    }

    /// Static shadow configuration variant mirroring Compose's `dropShadow(shape, shadow)`.
    pub fn drop_shadow_value(self, shape: LayerShape, shadow: Shadow) -> Self {
        let shadow_value = shadow.clone();
        let draw = Rc::new(move |scope: &mut DrawScopeDefault| {
            let shadow =
                shadow_value.to_scope(Density::from_scale(crate::render_state::current_density()));
            let primitives = build_drop_shadow_primitives(scope.size(), shape, &shadow);
            scope.push_recorded(primitives);
        });
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Behind(draw)))
            .with_inspector_metadata(inspector_metadata("dropShadow", move |info| {
                info.add_property("shape", format!("{shape:?}"));
                info.add_property("shadowKind", "static");
            }));
        self.then(modifier)
    }

    /// Draws an inner shadow on top of current content.
    ///
    /// This mirrors Compose 1.9's `innerShadow(shape) { ... }`.
    ///
    /// Backend note: the `pixels` renderer currently draws the shadow geometry
    /// without Gaussian blur; `wgpu` applies the requested blur radius.
    pub fn inner_shadow(
        self,
        shape: LayerShape,
        block: impl Fn(&mut ShadowScope) + 'static,
    ) -> Self {
        let block = Rc::new(block);
        let draw = Rc::new(move |scope: &mut DrawScopeDefault| {
            let mut shadow = ShadowScope::default();
            block(&mut shadow);
            let primitives = build_inner_shadow_primitives(scope.size(), shape, &shadow);
            scope.push_recorded(primitives);
        });
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Overlay(draw)))
            .with_inspector_metadata(inspector_metadata("innerShadow", move |info| {
                info.add_property("shape", format!("{shape:?}"));
                info.add_property("shadowKind", "block");
            }));
        self.then(modifier)
    }

    /// Static shadow configuration variant mirroring Compose's `innerShadow(shape, shadow)`.
    pub fn inner_shadow_value(self, shape: LayerShape, shadow: Shadow) -> Self {
        let shadow_value = shadow.clone();
        let draw = Rc::new(move |scope: &mut DrawScopeDefault| {
            let shadow =
                shadow_value.to_scope(Density::from_scale(crate::render_state::current_density()));
            let primitives = build_inner_shadow_primitives(scope.size(), shape, &shadow);
            scope.push_recorded(primitives);
        });
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Overlay(draw)))
            .with_inspector_metadata(inspector_metadata("innerShadow", move |info| {
                info.add_property("shape", format!("{shape:?}"));
                info.add_property("shadowKind", "static");
            }));
        self.then(modifier)
    }
}

fn normalized_scope(scope: &ShadowScope) -> Option<ShadowScope> {
    if !scope.alpha.is_finite() || scope.alpha <= 0.0 {
        return None;
    }
    let radius = if scope.radius.is_finite() {
        scope.radius.max(0.0)
    } else {
        0.0
    };
    let spread = if scope.spread.is_finite() {
        scope.spread
    } else {
        0.0
    };
    let offset = Point {
        x: if scope.offset.x.is_finite() {
            scope.offset.x
        } else {
            0.0
        },
        y: if scope.offset.y.is_finite() {
            scope.offset.y
        } else {
            0.0
        },
    };
    Some(ShadowScope {
        radius,
        spread,
        offset,
        color: scope.color,
        brush: scope.brush.clone(),
        alpha: scope.alpha.clamp(0.0, 1.0),
        blend_mode: scope.blend_mode,
        cutout: scope.cutout,
    })
}

fn build_drop_shadow_primitives(
    size: Size,
    shape: LayerShape,
    scope: &ShadowScope,
) -> Vec<DrawPrimitive> {
    let Some(scope) = normalized_scope(scope) else {
        return Vec::new();
    };
    if size.width <= 0.0 || size.height <= 0.0 {
        return Vec::new();
    }

    let brush = alpha_modulated_brush(
        scope.brush.unwrap_or_else(|| Brush::solid(scope.color)),
        scope.alpha,
    );

    let spread = scope.spread;
    let rect = Rect {
        x: scope.offset.x - spread,
        y: scope.offset.y - spread,
        width: size.width + spread * 2.0,
        height: size.height + spread * 2.0,
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Vec::new();
    }

    let Some(shape_prim) = primitive_for_shape(shape, rect, brush) else {
        return Vec::new();
    };

    let cutout = if scope.cutout {
        let element_rect = Rect {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        };
        primitive_for_shape(shape, element_rect, Brush::solid(Color::BLACK)).map(Box::new)
    } else {
        None
    };

    vec![DrawPrimitive::Shadow(ShadowPrimitive::Drop {
        shape: Box::new(shape_prim),
        cutout,
        blur_radius: scope.radius,
        blend_mode: scope.blend_mode,
    })]
}

fn build_inner_shadow_primitives(
    size: Size,
    shape: LayerShape,
    scope: &ShadowScope,
) -> Vec<DrawPrimitive> {
    let Some(scope) = normalized_scope(scope) else {
        return Vec::new();
    };
    if size.width <= 0.0 || size.height <= 0.0 {
        return Vec::new();
    }
    if scope.radius <= f32::EPSILON
        && scope.spread.abs() <= f32::EPSILON
        && scope.offset.x.abs() <= f32::EPSILON
        && scope.offset.y.abs() <= f32::EPSILON
    {
        return Vec::new();
    }

    let brush = alpha_modulated_brush(
        scope.brush.unwrap_or_else(|| Brush::solid(scope.color)),
        scope.alpha,
    );

    let outer = Rect {
        x: 0.0,
        y: 0.0,
        width: size.width,
        height: size.height,
    };
    let left = scope.offset.x + scope.spread;
    let top = scope.offset.y + scope.spread;
    let right = (scope.offset.x + size.width - scope.spread).max(left);
    let bottom = (scope.offset.y + size.height - scope.spread).max(top);
    let inner = Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    };
    if inner.width <= 0.0 || inner.height <= 0.0 {
        return Vec::new();
    }

    let Some(fill) = primitive_for_shape(shape, outer, brush) else {
        return Vec::new();
    };
    let Some(cutout) = primitive_for_shape(shape, inner, Brush::solid(Color::WHITE)) else {
        return Vec::new();
    };

    vec![DrawPrimitive::Shadow(ShadowPrimitive::Inner {
        fill: Box::new(fill),
        cutout: Box::new(cutout),
        blur_radius: scope.radius,
        blend_mode: scope.blend_mode,
        clip_rect: outer,
    })]
}

fn primitive_for_shape(shape: LayerShape, rect: Rect, brush: Brush) -> Option<DrawPrimitive> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }

    Some(match shape {
        LayerShape::Rectangle => DrawPrimitive::Rect {
            rect,
            brush,
            stroke: None,
        },
        LayerShape::Rounded(shape) => {
            let radii = shape.resolve(rect.width, rect.height);
            DrawPrimitive::RoundRect {
                rect,
                brush,
                radii,
                stroke: None,
            }
        }
    })
}

fn alpha_modulated_brush(brush: Brush, alpha: f32) -> Brush {
    let alpha = alpha.clamp(0.0, 1.0);
    match brush {
        Brush::Solid(color) => Brush::Solid(color.with_alpha(color.a() * alpha)),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => Brush::LinearGradient {
            colors: colors
                .into_iter()
                .map(|color| color.with_alpha(color.a() * alpha))
                .collect(),
            stops,
            start,
            end,
            tile_mode,
        },
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => Brush::RadialGradient {
            colors: colors
                .into_iter()
                .map(|color| color.with_alpha(color.a() * alpha))
                .collect(),
            stops,
            center,
            radius,
            tile_mode,
        },
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => Brush::SweepGradient {
            colors: colors
                .into_iter()
                .map(|color| color.with_alpha(color.a() * alpha))
                .collect(),
            stops,
            center,
        },
    }
}
