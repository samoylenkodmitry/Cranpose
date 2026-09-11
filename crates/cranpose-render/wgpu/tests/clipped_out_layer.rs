mod support;

use cranpose_ui_graphics::{
    GraphicsLayer, Point, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader,
};
use support::clip_band::{self, FRAME};

/// Where a child sits when it is on show: inside the band.
const SHOWN: Point = Point { x: 40.0, y: 10.0 };
/// Where a child sits when the list has scrolled it out: wholly above the
/// band's top, with 60 of its rows still inside the frame.
const SCROLLED_OUT: Point = Point { x: 40.0, y: -140.0 };

/// A backdrop shader that paints red wherever it is composited, so the
/// composite's footprint is what the frame shows.
fn red_backdrop() -> RenderEffect {
    RenderEffect::runtime_shader(RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(0.86, 0.12, 0.12, 1.0);
         }}"
    )))
}

/// A glass control the way the liquid widgets build one: a backdrop effect
/// under a layer that clips to its own bounds.
fn glass() -> GraphicsLayer {
    GraphicsLayer {
        clip: true,
        backdrop_effect: Some(red_backdrop()),
        ..GraphicsLayer::default()
    }
}

/// A glass control the list has scrolled wholly past its edge paints
/// nothing, the way its neighbours' fills do once they leave the list.
///
/// The control's own clip lies outside the list's, and a clip that meets
/// nothing must stay a clip: the frame's top rows are the tab bar above
/// the list, and a control drawn there is the bug.
#[test]
fn a_glass_control_scrolled_out_of_its_list_paints_nothing() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping clipped out layer: {err}");
            return;
        }
    };
    let shown = support::capture_graph(
        &mut renderer,
        clip_band::page(glass(), SHOWN, vec![]),
        FRAME,
        FRAME,
    );
    let (_, inside) = clip_band::red_pixels(&shown.pixels);
    assert!(
        inside > 1000,
        "the glass control must paint inside the band when it is on show for this to test \
         anything, painted {inside} pixels"
    );

    let scrolled = support::capture_graph(
        &mut renderer,
        clip_band::page(glass(), SCROLLED_OUT, vec![]),
        FRAME,
        FRAME,
    );
    let (above, inside) = clip_band::red_pixels(&scrolled.pixels);
    assert_eq!(
        (above, inside),
        (0, 0),
        "a glass control scrolled out of its list painted {above} pixels above the list and \
         {inside} inside it"
    );
}
