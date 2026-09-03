mod support;

use support::page::*;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 240;
const CARD: [f32; 4] = [24.0, 40.0, 272.0, 120.0];
const BUTTON: [f32; 4] = [236.0, 80.0, 40.0, 40.0];

#[composable]
#[allow(non_snake_case)]
fn GradientPage() {
    FramePage(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color(0.08, 0.12, 0.30, 1.0),
        || {
            Box(
                rect_modifier([0.0, 0.0, 160.0, 240.0]).background(Color(0.55, 0.20, 0.12, 1.0)),
                BoxSpec::new(),
                || {},
            );
            Box(
                rect_modifier(CARD)
                    .backdrop_effect(RenderEffect::blur(8.0))
                    .background(Color(1.0, 1.0, 1.0, 0.18))
                    .rounded_corners(14.0),
                BoxSpec::new(),
                || {
                    Text(
                        "Underlay bake",
                        Modifier::empty().offset(16.0, 16.0),
                        TextStyle::default(),
                    );
                    Box(
                        rect_modifier([
                            BUTTON[0] - CARD[0],
                            BUTTON[1] - CARD[1],
                            BUTTON[2],
                            BUTTON[3],
                        ])
                        .backdrop_effect(RenderEffect::blur(5.0))
                        .background(Color(1.0, 1.0, 1.0, 0.24))
                        .rounded_corners(20.0),
                        BoxSpec::new(),
                        || {},
                    );
                },
            );
        },
    );
}

fn cold_capture(bake: bool) -> Option<(Vec<u8>, cranpose_render_wgpu::RenderStatsSnapshot)> {
    let (_lock, mut shell) = support::app_shell_for(
        GradientPage,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |renderer| renderer.set_underlay_bake_enabled(bake),
    )?;
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    let stats = shell.renderer().last_frame_stats().expect("frame stats");
    Some((frame.pixels, stats))
}

#[test]
fn a_baked_underlay_reproduces_the_transparent_clear_within_one_lsb_and_saves_three_passes() {
    let Some((reference, reference_stats)) = cold_capture(false) else {
        return;
    };
    let Some((baked, baked_stats)) = cold_capture(true) else {
        return;
    };
    assert_eq!(reference.len(), baked.len());

    let mut max_delta = 0u8;
    let mut differing = 0usize;
    for (a, b) in reference
        .as_chunks::<4>()
        .0
        .iter()
        .zip(baked.as_chunks::<4>().0)
    {
        let delta = a
            .iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        max_delta = max_delta.max(delta);
        differing += usize::from(delta > 0);
    }
    assert!(
        max_delta <= 1,
        "a baked underlay must reproduce the transparent-clear render to within one LSB, got {max_delta}"
    );
    let card_pixels = (CARD[2] * CARD[3]) as usize;
    assert!(
        differing * 50 < card_pixels,
        "only rounded-corner coverage pixels may round differently, but {differing} pixels differ"
    );
    assert_eq!(
        baked_stats.pass_count + 3,
        reference_stats.pass_count,
        "baking the underlay turns the underlay resample, the nested capture's underlay merge and the surface's own clear into copies and loads: {baked_stats:?} vs {reference_stats:?}"
    );
}
