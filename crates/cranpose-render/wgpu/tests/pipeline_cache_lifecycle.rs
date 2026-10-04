use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cranpose_render_common::{
    Renderer,
    graph::{ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_render_wgpu::{
    debug_toggle_os, pipelines_created, pipelines_created_off_frame, set_debug_toggle_os,
};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, GraphicsLayer, LayerShape, LiquidGlassRect,
    LiquidGlassSpec, Rect, RenderEffect, RoundedCornerShape, liquid_glass_effect,
};

use crate::{shared_test_support, support};

const CACHE_FILE: &str = "CRANPOSE_PIPELINE_CACHE_FILE";
const CACHE_ENABLED: &str = "CRANPOSE_PIPELINE_DISK_CACHE";

struct CacheFiles {
    root: PathBuf,
    previous_file: Option<OsString>,
    previous_enabled: Option<OsString>,
}

impl CacheFiles {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows the Unix epoch")
            .as_nanos();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../target/test-output")
            .join(format!(
                "cranpose-cache-lifecycle-{}-{stamp}",
                std::process::id()
            ));
        fs::create_dir_all(&root).expect("create cache lifecycle directory");
        let files = Self {
            root,
            previous_file: debug_toggle_os(CACHE_FILE),
            previous_enabled: debug_toggle_os(CACHE_ENABLED),
        };
        set_debug_toggle_os(CACHE_ENABLED, Some(std::ffi::OsStr::new("1")));
        files
    }

    fn select(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        set_debug_toggle_os(CACHE_FILE, Some(path.as_os_str()));
        path
    }
}

impl Drop for CacheFiles {
    fn drop(&mut self) {
        set_debug_toggle_os(CACHE_FILE, self.previous_file.as_deref());
        set_debug_toggle_os(CACHE_ENABLED, self.previous_enabled.as_deref());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn wait_until(what: &str, path: &Path, done: impl Fn(&[u8]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read(path).is_ok_and(|bytes| done(&bytes)) {
        assert!(Instant::now() < deadline, "{what}: {}", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_cache(path: &Path) {
    wait_until(
        "live renderer did not persist its pipeline cache",
        path,
        |bytes| !bytes.is_empty(),
    );
}

/// A renderer drawing a white square, or `None` where its backend keeps no
/// pipeline cache.
fn caching_renderer() -> Option<support::LockedRenderer> {
    let mut renderer = support::LockedRenderer::beside_locked().expect("GPU required");
    if !renderer
        .try_device()
        .expect("GPU initialized")
        .features()
        .contains(wgpu::Features::PIPELINE_CACHE)
    {
        eprintln!("pipeline cache lifecycle requires PIPELINE_CACHE support");
        return None;
    }
    renderer.scene_mut().graph = Some(RenderGraph::new(support::layer_node(
        None,
        16.0,
        16.0,
        vec![support::draw_node(
            DrawPrimitive::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 16.0,
                    height: 16.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            },
            None,
        )],
    )));
    Some(renderer)
}

#[test]
fn a_cache_file_another_build_left_is_replaced_with_this_renderers_pipelines() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("another-build.bin");
    let stale = vec![0xa5; 64 * 1024];
    fs::write(&cache, &stale).expect("write another build's cache file");
    let Some(mut renderer) = caching_renderer() else {
        return;
    };
    let frame = renderer
        .capture_frame(16, 16)
        .expect("frame over a stale cache");
    assert!(frame.pixels.as_chunks::<4>().0.contains(&[255; 4]));
    wait_until(
        "the renderer kept another build's pipeline cache",
        &cache,
        |bytes| !bytes.is_empty() && bytes != stale.as_slice(),
    );
}

#[test]
fn dropping_a_renderer_finishes_its_pending_cache_write() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("pending-at-shutdown.bin");
    let Some(mut renderer) = caching_renderer() else {
        return;
    };
    renderer.capture_frame(16, 16).expect("first frame");
    drop(renderer);
    assert!(
        fs::read(&cache).is_ok_and(|bytes| !bytes.is_empty()),
        "dropping the renderer must finish its pending cache write"
    );
}

#[test]
fn dropping_a_renderer_stops_writing_its_previous_pipeline_cache() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let previous_cache = files.select("previous.bin");
    let Some(mut renderer) = caching_renderer() else {
        return;
    };
    let original = renderer.capture_frame(16, 16).expect("original frame");
    assert!(original.pixels.as_chunks::<4>().0.contains(&[255; 4]));
    wait_for_cache(&previous_cache);

    let graph = renderer.scene_mut().graph.take();
    drop(renderer);
    fs::remove_file(&previous_cache).expect("remove retired renderer's cache file");
    let replacement_cache = files.select("replacement.bin");
    let mut renderer = support::LockedRenderer::beside_locked().expect("replacement GPU required");
    renderer.scene_mut().graph = graph;
    let replacement = renderer.capture_frame(16, 16).expect("replacement frame");
    assert_eq!(replacement.pixels, original.pixels);
    wait_for_cache(&replacement_cache);
    assert!(
        !previous_cache.exists(),
        "a retired renderer rewrote its obsolete cache: {}",
        previous_cache.display()
    );
}

/// A white square beside a rounded one, so a frame draws with more than one
/// shape pipeline.
fn squares() -> RenderGraph {
    let square = |x: f32| Rect {
        x,
        y: 0.0,
        width: 8.0,
        height: 8.0,
    };
    RenderGraph::new(support::layer_node(
        None,
        16.0,
        16.0,
        vec![
            support::draw_node(
                DrawPrimitive::Rect {
                    rect: square(0.0),
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                None,
            ),
            support::draw_node(
                DrawPrimitive::RoundRect {
                    rect: square(8.0),
                    brush: Brush::solid(Color::WHITE),
                    radii: CornerRadii::uniform(3.0),
                    stroke: None,
                },
                None,
            ),
        ],
    ))
}

/// Pipelines built on the frame thread while `renderer` draws its first
/// frame.
fn first_frame_builds(renderer: &mut support::LockedRenderer) -> u64 {
    renderer.scene_mut().graph = Some(squares());
    let before = pipelines_created();
    let frame = renderer.capture_frame(16, 16).expect("first frame");
    assert!(frame.pixels.as_chunks::<4>().0.contains(&[255; 4]));
    pipelines_created() - before
}

/// Waits for the background compiler to fall quiet: no pipeline built off
/// the frame for half a second.
fn wait_for_warm_ups() {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = pipelines_created_off_frame();
    let mut quiet_since = Instant::now();
    while quiet_since.elapsed() < Duration::from_millis(500) {
        assert!(Instant::now() < deadline, "warm-ups never fell quiet");
        std::thread::sleep(Duration::from_millis(20));
        let built = pipelines_created_off_frame();
        if built != seen {
            seen = built;
            quiet_since = Instant::now();
        }
    }
}

#[test]
fn a_relaunch_prepares_the_first_screen_even_when_startup_was_slow() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("first-screen.bin");

    let mut first_launch =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    std::thread::sleep(Duration::from_millis(2100));
    let first_launch_builds = first_frame_builds(&mut first_launch);
    drop(first_launch);
    wait_for_cache(&cache);

    let mut relaunch =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    wait_for_warm_ups();
    let relaunch_builds = first_frame_builds(&mut relaunch);
    assert!(
        relaunch_builds < first_launch_builds,
        "the relaunch built {relaunch_builds} pipelines in its first frame, \
         the first launch {first_launch_builds}"
    );
}

fn relaunch_after_cache_change(
    change: impl FnOnce(&mut Vec<u8>),
    check: impl FnOnce(&mut support::LockedRenderer),
) {
    relaunch_after_drawing(
        |previous| {
            assert!(
                first_frame_builds(previous) > 0,
                "a fresh first screen must compile pipelines"
            );
        },
        change,
        check,
    );
}

/// A launch that `draw`s its first screen and writes its cache, then a
/// relaunch over that cache once `change` has edited it, handed to `check`.
fn relaunch_after_drawing(
    draw: impl FnOnce(&mut support::LockedRenderer),
    change: impl FnOnce(&mut Vec<u8>),
    check: impl FnOnce(&mut support::LockedRenderer),
) {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("updated-build.bin");
    let mut previous =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    draw(&mut previous);
    drop(previous);
    wait_for_cache(&cache);

    let mut old_build = fs::read(&cache).expect("previous build's cache");
    change(&mut old_build);
    fs::write(&cache, old_build).expect("replace the previous cache");

    let mut updated =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    check(&mut updated);
}

#[test]
fn an_updated_build_prepares_the_previous_first_screen_with_fresh_pipelines() {
    relaunch_after_cache_change(
        |bytes| bytes[0] ^= 0xff,
        |renderer| {
            wait_for_warm_ups();
            assert_eq!(
                first_frame_builds(renderer),
                0,
                "an update must retain which pipelines to prepare before the first screen"
            );
        },
    );
}

#[test]
fn a_cache_file_without_pipeline_records_keeps_its_first_screen_shapes() {
    relaunch_after_cache_change(
        |bytes| {
            bytes[0] ^= 0xff;
            let section = bytes
                .windows(4)
                .position(|window| window == b"RSP3")
                .expect("the cache file keeps its first screen's records");
            bytes.truncate(section);
        },
        |renderer| {
            wait_for_warm_ups();
            assert_eq!(
                first_frame_builds(renderer),
                0,
                "a file without pipeline records must keep its shape warm-ups"
            );
        },
    );
}

#[test]
fn a_first_frame_uses_its_requested_warm_up_instead_of_compiling_a_stand_in() {
    relaunch_after_cache_change(
        |bytes| bytes[0] ^= 0xff,
        |renderer| {
            first_frame_builds(renderer);
            assert_eq!(
                renderer
                    .last_frame_stats()
                    .expect("first-frame statistics")
                    .shape_pipeline_fallback_draws,
                0,
                "the first frame must use the pipelines already scheduled for it"
            );
        },
    );
}

/// A striped page under one rounded pane whose backdrop `effect` draws.
fn paned_page(effect: RenderEffect) -> RenderGraph {
    use support::glass_page::{
        FRAME_HEIGHT, FRAME_WIDTH, GLASS_HEIGHT, GLASS_LEFT, GLASS_RADIUS, GLASS_TOP, GLASS_WIDTH,
    };
    let mut children = support::striped_page(FRAME_WIDTH, FRAME_HEIGHT);
    children.push(RenderNode::Layer(Box::new(
        shared_test_support::layer_node(
            Rect {
                x: 0.0,
                y: 0.0,
                width: GLASS_WIDTH,
                height: GLASS_HEIGHT,
            },
            ProjectiveTransform::translation(GLASS_LEFT, GLASS_TOP),
            GraphicsLayer {
                backdrop_effect: Some(effect),
                clip: true,
                shape: LayerShape::Rounded(RoundedCornerShape::uniform(GLASS_RADIUS)),
                ..GraphicsLayer::default()
            },
            Vec::new(),
        ),
    )));
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

/// A striped page under one glass pane.
fn glass_page() -> RenderGraph {
    paned_page(support::glass_page::glass_shader())
}

/// [`glass_page`] with a pane of another material: a specialization of its
/// own.
fn other_glass_page() -> RenderGraph {
    use support::glass_page::{GLASS_HEIGHT, GLASS_WIDTH};
    paned_page(liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: GLASS_WIDTH,
            height: GLASS_HEIGHT,
            tint_color: Color(1.0, 1.0, 1.0, 0.12),
        },
        &LiquidGlassSpec {
            blur_radius: 9.0,
            ..LiquidGlassSpec::default()
        },
        GLASS_WIDTH,
        GLASS_HEIGHT,
    ))
}

/// Glass draws in `renderer`'s next frame that took the general pipeline
/// while their own compiled.
fn glass_fallback_draws(renderer: &mut support::LockedRenderer) -> u32 {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    support::capture_graph(renderer, glass_page(), FRAME_WIDTH, FRAME_HEIGHT);
    renderer
        .last_frame_stats()
        .expect("glass frame statistics")
        .shader_pipeline_fallback_draws
}

/// After an update the compiled pipelines are gone, but the glass the last
/// launch drew its first screen with is built before the first frame from
/// the framework's own shader source, so that frame draws with it instead
/// of compiling the general pipeline to stand in.
#[test]
fn an_updated_build_draws_its_first_glass_with_pipelines_built_before_it() {
    relaunch_after_drawing(
        |previous| {
            assert!(
                glass_fallback_draws(previous) > 0,
                "a fresh first screen's glass stands in with the general pipeline"
            );
        },
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            wait_for_warm_ups();
            assert_eq!(
                glass_fallback_draws(updated),
                0,
                "the first glass after an update must find its pipelines built"
            );
        },
    );
}

/// A striped page under one frosted pane: its blur and the composite that
/// places it draw with fixed pipelines.
fn frosted_page() -> RenderGraph {
    paned_page(RenderEffect::blur(8.0))
}

/// Pipelines `renderer` built on the frame thread while drawing the frosted
/// page.
fn frosted_frame_builds(renderer: &mut support::LockedRenderer) -> u64 {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    let before = pipelines_created();
    support::capture_graph(renderer, frosted_page(), FRAME_WIDTH, FRAME_HEIGHT);
    let stats = renderer
        .last_frame_stats()
        .expect("frosted frame statistics");
    assert!(stats.blur_passes > 0, "the pane must blur");
    pipelines_created() - before
}

/// The blur and composite pipelines the last launch drew its first screen
/// with are built before the next launch's first frame, like its shapes and
/// glass, so after an update that frame builds nothing.
#[test]
fn an_updated_build_draws_its_first_frosted_pane_with_pipelines_built_before_it() {
    relaunch_after_drawing(
        |previous| {
            assert!(
                frosted_frame_builds(previous) > 0,
                "a fresh first screen builds its blur inside its frame"
            );
        },
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            wait_for_warm_ups();
            assert_eq!(
                frosted_frame_builds(updated),
                0,
                "the first frosted pane after an update must find its pipelines built"
            );
        },
    );
}

/// A launch that draws squares, then the frosted page once its first screen
/// is over, and a relaunch over its cache once `change` edited it.
fn relaunch_after_a_later_screen(
    change: impl FnOnce(&mut Vec<u8>),
    check: impl FnOnce(&mut support::LockedRenderer),
) {
    relaunch_after_drawing(
        |previous| {
            first_frame_builds(previous);
            std::thread::sleep(Duration::from_millis(2100));
            frosted_frame_builds(previous);
        },
        change,
        check,
    );
}

/// After an update the pipelines the last launch drew past its first screen
/// are built once the first frame is drawn, before the screen that needs
/// them.
#[test]
fn an_updated_build_prepares_the_later_screens_of_its_last_launch_after_its_first_frame() {
    relaunch_after_a_later_screen(
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            first_frame_builds(updated);
            wait_for_warm_ups();
            assert_eq!(
                frosted_frame_builds(updated),
                0,
                "a later screen after an update must find its pipelines built"
            );
        },
    );
}

/// The same build finds its compiled pipelines in the driver's cache, so it
/// builds a later screen's pipelines only when that screen draws.
#[test]
fn a_relaunch_of_the_same_build_leaves_later_screens_to_their_first_draw() {
    relaunch_after_a_later_screen(
        |_| {},
        |relaunch| {
            first_frame_builds(relaunch);
            wait_for_warm_ups();
            assert!(
                frosted_frame_builds(relaunch) > 0,
                "only an update prepares the pipelines of later screens"
            );
        },
    );
}

/// A material no launch drew stands in with the general glass while its own
/// pipeline compiles. After an update the general the last launch stood in
/// with is built once the first frame is drawn, so standing in compiles
/// nothing inside a frame.
#[test]
fn an_updated_build_stands_a_new_glass_in_with_a_general_built_after_its_first_frame() {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    relaunch_after_drawing(
        |previous| {
            first_frame_builds(previous);
            std::thread::sleep(Duration::from_millis(2100));
            assert!(
                glass_fallback_draws(previous) > 0,
                "the last launch stands its glass in with the general pipeline"
            );
        },
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            first_frame_builds(updated);
            wait_for_warm_ups();
            let before = pipelines_created();
            support::capture_graph(updated, other_glass_page(), FRAME_WIDTH, FRAME_HEIGHT);
            let stats = updated.last_frame_stats().expect("glass frame statistics");
            assert!(
                stats.shader_pipeline_fallback_draws > 0,
                "a new material stands in while its own pipeline compiles"
            );
            assert_eq!(
                pipelines_created() - before,
                0,
                "the stand-in must be built before the new material needs it"
            );
        },
    );
}

#[test]
fn an_incompatible_first_screen_key_layout_is_ignored() {
    relaunch_after_cache_change(
        |bytes| bytes[8] ^= 0xff,
        |renderer| {
            wait_for_warm_ups();
            assert!(
                first_frame_builds(renderer) > 0,
                "unknown key layouts must not queue pipelines"
            );
        },
    );
}

#[test]
fn warm_up_lists_follow_presented_frames_across_process_restarts() {
    const PHASE: &str = "CRANPOSE_CACHE_LIFECYCLE_PHASE";
    if let Ok(phase) = std::env::var(PHASE) {
        let _lock = support::gpu_test_lock();
        let mut renderer =
            support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
        let cache = PathBuf::from(std::env::var_os(CACHE_FILE).expect("cache file"));
        match phase.as_str() {
            "first" => {
                first_frame_builds(&mut renderer);
                wait_for_cache(&cache);
            }
            "closed" => {
                wait_for_warm_ups();
            }
            "reopened" => {
                wait_for_warm_ups();
                assert_eq!(
                    first_frame_builds(&mut renderer),
                    0,
                    "warm-up completion must not replace the known first screen with an empty list"
                );
            }
            "empty-screen" => {
                renderer.scene_mut().graph = Some(RenderGraph::new(support::layer_node(
                    None,
                    16.0,
                    16.0,
                    Vec::new(),
                )));
                renderer.capture_frame(16, 16).expect("empty screen");
                wait_for_warm_ups();
            }
            "changed-screen" => {
                wait_for_warm_ups();
                assert!(
                    first_frame_builds(&mut renderer) > 0,
                    "an empty screen must retire the earlier screen's warm-up list"
                );
            }
            _ => panic!("unknown lifecycle phase"),
        }
        return;
    }

    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("closed-before-first-frame.bin");
    for phase in [
        "first",
        "closed",
        "reopened",
        "empty-screen",
        "changed-screen",
    ] {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "pipeline_cache_lifecycle::warm_up_lists_follow_presented_frames_across_process_restarts",
                "--nocapture",
            ])
            .env(PHASE, phase)
            .env(CACHE_FILE, &cache)
            .env(CACHE_ENABLED, "1")
            .output()
            .expect("launch cache lifecycle process");
        assert!(
            output.status.success(),
            "{phase} failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
