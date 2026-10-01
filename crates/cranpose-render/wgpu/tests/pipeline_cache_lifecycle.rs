use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cranpose_render_common::{Renderer, graph::RenderGraph};
use cranpose_render_wgpu::{debug_toggle_os, set_debug_toggle_os};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect};

use crate::support;

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
        let root = std::env::temp_dir().join(format!(
            "cranpose-cache-lifecycle-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create cache lifecycle directory");
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

fn wait_for_cache(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0) {
        assert!(
            Instant::now() < deadline,
            "live renderer did not persist its pipeline cache: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn dropping_a_renderer_stops_writing_its_previous_pipeline_cache() {
    let _lock = support::gpu_test_lock();
    let files = CacheFiles::new();
    let previous_cache = files.select("previous.bin");
    let mut renderer = support::LockedRenderer::beside_locked().expect("GPU required");
    if !renderer
        .try_device()
        .expect("GPU initialized")
        .features()
        .contains(wgpu::Features::PIPELINE_CACHE)
    {
        eprintln!("pipeline cache lifecycle requires PIPELINE_CACHE support");
        return;
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
    let original = renderer.capture_frame(16, 16).expect("original frame");
    assert!(
        original
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel == [255; 4])
    );
    wait_for_cache(&previous_cache);

    let graph = renderer.scene_mut().graph.take();
    drop(renderer);
    std::thread::sleep(Duration::from_secs(3));
    fs::remove_file(&previous_cache).expect("remove retired renderer's cache file");
    let replacement_cache = files.select("replacement.bin");
    let mut renderer = support::LockedRenderer::beside_locked().expect("replacement GPU required");
    renderer.scene_mut().graph = graph;
    let replacement = renderer.capture_frame(16, 16).expect("replacement frame");
    assert_eq!(replacement.pixels, original.pixels);
    wait_for_cache(&replacement_cache);
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        !previous_cache.exists(),
        "a retired renderer rewrote its obsolete cache: {}",
        previous_cache.display()
    );
}
