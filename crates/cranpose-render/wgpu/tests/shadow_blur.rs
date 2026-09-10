mod support;

use cranpose_render_common::{
    Renderer,
    geometry::blur_reach_px,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, RenderGraph, RenderNode,
    },
};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{BlendMode, Brush, Color, DrawPrimitive, Rect, ShadowPrimitive};
use support::{ReferenceEdge, region_pixels, solid_rect};

const FRAME: u32 = 160;
const WIDE_RADIUS: f32 = 20.0;
const BACKGROUND: Color = Color(0.8, 0.8, 0.85, 1.0);
const CASTER: Rect = Rect {
    x: 50.0,
    y: 50.0,
    width: 60.0,
    height: 50.0,
};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn caster() -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: CASTER,
        brush: Brush::solid(Color::BLACK),
        stroke: None,
    }
}

#[test]
fn changing_shadow_casters_and_cutouts_preserves_the_painted_region() {
    let mut renderer = support::headless_renderer().expect("headless WGPU init failed");
    let background = || solid_rect(rect(0.0, 0.0, FRAME as f32, FRAME as f32), BACKGROUND);
    for inner in [false, true] {
        for (outer, hole, color) in [
            (
                rect(20.0, 25.0, 100.0, 90.0),
                rect(35.0, 40.0, 40.0, 50.0),
                Color(0.8, 0.1, 0.2, 0.5),
            ),
            (
                rect(30.0, 15.0, 95.0, 110.0),
                rect(60.0, 30.0, 45.0, 65.0),
                Color(0.1, 0.7, 0.3, 0.75),
            ),
        ] {
            let primitive = |bounds, color| {
                Box::new(DrawPrimitive::Rect {
                    rect: bounds,
                    brush: Brush::solid(color),
                    stroke: None,
                })
            };
            let shadow = if inner {
                ShadowPrimitive::Inner {
                    fill: primitive(outer, color),
                    cutout: primitive(hole, Color::WHITE),
                    blur_radius: 0.0,
                    blend_mode: BlendMode::SrcOver,
                    clip_rect: outer,
                }
            } else {
                ShadowPrimitive::Drop {
                    shape: primitive(outer, color),
                    cutout: Some(primitive(hole, Color::WHITE)),
                    blur_radius: 0.0,
                    blend_mode: BlendMode::SrcOver,
                }
            };
            let actual = support::capture_graph(
                &mut renderer,
                support::page_graph(
                    FRAME,
                    FRAME,
                    vec![
                        background(),
                        RenderNode::Primitive(PrimitiveEntry {
                            phase: PrimitivePhase::BeforeChildren,
                            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                                primitive: DrawPrimitive::Shadow(shadow),
                                clip: None,
                            }),
                        }),
                    ],
                ),
                FRAME,
                FRAME,
            );
            let mut reference = vec![background()];
            reference.extend(
                [
                    rect(outer.x, outer.y, outer.width, hole.y - outer.y),
                    rect(
                        outer.x,
                        hole.y + hole.height,
                        outer.width,
                        outer.y + outer.height - hole.y - hole.height,
                    ),
                    rect(outer.x, hole.y, hole.x - outer.x, hole.height),
                    rect(
                        hole.x + hole.width,
                        hole.y,
                        outer.x + outer.width - hole.x - hole.width,
                        hole.height,
                    ),
                ]
                .map(|bounds| solid_rect(bounds, color)),
            );
            let reference = support::capture_graph(
                &mut renderer,
                support::page_graph(FRAME, FRAME, reference),
                FRAME,
                FRAME,
            );
            let region = rect(0.0, 0.0, FRAME as f32, FRAME as f32);
            let difference = support::max_channel_delta(
                &region_pixels(&actual, region),
                &region_pixels(&reference, region),
            );
            assert!(
                difference <= 1,
                "inner={inner}, outer={outer:?}: {difference:?}"
            );
        }
    }
}

/// A page with one black drop shadow of `CASTER` and no caster drawn over
/// it, so the whole blurred shape shows; `cutout` clears the shape itself.
fn page(radius: Option<f32>, cutout: bool) -> RenderGraph {
    let mut children = vec![solid_rect(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        BACKGROUND,
    )];
    if let Some(blur_radius) = radius {
        children.push(RenderNode::Primitive(PrimitiveEntry {
            phase: PrimitivePhase::BeforeChildren,
            node: PrimitiveNode::Draw(DrawPrimitiveNode {
                primitive: DrawPrimitive::Shadow(ShadowPrimitive::Drop {
                    shape: Box::new(caster()),
                    cutout: cutout.then(|| Box::new(caster())),
                    blur_radius,
                    blend_mode: BlendMode::SrcOver,
                }),
                clip: None,
            }),
        }));
    }
    support::page_graph(FRAME, FRAME, children)
}

/// The caster's coverage blurred by the kernel, transparent beyond the
/// frame.
fn reference_alpha(radius: f32) -> Vec<f32> {
    let size = FRAME as usize;
    let coverage: Vec<f32> = (0..size * size)
        .map(|index| {
            let x = (index % size) as f32 + 0.5;
            let y = (index / size) as f32 + 0.5;
            let inside = (CASTER.x..CASTER.x + CASTER.width).contains(&x)
                && (CASTER.y..CASTER.y + CASTER.height).contains(&y);
            f32::from(u8::from(inside))
        })
        .collect();
    support::reference_blur(&coverage, size, size, 1, radius, ReferenceEdge::Transparent)
}

/// The worst channel deviation of `frame` from the background darkened by
/// `alpha`, over `region`, and where it is.
fn worst_deviation(frame: &CapturedFrame, alpha: &[f32], region: Rect) -> (f32, (usize, usize)) {
    let actual = region_pixels(frame, region);
    let background = [
        BACKGROUND.0 * 255.0,
        BACKGROUND.1 * 255.0,
        BACKGROUND.2 * 255.0,
    ];
    let mut worst = (0.0f32, (0, 0));
    for (index, chunk) in actual.chunks(4).enumerate() {
        let x = region.x as usize + index % region.width as usize;
        let y = region.y as usize + index / region.width as usize;
        let a = alpha[y * FRAME as usize + x];
        for (channel, value) in chunk.iter().take(3).enumerate() {
            let want = background[channel] * (1.0 - a);
            let delta = (f32::from(*value) - want).abs();
            if delta > worst.0 {
                worst = (delta, (x, y));
            }
        }
    }
    worst
}

/// The caster with its blur's reach around it, where the shadow lands.
fn shadow_region(radius: f32) -> Rect {
    let reach = radius.ceil() + 2.0;
    rect(
        CASTER.x - reach,
        CASTER.y - reach,
        CASTER.width + 2.0 * reach,
        CASTER.height + 2.0 * reach,
    )
}

/// A wide shadow's blur runs at a quarter of its surface's size, its
/// texels averaged into blocks and interpolated back, and its kernel
/// truncates at a whole number of scratch texels, which the surface's
/// margin makes a fraction over four pixels each: a bounded distance from
/// the kernel (9.8 measured), where a pass at the wrong pitch is 30 to 95.
const DOWNSCALE_BUDGET: f32 = 12.0;

#[test]
fn a_wide_drop_shadow_matches_its_kernel_within_the_downscale_budget() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let frame = support::capture_graph(&mut renderer, page(Some(WIDE_RADIUS), false), FRAME, FRAME);
    let (worst, at) = worst_deviation(
        &frame,
        &reference_alpha(WIDE_RADIUS),
        shadow_region(WIDE_RADIUS),
    );
    assert!(
        worst <= DOWNSCALE_BUDGET,
        "the radius-{WIDE_RADIUS} shadow diverges from its kernel by {worst} at {at:?}"
    );
}

/// The cutout clears the shadow under the caster exactly, at the surface's
/// full size, and the shadow around it still follows the kernel.
#[test]
fn a_wide_drop_shadow_with_a_cutout_is_clear_under_its_caster_and_blurred_around_it() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let frame = support::capture_graph(&mut renderer, page(Some(WIDE_RADIUS), true), FRAME, FRAME);
    let plain = support::capture_graph(&mut renderer, page(None, false), FRAME, FRAME);
    let interior = rect(
        CASTER.x + 1.0,
        CASTER.y + 1.0,
        CASTER.width - 2.0,
        CASTER.height - 2.0,
    );
    assert_eq!(
        region_pixels(&frame, interior),
        region_pixels(&plain, interior),
        "the cutout leaves the page untouched under the caster"
    );
    let alpha = reference_alpha(WIDE_RADIUS);
    let region = shadow_region(WIDE_RADIUS);
    let band = rect(region.x, region.y, region.width, CASTER.y - 2.0 - region.y);
    let (worst, at) = worst_deviation(&frame, &alpha, band);
    assert!(
        worst <= DOWNSCALE_BUDGET,
        "the shadow above the cut caster diverges from its kernel by {worst} at {at:?}"
    );
}

/// A wide shadow draws its surface at full size and blurs it at the
/// scratch size, three passes over a sixteenth of the surface. The cutout
/// page adds the interpolation back to full size and the cutout, two
/// passes over the surface whole, which sizes the surface; the plain page
/// then spends at most one and a half surfaces beyond the empty page. A
/// vertical pass left at full size spends two, and the cutout page one
/// more, which reads as a half-size surface and puts the plain page at
/// four.
#[test]
fn a_wide_drop_shadow_blurs_at_the_scratch_size() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let pass_pixels = |renderer: &mut support::LockedRenderer, radius: Option<f32>, cutout| {
        support::capture_graph(renderer, page(radius, cutout), FRAME, FRAME);
        renderer.last_frame_stats().expect("stats").pass_pixels
    };
    let none = pass_pixels(&mut renderer, None, false);
    let plain = pass_pixels(&mut renderer, Some(WIDE_RADIUS), false);
    let cut = pass_pixels(&mut renderer, Some(WIDE_RADIUS), true);
    let surface = cut.saturating_sub(plain) / 2;
    let spent = plain.saturating_sub(none);
    assert!(
        spent <= surface * 3 / 2,
        "the wide shadow must blur at the scratch size: none={none} plain={plain} cut={cut} \
         surface={surface} spent={spent}"
    );
}

/// A shadow's surface, and so every pixel it blurs, caches and blits, ends
/// where its kernel does: the caster grown by the blur's reach and the
/// surface's own pixel of rounding on each side, whatever the radius. At
/// radius 20 the blur runs on blocks of four, so the reach is the radius,
/// three blocks and the caster's own pixel, rounded up to the next block.
/// The ring past it is the page untouched.
#[test]
fn a_shadow_surface_ends_where_its_kernel_does() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    renderer.scene_mut().graph = Some(page(Some(WIDE_RADIUS), false));
    let stats = renderer
        .render_current_scene_to_texture(FRAME, FRAME)
        .expect("render should succeed");
    let reach = ((WIDE_RADIUS + 3.0 * 4.0 + 1.0) / 4.0).ceil() * 4.0;
    let budget = (CASTER.width + 2.0 * reach + 2.0) * (CASTER.height + 2.0 * reach + 2.0);
    assert!(
        stats.shadow_shape_cache_miss_pixels as f32 <= budget,
        "the radius-{WIDE_RADIUS} shadow's surface holds {} pixels, more than the {budget} its reach of {reach} allows",
        stats.shadow_shape_cache_miss_pixels
    );
    assert_eq!(blur_reach_px(WIDE_RADIUS), reach);

    let frame = support::capture_graph(&mut renderer, page(Some(WIDE_RADIUS), false), FRAME, FRAME);
    let background = region_pixels(
        &support::capture_graph(&mut renderer, page(None, false), FRAME, FRAME),
        rect(0.0, 0.0, 1.0, 1.0),
    );
    let outer = reach + 1.0;
    for y in 0..FRAME {
        for x in 0..FRAME {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = (CASTER.x - px).max(px - CASTER.x - CASTER.width);
            let dy = (CASTER.y - py).max(py - CASTER.y - CASTER.height);
            if dx.max(dy) < outer {
                continue;
            }
            let at = ((y * FRAME + x) * 4) as usize;
            assert_eq!(
                &frame.pixels[at..at + 4],
                &background[..],
                "the shadow reaches ({x}, {y}), past its kernel's reach of {reach}"
            );
        }
    }
}
