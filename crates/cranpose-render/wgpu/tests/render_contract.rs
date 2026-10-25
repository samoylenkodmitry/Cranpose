use std::path::{Path, PathBuf};

use cranpose_render_common::{
    Renderer,
    render_contract::{ALL_SHARED_RENDER_CASES, RenderedFrame},
};

use crate::support;

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("failed to read source directory") {
        let entry = entry.expect("failed to read source entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn assert_frame_graph_owns_calls(calls: &[&str], entry_point: Option<&str>) {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src_dir = crate_dir.join("src");
    let frame_graph = src_dir.join("frame_graph.rs");
    let mut rust_files = Vec::new();
    collect_rust_files(&src_dir, &mut rust_files);
    let violations: Vec<_> = rust_files
        .into_iter()
        .filter(|path| *path != frame_graph && !path.starts_with(src_dir.join("frame_graph")))
        .filter(|path| {
            let source = std::fs::read_to_string(path).expect("renderer source");
            calls.iter().any(|call| source.contains(call))
        })
        .map(|path| {
            path.strip_prefix(crate_dir)
                .unwrap_or(&path)
                .display()
                .to_string()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "{calls:?} must stay inside the frame_graph module: {violations:?}"
    );
    if let Some(entry_point) = entry_point {
        let source = std::fs::read_to_string(frame_graph).expect("frame graph source");
        assert!(
            source.contains(entry_point),
            "missing executor entry point: {entry_point}"
        );
    }
}

#[test]
fn wgpu_command_buffers_are_owned_by_frame_graph_executor() {
    assert_frame_graph_owns_calls(&[".submit(", ".create_command_encoder("], None);
}

#[test]
fn texture_uploads_are_owned_by_frame_graph_executor() {
    assert_frame_graph_owns_calls(&[".write_texture("], Some("pub(crate) fn upload_texture("));
}

/// The web writes buffers through the queue, ahead of the whole submit; the
/// frame graph decides which writes may land so.
#[test]
fn buffer_writes_are_owned_by_frame_graph_executor() {
    assert_frame_graph_owns_calls(&[".write_buffer("], None);
}

#[test]
fn wgpu_renderer_matches_shared_render_contracts() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping shared render contract assertions because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    for case in ALL_SHARED_RENDER_CASES {
        let mut frames = Vec::new();
        for fixture in case.fixtures() {
            renderer.scene_mut().graph = Some(fixture.graph);
            let frame = renderer
                .capture_frame(fixture.width, fixture.height)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to capture shared render case {}: {err:?}",
                        case.name()
                    )
                });
            frames.push(RenderedFrame {
                width: frame.width,
                height: frame.height,
                pixels: frame.pixels,
                normalized_rect: fixture.normalized_rect,
            });
        }
        case.assert_frames(&frames);
    }
}

#[test]
fn wgpu_renderer_matches_shared_stroke_and_arc_contracts() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!(
                "skipping shared stroke/arc contract assertions because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    for case in [
        cranpose_render_common::render_contract::SharedRenderCase::StrokedRoundRect,
        cranpose_render_common::render_contract::SharedRenderCase::AnnularSector,
    ] {
        let mut frames = Vec::new();
        for fixture in case.fixtures() {
            renderer.scene_mut().graph = Some(fixture.graph);
            let frame = renderer
                .capture_frame(fixture.width, fixture.height)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to capture shared render case {}: {err:?}",
                        case.name()
                    )
                });
            frames.push(RenderedFrame {
                width: frame.width,
                height: frame.height,
                pixels: frame.pixels,
                normalized_rect: fixture.normalized_rect,
            });
        }
        case.assert_frames(&frames);
    }
}
