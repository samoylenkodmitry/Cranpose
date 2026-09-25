use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{
    Alignment, Box, BoxSpec, Brush, Color, CompositingStrategy, GraphicsLayer, Modifier, Rect,
    Text, TextStyle, composable,
    text::{SpanStyle, TextUnit},
};

use crate::support;

const FRAME: u32 = 240;
const PAGE: Color = Color(1.0, 1.0, 1.0, 1.0);
const INK: Color = Color(0.1, 0.2, 0.8, 1.0);
/// Enough rects that a layer of them is past what draws in place every
/// frame whatever its content does.
const MANY_RECTS: usize = 300;
/// Where the layer cache counts its hits on isolated layers' surfaces.
const SOURCE_KIND: usize = 0;

/// What the page turns about its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Scene {
    /// A 120x60 box of ink turned by 23 degrees.
    Box,
    /// A line of ink text turned by -17 degrees.
    Text,
    /// An 80x40 box of ink turned by 40 degrees inside a clear box turned
    /// by -15.
    Nested,
    /// The nested boxes with the outer one always drawn offscreen, so the
    /// inner one draws in place into a surface at an offset of its own.
    InSurface,
    /// A 160x40 box of ink turned by 30 degrees, clipped by the unturned
    /// 100x100 box it sits in.
    Clipped,
    /// Many small rects of ink in a box turned by 11 degrees, as wide as the
    /// page's width state.
    Many,
}

impl Scene {
    /// The surfaces the scene renders when its turned layers may draw in
    /// place.
    fn surfaces_in_place(self) -> u32 {
        u32::from(self == Scene::InSurface)
    }
}

fn turned(degrees: f32, offscreen: bool) -> Modifier {
    Modifier::empty().graphics_layer_value(GraphicsLayer {
        rotation_z: degrees,
        compositing_strategy: if offscreen {
            CompositingStrategy::Offscreen
        } else {
            CompositingStrategy::Auto
        },
        ..Default::default()
    })
}

fn centred() -> BoxSpec {
    BoxSpec::new().content_alignment(Alignment::CENTER)
}

#[composable]
fn TurnedPage(scene: Scene, offscreen: bool, width: MutableState<f32>) {
    Box(
        Modifier::empty()
            .size_points(FRAME as f32, FRAME as f32)
            .background(PAGE),
        centred(),
        move || {
            match scene {
                Scene::Box => Box(
                    Modifier::empty()
                        .size_points(120.0, 60.0)
                        .then(turned(23.0, offscreen))
                        .background(INK),
                    BoxSpec::default(),
                    || {},
                ),
                Scene::Text => Box(turned(-17.0, offscreen), BoxSpec::default(), || {
                    Text(
                        "Turned in place 0123",
                        Modifier::empty(),
                        TextStyle::from_span_style(SpanStyle {
                            color: Some(INK),
                            font_size: TextUnit::Sp(18.0),
                            ..Default::default()
                        }),
                    );
                }),
                Scene::Nested | Scene::InSurface => Box(
                    Modifier::empty()
                        .size_points(160.0, 100.0)
                        .then(turned(-15.0, offscreen || scene == Scene::InSurface)),
                    centred(),
                    move || {
                        Box(
                            Modifier::empty()
                                .size_points(80.0, 40.0)
                                .then(turned(40.0, offscreen))
                                .background(INK),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                ),
                Scene::Clipped => Box(
                    Modifier::empty().size_points(100.0, 100.0).clip_to_bounds(),
                    centred(),
                    move || {
                        Box(
                            Modifier::empty()
                                .size_points(160.0, 40.0)
                                .then(turned(30.0, offscreen))
                                .background(INK),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                ),
                Scene::Many => Box(
                    Modifier::empty()
                        .size_points(width.get(), 100.0)
                        .then(turned(11.0, offscreen))
                        .draw_behind(|scope| {
                            let size = scope.size();
                            let step = size.width / MANY_RECTS as f32;
                            for index in 0..MANY_RECTS {
                                scope.draw_rect_at(
                                    Rect {
                                        x: index as f32 * step,
                                        y: (index % 10) as f32 * 9.0,
                                        width: step * 0.5,
                                        height: 6.0,
                                    },
                                    Brush::solid(INK),
                                );
                            }
                        }),
                    BoxSpec::default(),
                    || {},
                ),
            };
        },
    );
}

struct TurnedHarness {
    shell: AppShell<WgpuRenderer>,
    width: Rc<RefCell<Option<MutableState<f32>>>>,
}

impl TurnedHarness {
    fn new(renderer: WgpuRenderer, scene: Scene, offscreen: bool) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let width: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
        let width_for_app = Rc::clone(&width);
        let mut shell = AppShell::new(renderer, root_key, move || {
            let state = cranpose_core::rememberMutableStateOf(|| 200.0f32);
            *width_for_app.borrow_mut() = Some(state);
            TurnedPage(scene, offscreen, state);
        });
        shell.set_viewport(FRAME as f32, FRAME as f32);
        shell.set_buffer_size(FRAME, FRAME);
        shell.update();
        Self { shell, width }
    }

    fn frame(&mut self, width: f32) -> (RenderStatsSnapshot, CapturedFrame) {
        let state = self
            .width
            .borrow()
            .as_ref()
            .copied()
            .expect("state captured");
        self.shell.debug_enter_app_context(|| state.set(width));
        support::update_and_capture(&mut self.shell, FRAME, FRAME)
    }

    fn settled(&mut self) -> (RenderStatsSnapshot, CapturedFrame) {
        let mut stats = None;
        let frame = support::settle(|| {
            let (frame_stats, frame) = self.frame(200.0);
            stats = Some(frame_stats);
            (frame_stats, frame)
        });
        (stats.expect("settle draws a frame"), frame)
    }
}

/// How much ink a frame holds and where: each pixel counts by how far its red
/// channel has gone from the page's towards the ink's.
struct Coverage {
    area: f32,
    centroid: [f32; 2],
}

fn coverage(frame: &CapturedFrame) -> Coverage {
    let page = PAGE.0 * 255.0;
    let ink = INK.0 * 255.0;
    let mut area = 0.0f32;
    let mut moment = [0.0f32; 2];
    for (index, pixel) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
        let weight = ((page - f32::from(pixel[0])) / (page - ink)).clamp(0.0, 1.0);
        let x = (index % FRAME as usize) as f32 + 0.5;
        let y = (index / FRAME as usize) as f32 + 0.5;
        area += weight;
        moment[0] += weight * x;
        moment[1] += weight * y;
    }
    Coverage {
        area,
        centroid: [moment[0] / area, moment[1] / area],
    }
}

/// Draws `scene` in place and through surfaces of its own and returns the
/// ink of each, having checked each took the path it names.
fn in_place_and_offscreen(scene: Scene) -> Option<(Coverage, Coverage)> {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return None;
        }
    };
    let (in_place_stats, in_place) = TurnedHarness::new(renderer, scene, false).settled();
    let offscreen_renderer =
        support::headless_renderer_beside_locked().expect("reference renderer");
    let (offscreen_stats, offscreen) =
        TurnedHarness::new(offscreen_renderer, scene, true).settled();
    assert_eq!(
        in_place_stats.isolated_layer_renders,
        scene.surfaces_in_place(),
        "{scene:?}: a layer that only turns draws in place: {in_place_stats:?}"
    );
    assert!(
        offscreen_stats.isolated_layer_renders > scene.surfaces_in_place(),
        "{scene:?}: an offscreen layer draws through its surface: {offscreen_stats:?}"
    );
    Some((coverage(&in_place), coverage(&offscreen)))
}

fn assert_lands_alike(
    scene: Scene,
    area_tolerance: f32,
    centroid_tolerance: f32,
) -> Option<Coverage> {
    let (drawn, composited) = in_place_and_offscreen(scene)?;
    let area_error = (drawn.area - composited.area).abs() / composited.area;
    assert!(
        area_error <= area_tolerance,
        "{scene:?}: in place draws {} of ink, its surface {}",
        drawn.area,
        composited.area
    );
    let shift = (drawn.centroid[0] - composited.centroid[0])
        .hypot(drawn.centroid[1] - composited.centroid[1]);
    assert!(
        shift <= centroid_tolerance,
        "{scene:?}: in place the ink centres at {:?}, through its surface at {:?}",
        drawn.centroid,
        composited.centroid
    );
    Some(drawn)
}

fn assert_centred(scene: Scene, coverage: &Coverage, tolerance: f32) {
    let centre = FRAME as f32 / 2.0;
    let shift = (coverage.centroid[0] - centre).hypot(coverage.centroid[1] - centre);
    assert!(
        shift <= tolerance,
        "{scene:?}: the turned ink centres at {:?}, not the page's centre",
        coverage.centroid
    );
}

#[test]
fn a_turned_box_drawn_in_place_covers_what_its_surface_covers() {
    let Some(drawn) = assert_lands_alike(Scene::Box, 0.005, 0.05) else {
        return;
    };
    assert!(
        (drawn.area - 120.0 * 60.0).abs() <= 120.0 * 60.0 * 0.005,
        "a 120x60 box keeps its area when turned, not {}",
        drawn.area
    );
    assert_centred(Scene::Box, &drawn, 0.05);
}

#[test]
fn turned_text_drawn_in_place_lands_where_its_surface_lands() {
    assert_lands_alike(Scene::Text, 0.04, 0.25);
}

#[test]
fn nested_turns_drawn_in_place_compose_like_nested_surfaces() {
    let Some(drawn) = assert_lands_alike(Scene::Nested, 0.005, 0.05) else {
        return;
    };
    assert!(
        (drawn.area - 80.0 * 40.0).abs() <= 80.0 * 40.0 * 0.005,
        "an 80x40 box keeps its area under two turns, not {}",
        drawn.area
    );
    assert_centred(Scene::Nested, &drawn, 0.05);
}

#[test]
fn a_child_drawn_in_place_into_its_parent_s_surface_lands_where_its_own_surface_does() {
    let Some(drawn) = assert_lands_alike(Scene::InSurface, 0.005, 0.05) else {
        return;
    };
    assert_centred(Scene::InSurface, &drawn, 0.05);
}

#[test]
fn a_clip_around_a_child_drawn_in_place_cuts_it_like_its_surface() {
    let Some(drawn) = assert_lands_alike(Scene::Clipped, 0.005, 0.05) else {
        return;
    };
    assert!(
        drawn.area < 160.0 * 40.0 * 0.9,
        "the unturned clip must cut the turned box's ends, leaving {}",
        drawn.area
    );
}

fn many_rects() -> Option<(std::sync::MutexGuard<'static, ()>, TurnedHarness)> {
    match support::headless_renderer_parts() {
        Ok((lock, renderer)) => Some((lock, TurnedHarness::new(renderer, Scene::Many, false))),
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            None
        }
    }
}

#[test]
fn a_large_turned_layer_whose_content_holds_still_is_composited_from_the_cache() {
    let Some((_lock, mut harness)) = many_rects() else {
        return;
    };
    for frame in 0..6 {
        let (stats, _) = harness.frame(200.0);
        if frame < 2 {
            continue;
        }
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame}: still content renders its surface once: {stats:?}"
        );
        assert!(
            stats.layer_cache_hits_by_kind[SOURCE_KIND] > 0,
            "frame {frame}: {MANY_RECTS} still rects cost less composited from their cached \
             surface than drawn again: {stats:?}"
        );
    }
}

#[test]
fn a_large_turned_layer_whose_content_changes_draws_in_place() {
    let Some((_lock, mut harness)) = many_rects() else {
        return;
    };
    for frame in 0..6 {
        let (stats, _) = harness.frame(150.0 + frame as f32 * 7.0);
        if frame < 1 {
            continue;
        }
        assert_eq!(
            stats.isolated_layer_renders, 0,
            "frame {frame}: content that changes every frame draws in place, not into a \
             surface it would render afresh: {stats:?}"
        );
        assert_eq!(
            stats.layer_cache_hits_by_kind[SOURCE_KIND], 0,
            "frame {frame}: {stats:?}"
        );
    }
}
