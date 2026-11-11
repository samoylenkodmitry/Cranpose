// The window behind the roll: a night seen through wet glass, for a piece
// named after moonlight. A shader draws it: the fogged glass, the moon out
// of focus behind it, the drops that sit on it and the rivulets that run
// down it. The window moves a little with the clock of the performance
// around it: drops run down, and light flows along the rivulets. A piece
// that stands still leaves the window still.

use std::sync::{Arc, OnceLock};

use cranpose_ui::{
    composable, Box as CranposeBox, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, Color,
    GraphicsLayer, Modifier,
};
use cranpose_ui_graphics::{
    CompositingStrategy, RenderEffect, RuntimeShader, RUNTIME_SHADER_PRELUDE_WGSL,
};

use super::{performance::local_performance, song::note_seconds};

// Where the moon stands, as shares of the window's width and height.
pub(super) const MOON_PLACE: (f32, f32) = (0.86, 0.62);
// What the window shows until its shader is ready.
const NIGHT: Color = Color::from_rgb_u8(0x0A, 0x12, 0x2E);

fn window_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(SOURCE.get_or_init(|| {
        Arc::from(format!(
            "{RUNTIME_SHADER_PRELUDE_WGSL}{}",
            include_str!("rain_window.wgsl")
        ))
    }))
}

// Fills its place with the night behind wet glass.
#[composable]
pub(super) fn NightSky(modifier: Modifier) {
    let performance = local_performance().current();
    BoxWithConstraints(modifier, move |scope| {
        let performance = performance.clone();
        let width = scope.constraints().max_width;
        let height = scope.constraints().max_height;
        CranposeBox(
            Modifier::empty()
                .fill_max_size()
                .background(NIGHT)
                .graphics_layer(move || {
                    let mut shader = RuntimeShader::from_shared_source(window_wgsl());
                    shader.set_position_independent(true);
                    shader.set_float2(0, width, height);
                    shader.set_float2(2, width * MOON_PLACE.0, height * MOON_PLACE.1);
                    // The window draws again while the clock moves.
                    let seconds = performance
                        .as_ref()
                        .map_or(0.0, |performance| note_seconds(performance.seconds()));
                    shader.set_float(4, seconds);
                    GraphicsLayer {
                        render_effect: Some(RenderEffect::runtime_shader(shader)),
                        compositing_strategy: CompositingStrategy::Offscreen,
                        ..GraphicsLayer::default()
                    }
                }),
            BoxSpec::default(),
            || {},
        );
    });
}
