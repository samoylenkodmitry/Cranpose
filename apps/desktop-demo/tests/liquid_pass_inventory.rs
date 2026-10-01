use cranpose_render_wgpu::RenderStatsSnapshot;
use liquid_page_support::LiquidPage;

use crate::liquid_page_support;

const LOGICAL: (u32, u32) = (393, 816);
const DENSITY: f32 = 2.75;

#[test]
fn scrolling_liquid_cards_adds_no_blur_passes_over_the_existing_page_budget() {
    let Some(mut page) = LiquidPage::open("Liquid Card Cache Test Device", LOGICAL, DENSITY) else {
        return;
    };
    page.scroll_to(700.0);
    for _ in 0..10 {
        page.capture();
    }
    for step in 0..8 {
        page.step_one_pixel();
        let cached = page.capture();
        let stats = page.stats();
        assert!(
            stats.blur_passes <= 4,
            "scroll step {step} re-blurred an unchanged card: {stats:?}"
        );
        assert!(
            stats.pass_count <= 17,
            "scroll step {step} added a pass for unchanged controls: {stats:?}"
        );
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", Some("1"));
        let uncached = page.capture();
        cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_CACHE", None);
        if cached.pixels != uncached.pixels {
            let output = std::path::Path::new("target/liquid-cache-parity");
            std::fs::create_dir_all(output).expect("create cache comparison directory");
            for (name, frame) in [("cached", &cached), ("uncached", &uncached)] {
                image::save_buffer(
                    output.join(format!("step-{step}-{name}.png")),
                    &frame.pixels,
                    frame.width,
                    frame.height,
                    image::ColorType::Rgba8,
                )
                .expect("save cache comparison");
            }
        }
        assert!(
            cached.pixels == uncached.pixels,
            "scroll step {step} changed pixels when reusing a backdrop"
        );
        page.capture();
    }
    assert_eq!(page.device_errors(), 0);
}

fn report(label: &str, stats: &RenderStatsSnapshot) {
    eprintln!(
        "[inventory] {label}: passes={} pass_px={} blur_passes={} blur_px={} stages={} \
         backdrop_admissions={} prefix_admissions={} substrates={} composites={} effects={} \
         shader_px={} glass_px={} isolated={} isolated_px={} layer_cache_hits={} misses={} \
         shape_passes={} shape_fill_px={} text_passes={} image_passes={} copies={} copy_px={} \
         uploads={}B/{} pipelines={}",
        stats.pass_count,
        stats.pass_pixels,
        stats.blur_passes,
        stats.blur_pixels,
        stats.stages,
        stats.backdrop_admissions,
        stats.prefix_admissions,
        stats.substrates,
        stats.composite_passes,
        stats.effect_applies,
        stats.shader_pixels,
        stats.glass_rasterized_pixels,
        stats.isolated_layer_renders,
        stats.isolated_layer_pixels,
        stats.layer_cache_hits,
        stats.layer_cache_misses,
        stats.shape_passes,
        stats.shape_fill_pixels,
        stats.text_passes,
        stats.image_passes,
        stats.copy_count,
        stats.copy_pixels,
        stats.upload_bytes,
        stats.upload_writes,
        stats.pipelines_created,
    );
    for layer in stats.top_isolated_layers.iter().flatten() {
        eprintln!(
            "[inventory]   isolated layer {}x{} px at {:?}",
            layer.width, layer.height, layer.logical_rect
        );
    }
}

/// The per-frame pass inventory of the demo's Liquid tab at the Mate 20 X's
/// geometry: a rest frame, two scroll offsets and three one-pixel steps.
#[test]
fn liquid_page_pass_inventory() {
    #[cfg(feature = "logging")]
    let _ = env_logger::try_init();
    let Some(mut page) = LiquidPage::open("Liquid Pass Inventory Test Device", LOGICAL, DENSITY)
    else {
        eprintln!("skipping liquid pass inventory: no headless GPU");
        return;
    };
    for _ in 0..8 {
        page.capture();
    }
    page.capture();
    report("rest", &page.stats());
    for (label, offset) in [("offset 300", 300.0), ("offset 700", 700.0)] {
        page.scroll_to(offset);
        page.capture();
        page.capture();
        report(label, &page.stats());
    }
    for step in 1..=3 {
        page.step_one_pixel();
        page.capture();
        report(&format!("one pixel step {step}"), &page.stats());
    }
    assert_eq!(page.device_errors(), 0);
    if std::env::var_os("CRANPOSE_GPU_PASS_TIMING").is_some() {
        for step in 0..360 {
            page.scroll_to(700.0 + (step % 16) as f32 / DENSITY);
            page.capture();
        }
        let timing = page.shell.renderer().gpu_pass_timings();
        assert!(timing.frames > 0, "the adapter must expose GPU timestamps");
        assert_eq!(page.device_errors(), 0);
    }
}
