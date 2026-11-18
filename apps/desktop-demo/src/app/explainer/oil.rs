// An oil painting of a page of live composables. The page draws itself in
// key colours, which say where the headline, the small type, a selection
// and the button lie. Two layers turn it into paint: the inner one bakes
// the strokes and their thickness from those colours, the outer one lights
// that relief with a raking lamp.
//
// The renderer keeps the outer layer's surface, the baked painting, while
// the page holds still, so a frame in which only the lamp moves runs only
// the light: the strokes are painted again only when the page changes, as
// when a selection grows.

use std::sync::{Arc, OnceLock};

use cranpose_ui::{
    Color, CompositingStrategy, GraphicsLayer, Modifier, RenderEffect, RuntimeShader,
};

use super::paint_lamp::LampPlace;

// The alpha the type's key colours are drawn at: a selection behind a
// letter shows through it, so the bake knows a selected letter.
const INK: f32 = 0.6;
// The page under everything.
pub(super) const PAGE: Color = Color(0.0, 0.0, 0.0, 1.0);
// The headline's letters.
pub(super) const HEAD_INK: Color = Color(1.0, 0.0, 0.0, INK);
// The headline's orange word.
pub(super) const ACCENT_INK: Color = Color(1.0, 1.0, 0.0, INK);
// The small type and its links.
pub(super) const BODY_INK: Color = Color(0.0, 1.0, 0.0, INK);
// A selection: the bake paints a wet orange stroke over it.
pub(super) const SELECTION: Color = Color(0.0, 0.0, 1.0, 1.0);
// The button's slab and its label.
pub(super) const SLAB: Color = Color(0.2, 0.2, 0.2, 1.0);
pub(super) const LABEL: Color = Color(0.1, 0.1, 0.1, 1.0);

const BAKE_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "oil_bake.wgsl");
const LIGHT_WGSL: &str = cranpose_ui::include_wgsl!("noise.wgsl", "oil_light.wgsl");

// Where things stand on the painted page, in its unit u, and how the
// button looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct OilPage {
    // Logical pixels per u.
    pub unit: f32,
    pub head_em: f32,
    pub body_em: f32,
    // The headline's top and bottom.
    pub head: (f32, f32),
    // The small type's top, bottom and right edge.
    pub body: (f32, f32, f32),
    pub button: ButtonLook,
}

// How the painted button looks: each look is painted anew.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ButtonLook {
    Rest,
    Hovered,
    Pressed,
}

impl ButtonLook {
    fn code(self) -> f32 {
        match self {
            Self::Rest => 0.0,
            Self::Hovered => 1.0,
            Self::Pressed => 2.0,
        }
    }
}

// The inner layer: the page under it, drawn in key colours, painted in oil
// as `page()` says.
pub(super) fn painted(modifier: Modifier, page: impl Fn() -> OilPage + 'static) -> Modifier {
    modifier.graphics_layer(move || {
        let page = page();
        let mut shader = RuntimeShader::from_shared_source(shared(&BAKE, BAKE_WGSL));
        shader.set_float4(0, page.unit, page.head_em, page.body_em, page.button.code());
        shader.set_float4(4, page.head.0, page.head.1, page.body.0, page.body.1);
        shader.set_float(8, page.body.2);
        layer(shader)
    })
}

// The outer layer: the painting under it lit by the lamp at `lamp()`, on
// a page of `unit` logical pixels per u.
pub(super) fn lit(
    modifier: Modifier,
    unit: f32,
    lamp: impl Fn() -> LampPlace + 'static,
) -> Modifier {
    modifier.graphics_layer(move || {
        let lamp = lamp();
        let mut shader = RuntimeShader::from_shared_source(shared(&LIGHT, LIGHT_WGSL));
        shader.set_float4(0, unit, lamp.across, lamp.down, lamp.height);
        layer(shader)
    })
}

static BAKE: OnceLock<Arc<str>> = OnceLock::new();
static LIGHT: OnceLock<Arc<str>> = OnceLock::new();

fn shared(cell: &'static OnceLock<Arc<str>>, wgsl: &str) -> Arc<str> {
    Arc::clone(cell.get_or_init(|| {
        Arc::from(format!(
            "{}{wgsl}",
            cranpose_ui_graphics::RUNTIME_SHADER_PRELUDE_WGSL
        ))
    }))
}

fn layer(shader: RuntimeShader) -> GraphicsLayer {
    GraphicsLayer {
        render_effect: Some(RenderEffect::runtime_shader(shader)),
        compositing_strategy: CompositingStrategy::Offscreen,
        ..GraphicsLayer::default()
    }
}
