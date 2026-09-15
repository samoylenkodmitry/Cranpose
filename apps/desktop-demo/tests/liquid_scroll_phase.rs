pub mod liquid_page_support;

use liquid_page_support::LiquidPage;

const LOGICAL: (u32, u32) = (800, 600);
const DENSITY: f32 = 130.0 / 96.0;
const BOTTOM_BAR_SCENE_OFFSET: f32 = 1423.154;
const PHYSICAL_PIXEL_STEPS: u32 = 10;
const EDGE_TRIM_LOGICAL: u32 = 180;
const MAX_CHANNEL_DELTA: u32 = 1;
const MAX_DIFFERING_PIXELS: u32 = 48;

fn physical(logical: u32) -> u32 {
    liquid_page_support::physical(logical, DENSITY)
}

struct RasterDrift {
    differing_pixels: u32,
    worst_channel_delta: u32,
}

fn raster_drift(reference: &[u8], moved: &[u8]) -> RasterDrift {
    let mut drift = RasterDrift {
        differing_pixels: 0,
        worst_channel_delta: 0,
    };
    for (lhs, rhs) in reference
        .as_chunks::<4>()
        .0
        .iter()
        .zip(moved.as_chunks::<4>().0)
    {
        let delta = lhs
            .iter()
            .zip(rhs)
            .map(|(left, right)| u32::from(left.abs_diff(*right)))
            .max()
            .unwrap_or(0);
        if delta > 0 {
            drift.differing_pixels += 1;
            drift.worst_channel_delta = drift.worst_channel_delta.max(delta);
        }
    }
    drift
}

fn rows(frame: &[u8], first_row: u32, row_count: u32) -> &[u8] {
    let stride = (physical(LOGICAL.0) * 4) as usize;
    let start = first_row as usize * stride;
    &frame[start..start + row_count as usize * stride]
}

#[test]
fn the_liquid_page_at_the_bottom_bar_stays_exact_across_one_physical_pixel_scrolls() {
    let Some(mut page) = LiquidPage::open("Liquid Scroll Phase Test Device", LOGICAL, DENSITY)
    else {
        eprintln!("skipping liquid scroll phase assertions: no headless GPU");
        return;
    };
    page.scroll_to(BOTTOM_BAR_SCENE_OFFSET);
    page.shell.update();
    let mut previous = page.capture().pixels;
    let edge = physical(EDGE_TRIM_LOGICAL);
    let compared_rows = physical(LOGICAL.1) - edge * 2 - 1;
    let mut failures = Vec::new();
    for step in 1..=PHYSICAL_PIXEL_STEPS {
        let consumed = page.step_one_pixel();
        assert!(
            (consumed - 1.0 / DENSITY).abs() < 1e-4,
            "step {step}: the page must consume one physical pixel, consumed {consumed}"
        );
        let current = page.capture().pixels;
        let drift = raster_drift(
            rows(&previous, edge + 1, compared_rows),
            rows(&current, edge, compared_rows),
        );
        eprintln!(
            "step {step}: offset={:.3} differing={} worst_channel_delta={}",
            page.scroll_offset(),
            drift.differing_pixels,
            drift.worst_channel_delta
        );
        if drift.worst_channel_delta > MAX_CHANNEL_DELTA
            || drift.differing_pixels > MAX_DIFFERING_PIXELS
        {
            failures.push(format!(
                "step {step}: differing={} worst_channel_delta={}",
                drift.differing_pixels, drift.worst_channel_delta
            ));
        }
        previous = current;
    }
    assert_eq!(page.device_errors(), 0);
    assert!(
        failures.is_empty(),
        "the liquid page scrolled by one physical pixel must move as one raster: {}",
        failures.join("; ")
    );
}
