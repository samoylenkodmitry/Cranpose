//! A layer the sprite compositor's shapes cannot follow, as a turned draw
//! group, reaches it as a texture that holds the layer's pixels.

use cranpose_render_common::{Renderer, render_contract::SharedRenderCase};
use cranpose_render_pixels::{
    PixelsRenderer,
    sprites::{SPRITE_FLOATS, SpriteFrame, SpriteKind, field},
};

/// The alpha the texture sprites of `frame` put at device pixel `x`, `y`.
fn texture_alpha_at(frame: &SpriteFrame, x: f32, y: f32) -> Option<u8> {
    frame
        .sprites()
        .as_chunks::<SPRITE_FLOATS>()
        .0
        .iter()
        .filter(|sprite| sprite[field::KIND] == SpriteKind::Texture as u8 as f32)
        .find_map(|sprite| {
            let id = sprite[field::KIND + 1] as u32;
            let [left, top, width, height] = [0, 1, 2, 3].map(|at| sprite[field::GEOMETRY + at]);
            let inside = x >= left && x < left + width && y >= top && y < top + height;
            let upload = frame.uploads().iter().find(|upload| upload.id == id)?;
            inside.then(|| {
                let column = (x - left) as usize;
                let row = (y - top) as usize;
                upload.pixels[(row * upload.width as usize + column) * 4 + 3]
            })
        })
}

#[test]
fn a_turned_draw_group_reaches_the_sprite_compositor_as_a_texture() {
    let app_context = cranpose_ui::AppContext::new();
    let fixture = SharedRenderCase::DrawScopeGroups
        .fixtures()
        .pop()
        .expect("the draw groups case has a fixture");
    let mut renderer = PixelsRenderer::new();
    renderer.scene_mut().graph = Some(fixture.graph);
    let mut frame = SpriteFrame::default();
    app_context.enter(|| {
        renderer.compose_sprites(&mut frame, fixture.width, fixture.height, 1.0);
    });
    let upright = texture_alpha_at(&frame, 90.5, 8.5);
    assert!(
        upright.is_some_and(|alpha| alpha > 200),
        "the turned bar stands upright in a texture, got {upright:?}"
    );
    let across = texture_alpha_at(&frame, 68.5, 30.5);
    assert!(
        across.is_none_or(|alpha| alpha < 40),
        "the turned bar no longer lies across, got {across:?}"
    );
}
