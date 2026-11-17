use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::{RenderStatsSnapshot, WgpuRenderer};
use desktop_app::app::{self, DemoTab, StartupSelection};

use crate::liquid_page_support::headless_renderer;

const LOGICAL: (u32, u32) = (1200, 800);

fn explainer() -> Option<AppShell<WgpuRenderer>> {
    let renderer = headless_renderer("Explainer Paint Test Device")?;
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    let mut shell = AppShell::new_with_size_and_density(
        renderer,
        root_key,
        || {
            app::combined_app_with_startup(StartupSelection {
                initial_tab: Some(DemoTab::Explainer),
                initial_shader_section: None,
            });
        },
        LOGICAL,
        (LOGICAL.0 as f32, LOGICAL.1 as f32),
        1.0,
    );
    shell.update();
    Some(shell)
}

fn frame(shell: &mut AppShell<WgpuRenderer>) -> (Vec<u8>, RenderStatsSnapshot) {
    shell.update();
    let pixels = shell
        .renderer()
        .capture_frame_with_scale(LOGICAL.0, LOGICAL.1, 1.0)
        .expect("the explainer draws a frame")
        .pixels;
    let stats = shell
        .renderer()
        .last_frame_stats()
        .expect("the renderer publishes frame statistics");
    (pixels, stats)
}

// The title page is an oil painting whose strokes are worked out from the
// page: a frame in which only the lamp leans after the pointer lights the
// painting again and paints none of it.
#[test]
fn a_leaning_lamp_lights_the_painting_without_painting_it_again() {
    let Some(mut shell) = explainer() else {
        eprintln!("skipping the explainer's lamp test: no headless GPU");
        return;
    };
    let mut rest = Vec::new();
    for _ in 0..12 {
        rest = frame(&mut shell).0;
    }
    let window = u64::from(LOGICAL.0 * LOGICAL.1);
    let mut leaned = Vec::new();
    for step in 0..12 {
        let x = 700.0 + 30.0 * step as f32;
        shell.set_cursor(x, 200.0 + 10.0 * step as f32);
        #[expect(
            clippy::disallowed_methods,
            reason = "the shell takes its frame time from the wall clock, so a pointer move needs time before its frame"
        )]
        std::thread::sleep(std::time::Duration::from_millis(30));
        let (pixels, stats) = frame(&mut shell);
        leaned = pixels;
        eprintln!(
            "[lamp] step {step}: shader px {} of {window}, isolated {}, effects {}, cache hits {} misses {}",
            stats.shader_pixels,
            stats.isolated_layer_renders,
            stats.effect_applies,
            stats.layer_cache_hits,
            stats.layer_cache_misses,
        );
        for layer in stats.top_isolated_layers() {
            eprintln!(
                "[lamp]   isolated {:?} {}x{} at {:?}",
                layer.node_id, layer.width, layer.height, layer.logical_rect
            );
        }
        if step >= 3 {
            assert_eq!(
                stats.isolated_layer_renders, 0,
                "step {step}: the lamp's move drew the painted page again"
            );
            assert!(
                stats.shader_pixels <= window,
                "step {step}: the lamp's move ran the strokes again"
            );
        }
    }
    let relit = rest
        .as_chunks::<4>()
        .0
        .iter()
        .zip(leaned.as_chunks::<4>().0)
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 12))
        .count();
    eprintln!("[lamp] the lean relit {relit} pixels");
    assert!(relit > 20_000, "the lamp did not lean after the pointer");
}
