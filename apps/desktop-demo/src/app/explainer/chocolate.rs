// The effect that turns the words and their drips into molten chocolate:
// see `chocolate.wgsl`. The content draws in the key colours of `keys.rs`,
// and its drips in black.

use std::sync::{Arc, OnceLock};

use cranpose_ui::{
    space::{Material, Shaded},
    CompositingStrategy, GraphicsLayer, Point, RenderEffect, RuntimeShader, Size,
};
use cranpose_ui_graphics::{SubstrateSpec, RUNTIME_SHADER_PRELUDE_WGSL};

use super::{
    melting::Melting,
    plaster::{WALL_BOTTOM, WALL_TOP},
};

// Gaussian sigmas in units: the paragraph rounds off and pours on the
// first copy, the headline on the second, and the wide shadow falls from
// the third.
const SIGMAS: [f32; 3] = [1.3, 6.5, 12.0];
const CHOCOLATE_WGSL: &str = include_str!("chocolate.wgsl");
// A scene draws its shaded surfaces after its other ones, far to near. A
// surface over the shaded plaster wall is shaded too, so its translucent
// shadow lies over the wall and not over the empty page.
const AS_DRAWN_WGSL: &str =
    "fn shade(surface: ShadedSurface) -> vec4<f32> {\n    return surface.color;\n}\n";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pour {
    pub size: Size,
    pub offset: Point,
    pub head_scale: f32,
    pub copy_scale: f32,
}

pub(super) fn chocolate_layer(pour: Pour, melting: Melting, density: f32) -> GraphicsLayer {
    let u = melting.u;
    let mut shader = RuntimeShader::from_shared_source(source());
    shader.set_substrates(&SIGMAS.map(|sigma| SubstrateSpec::Blur {
        radius_px: 2.0 * sigma * u * density,
    }));
    let warmth = melting.warmth();
    let values = [
        (0, pour.size.width),
        (1, pour.size.height),
        (2, u),
        (3, melting.softness()),
        (4, melting.swelling()),
        (5, melting.heat()),
        (6, warmth.at.x - pour.offset.x),
        (7, warmth.at.y - pour.offset.y),
        (8, warmth.strength),
        (9, pour.head_scale),
        (10, pour.copy_scale),
        (11, density),
        (12, SIGMAS[0] * u),
        (13, SIGMAS[1] * u),
        (14, SIGMAS[2] * u),
        (15, melting.split - pour.offset.y),
        (24, pour.offset.x),
        (25, pour.offset.y),
        (26, melting.size.width),
        (27, melting.size.height),
    ];
    for (index, value) in values {
        shader.set_float(index, value);
    }
    for (first, [r, g, b]) in [(16, WALL_TOP), (20, WALL_BOTTOM)] {
        shader.set_float4(first, r, g, b, 1.0);
    }
    GraphicsLayer {
        render_effect: Some(RenderEffect::runtime_shader(shader)),
        compositing_strategy: CompositingStrategy::Offscreen,
        ..GraphicsLayer::default()
    }
}

pub(super) fn poured() -> Material {
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    Material::Shaded(Shaded {
        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(AS_DRAWN_WGSL))),
        params: [0.0; Shaded::PARAMS],
    })
}

fn source() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(
        SOURCE.get_or_init(|| Arc::from(format!("{RUNTIME_SHADER_PRELUDE_WGSL}{CHOCOLATE_WGSL}"))),
    )
}
