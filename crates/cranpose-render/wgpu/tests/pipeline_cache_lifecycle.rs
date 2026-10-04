use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::ThreadId,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cranpose_render_common::{Renderer, graph::RenderGraph};
use cranpose_render_wgpu::{
    CapturedFrame, debug_toggle_os, pipelines_created, pipelines_created_off_frame,
    set_debug_toggle_os,
};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, LiquidGlassRect, LiquidGlassSpec, LiquidLoupeSpec,
    Rect, RenderEffect, liquid_glass_effect, liquid_loupe_effect,
};

use crate::support::{
    self,
    glass_page::{GlassFolds, panes_page},
};

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

fn cache_process(test: &str, cache: &Path, environment: &[(&str, &OsStr)]) -> std::process::Output {
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", test, "--nocapture"])
        .env(CACHE_FILE, cache)
        .env(CACHE_ENABLED, "1")
        .envs(environment.iter().copied())
        .output()
        .expect("launch cache lifecycle process");
    assert!(
        output.status.success(),
        "{test} {environment:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
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

/// A striped page under one glass pane.
fn glass_page() -> RenderGraph {
    panes_page([support::glass_page::glass_shader()])
}

/// A glass pane of the material `spec` describes.
fn glass_of(spec: &LiquidGlassSpec) -> RenderEffect {
    use support::glass_page::{GLASS_HEIGHT, GLASS_WIDTH};
    liquid_glass_effect(
        &LiquidGlassRect {
            left: 0.0,
            top: 0.0,
            width: GLASS_WIDTH,
            height: GLASS_HEIGHT,
            tint_color: Color(1.0, 1.0, 1.0, 0.12),
        },
        spec,
        GLASS_WIDTH,
        GLASS_HEIGHT,
    )
}

/// What `renderer` waited for while drawing `page`.
fn frame_waits_of(renderer: &mut support::LockedRenderer, page: RenderGraph) -> u64 {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    support::capture_waits(renderer, page, FRAME_WIDTH, FRAME_HEIGHT).1
}

/// Draws `page` until it draws without a placeholder, so the launch
/// records the pipelines it draws with.
fn draw_settled(renderer: &mut support::LockedRenderer, page: RenderGraph) {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    support::capture_graph_settled(renderer, page, FRAME_WIDTH, FRAME_HEIGHT);
}

/// After an update the compiled pipelines are gone, but the glass the last
/// launch drew its first screen with is built before the first frame from
/// the framework's own shader source, so that frame waits for nothing.
#[test]
fn an_updated_build_draws_its_first_glass_with_pipelines_built_before_it() {
    relaunch_after_drawing(
        |previous| {
            assert!(
                frame_waits_of(previous, glass_page()) > 0,
                "a fresh first screen waits for its glass"
            );
            draw_settled(previous, glass_page());
        },
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            wait_for_warm_ups();
            assert_eq!(
                frame_waits_of(updated, glass_page()),
                0,
                "the first glass after an update must find its pipelines built"
            );
        },
    );
}

/// A striped page under one frosted pane: its blur and the composite that
/// places it draw with fixed pipelines.
fn frosted_page() -> RenderGraph {
    panes_page([RenderEffect::blur(8.0)])
}

/// What `renderer` waited for while drawing the frosted page.
fn frosted_frame_waits(renderer: &mut support::LockedRenderer) -> u64 {
    let waits = frame_waits_of(renderer, frosted_page());
    let stats = renderer
        .last_frame_stats()
        .expect("frosted frame statistics");
    assert!(
        stats.blur_passes > 0 || stats.placeholder_draws > 0,
        "the pane must blur, or wait for its blur"
    );
    waits
}

fn capture_frosted_page(renderer: &mut support::LockedRenderer) -> CapturedFrame {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    let frame = support::capture_graph(renderer, frosted_page(), FRAME_WIDTH, FRAME_HEIGHT);
    let stats = renderer
        .last_frame_stats()
        .expect("frosted frame statistics");
    assert!(stats.blur_passes > 0, "the pane must blur");
    frame
}

/// The blur and composite pipelines the last launch drew its first screen
/// with are built before the next launch's first frame, like its shapes and
/// glass, so after an update that frame builds nothing.
#[test]
fn an_updated_build_draws_its_first_frosted_pane_with_pipelines_built_before_it() {
    relaunch_after_drawing(
        |previous| {
            assert!(
                frosted_frame_waits(previous) > 0,
                "a fresh first screen waits for its blur"
            );
            draw_settled(previous, frosted_page());
        },
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            wait_for_warm_ups();
            assert_eq!(
                frosted_frame_waits(updated),
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
            draw_settled(previous, frosted_page());
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
                frosted_frame_waits(updated),
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
                frosted_frame_waits(relaunch) > 0,
                "only an update prepares the pipelines of later screens"
            );
        },
    );
}

/// The same build finds what its launches drew in the driver's cache: a
/// later screen's first draw builds those pipelines in its frame, as cache
/// hits, instead of drawing placeholders.
#[test]
fn a_relaunch_of_the_same_build_draws_what_it_recorded_from_the_driver_cache() {
    relaunch_after_a_later_screen(
        |_| {},
        |relaunch| {
            if !relaunch
                .try_device()
                .expect("GPU initialized")
                .features()
                .contains(wgpu::Features::PIPELINE_CACHE)
            {
                eprintln!("the driver cache needs PIPELINE_CACHE support");
                return;
            }
            first_frame_builds(relaunch);
            wait_for_warm_ups();
            frosted_frame_waits(relaunch);
            let stats = relaunch
                .last_frame_stats()
                .expect("frosted frame statistics");
            assert_eq!(
                stats.placeholder_draws, 0,
                "a pipeline this build recorded is drawn from the driver cache"
            );
        },
    );
}

/// After an update an app builds every pipeline its last launches drew
/// before it draws a frame, its later screens' included, so a screen it
/// opens later finds them built.
#[test]
fn preparing_an_update_builds_every_recorded_pipeline_before_any_frame() {
    relaunch_after_a_later_screen(
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            assert!(
                updated.prepare_recorded_pipelines(Duration::from_secs(30), &|| false),
                "the recorded pipelines must build"
            );
            assert_eq!(
                frosted_frame_waits(updated),
                0,
                "a prepared update draws its last launch's later screen with pipelines built before it"
            );
        },
    );
}

/// An app that launches while its update is being prepared stops the
/// preparation: its own frames build what is left.
#[test]
fn preparing_an_update_stops_when_told() {
    relaunch_after_a_later_screen(
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            assert!(
                !updated.prepare_recorded_pipelines(Duration::from_secs(30), &|| true),
                "a stopped preparation reports the pipelines unfinished"
            );
        },
    );
}

/// After an update the pipelines an app prepares in the background are
/// written for this build: its next launch is not an update.
#[test]
fn a_prepared_update_leaves_this_builds_cache() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("prepared-update.bin");
    let mut previous =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    draw_settled(&mut previous, glass_page());
    drop(previous);
    wait_for_cache(&cache);
    let this_build = fs::read(&cache).expect("this build's cache");
    let mut old_build = this_build.clone();
    old_build[0] ^= 0xff;
    fs::write(&cache, &old_build).expect("an older build's cache");

    let mut preparing =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    assert!(preparing.prepare_recorded_pipelines(Duration::from_secs(30), &|| false));
    drop(preparing);
    let prepared = fs::read(&cache).expect("the prepared cache");
    assert_eq!(
        prepared[0], this_build[0],
        "the prepared file is this build's"
    );
}

/// A cache the app's build wrote while drawing its glass page, as an app
/// ships one for its fresh installs, with the cache file removed after.
fn shipped_cache(cache: &Path) -> Vec<u8> {
    let mut writer =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    draw_settled(&mut writer, glass_page());
    drop(writer);
    wait_for_cache(cache);
    let shipped = fs::read(cache).expect("the shipped cache");
    fs::remove_file(cache).expect("a fresh install has no cache");
    shipped
}

/// A fresh install that ships the cache its build wrote on a device with
/// this GPU and driver starts from it whole, like a relaunch: its first
/// glass finds its pipelines built. A seed never replaces a cache the app
/// has.
#[test]
fn a_fresh_install_seeded_for_this_driver_launches_like_a_relaunch() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("seeded-same-driver.bin");
    let shipped = shipped_cache(&cache);
    cranpose_render_wgpu::pipeline_disk_cache::seed(&cache, shipped.clone(), true);
    assert_eq!(fs::read(&cache).expect("the seeded cache"), shipped);
    cranpose_render_wgpu::pipeline_disk_cache::seed(&cache, vec![0; 64], true);
    assert_eq!(
        fs::read(&cache).expect("the seeded cache"),
        shipped,
        "a seed never replaces a cache the app has"
    );
    let mut launch =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    wait_for_warm_ups();
    assert_eq!(
        frame_waits_of(&mut launch, glass_page()),
        0,
        "a seeded fresh install draws its first glass with pipelines built before it"
    );
}

/// Another device's seed counts only for its records: its compiled
/// pipelines are that driver's, but the records still build the first
/// screen before the first frame.
#[test]
fn a_seed_from_another_driver_keeps_only_its_records() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("seeded-other-driver.bin");
    let shipped = shipped_cache(&cache);
    cranpose_render_wgpu::pipeline_disk_cache::seed(&cache, shipped.clone(), false);
    let seeded = fs::read(&cache).expect("the seeded cache");
    assert_ne!(
        seeded[..8],
        shipped[..8],
        "another driver's seed is another build's"
    );
    assert_eq!(seeded[8..], shipped[8..]);
    let mut launch =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    wait_for_warm_ups();
    assert_eq!(
        frame_waits_of(&mut launch, glass_page()),
        0,
        "the records build the first glass before the first frame"
    );
}

/// The driver cache holds what the last launch drew, but a launch with no
/// records draws no stand-in: one would compile from nothing. A relaunch
/// draws its cached glass with the glass's own pipelines, not a stand-in.
#[test]
fn a_relaunch_draws_cached_glass_with_its_own_pipelines() {
    use support::glass_page::{FRAME_HEIGHT, FRAME_WIDTH};
    // Folded, each material below is a pipeline of its own.
    let _folds = GlassFolds::set(true);
    let frosted = LiquidGlassSpec {
        blur_radius: 4.0,
        ..LiquidGlassSpec::default()
    };
    let page = || panes_page([support::glass_page::glass_shader(), glass_of(&frosted)]);
    relaunch_after_drawing(
        |previous| draw_settled(previous, page()),
        |_| {},
        |relaunch| {
            if !relaunch
                .try_device()
                .expect("GPU initialized")
                .features()
                .contains(wgpu::Features::PIPELINE_CACHE)
            {
                eprintln!("the driver cache needs PIPELINE_CACHE support");
                return;
            }
            support::capture_graph(relaunch, page(), FRAME_WIDTH, FRAME_HEIGHT);
            let stats = relaunch.last_frame_stats().expect("glass frame statistics");
            assert_eq!(
                stats.shader_pipeline_fallback_draws, 0,
                "a cached glass draws with its own pipelines"
            );
            assert_eq!(stats.placeholder_draws, 0);
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
    // Folded, each material below is a pipeline of its own.
    let _folds = GlassFolds::set(true);
    let frosted = LiquidGlassSpec {
        blur_radius: 4.0,
        ..LiquidGlassSpec::default()
    };
    relaunch_after_drawing(
        |previous| {
            first_frame_builds(previous);
            std::thread::sleep(Duration::from_millis(2100));
            // A new material waits for its own pipelines; the general
            // follows its frame, and the next new material stands in.
            draw_settled(previous, panes_page([support::glass_page::glass_shader()]));
            support::wait_for_background_compiler_idle();
            let (_, stats) = support::capture_drawn(
                previous,
                &panes_page([glass_of(&frosted)]),
                FRAME_WIDTH,
                FRAME_HEIGHT,
            );
            assert!(
                stats.shader_pipeline_fallback_draws > 0,
                "the last launch stands a glass in with the general pipeline"
            );
        },
        |bytes| bytes[0] ^= 0xff,
        |updated| {
            first_frame_builds(updated);
            wait_for_warm_ups();
            use support::glass_page::{GLASS_HEIGHT, GLASS_WIDTH};
            let loupe =
                liquid_loupe_effect((GLASS_WIDTH, GLASS_HEIGHT), &LiquidLoupeSpec::default());
            let builds = frame_waits_of(updated, panes_page([loupe]));
            let stats = updated.last_frame_stats().expect("glass frame statistics");
            assert!(
                stats.shader_pipeline_fallback_draws > 0,
                "a new material stands in while its own pipeline compiles"
            );
            assert_eq!(
                builds, 0,
                "the stand-in must be built before the new material needs it"
            );
        },
    );
}

static DRAW_THREAD: OnceLock<ThreadId> = OnceLock::new();
static DELAYED_ON_DRAW: AtomicBool = AtomicBool::new(false);
static PIPELINES_AFTER_DELAY: AtomicU64 = AtomicU64::new(0);

struct DemandCompileDelay;

impl log::Log for DemandCompileDelay {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let pipeline_log =
            message.starts_with("[gpu-pipeline]") || message.starts_with("[pipeline-create]");
        let on_draw_thread = DRAW_THREAD
            .get()
            .is_some_and(|draw_thread| *draw_thread == std::thread::current().id());
        if !pipeline_log || !on_draw_thread {
            return;
        }
        if DELAYED_ON_DRAW.load(Ordering::Acquire) {
            PIPELINES_AFTER_DELAY.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if !message.starts_with("[gpu-pipeline]")
            || !message.contains("effect/blur")
            || DELAYED_ON_DRAW.swap(true, Ordering::AcqRel)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(2100));
    }

    fn flush(&self) {}
}

static DEMAND_COMPILE_DELAY: DemandCompileDelay = DemandCompileDelay;

#[test]
fn a_demand_compile_wait_keeps_later_first_screen_pipelines() {
    const PHASE: &str = "CRANPOSE_CACHE_DEMAND_WAIT_PHASE";
    const PIXELS: &str = "CRANPOSE_CACHE_DEMAND_WAIT_PIXELS";

    if let Ok(phase) = std::env::var(PHASE) {
        if phase == "first" {
            DRAW_THREAD
                .set(std::thread::current().id())
                .expect("record the draw thread before installing the logger");
            log::set_logger(&DEMAND_COMPILE_DELAY).expect("install the demand-delay logger");
            log::set_max_level(log::LevelFilter::Info);
        }
        let _lock = support::gpu_test_lock();
        let cache = PathBuf::from(std::env::var_os(CACHE_FILE).expect("cache file"));
        let pixels = PathBuf::from(std::env::var_os(PIXELS).expect("pixel file"));
        let mut renderer =
            support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
        if !renderer
            .try_device()
            .expect("GPU initialized")
            .features()
            .contains(wgpu::Features::PIPELINE_CACHE)
        {
            eprintln!("PIPELINE_CACHE_UNSUPPORTED");
            return;
        }

        match phase.as_str() {
            "first" => {
                let frame = capture_frosted_page(&mut renderer);
                assert!(
                    frame.pixels.iter().any(|pixel| *pixel != 0),
                    "the frosted first screen must produce visible pixels"
                );
                assert!(
                    DELAYED_ON_DRAW.load(Ordering::Acquire),
                    "the fixed pipeline delay did not run on the draw thread"
                );
                assert!(
                    PIPELINES_AFTER_DELAY.load(Ordering::Acquire) > 0,
                    "no later pipeline was built after the delay"
                );
                fs::write(&pixels, frame.pixels).expect("save the delayed first-screen pixels");
                drop(renderer);
                wait_for_cache(&cache);
            }
            "reopened" => {
                wait_for_warm_ups();
                let before = pipelines_created();
                let frame = capture_frosted_page(&mut renderer);
                assert_eq!(
                    pipelines_created() - before,
                    0,
                    "the first scene after a slow compile must build no pipelines"
                );
                assert_eq!(
                    frame.pixels,
                    fs::read(&pixels).expect("read the first launch pixels"),
                    "warm-up must preserve the rendered first screen"
                );
            }
            _ => panic!("unknown demand-wait phase"),
        }
        return;
    }

    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let cache = files.select("demand-wait.bin");
    let pixels = cache.with_extension("first-screen.rgba");
    for phase in ["first", "reopened"] {
        let output = cache_process(
            "pipeline_cache_lifecycle::a_demand_compile_wait_keeps_later_first_screen_pipelines",
            &cache,
            &[(PHASE, OsStr::new(phase)), (PIXELS, pixels.as_os_str())],
        );
        if String::from_utf8_lossy(&output.stderr).contains("PIPELINE_CACHE_UNSUPPORTED") {
            eprintln!("pipeline cache lifecycle requires PIPELINE_CACHE support");
            return;
        }
    }
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
        cache_process(
            "pipeline_cache_lifecycle::warm_up_lists_follow_presented_frames_across_process_restarts",
            &cache,
            &[(PHASE, OsStr::new(phase))],
        );
    }
}
