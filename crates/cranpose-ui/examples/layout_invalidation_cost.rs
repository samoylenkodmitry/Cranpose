//! What does invalidating the whole layout cache cost?
//!
//! `measure_layout` mints a new global cache epoch whenever the ROOT reports
//! `needs_measure`, and `bubble_measure_dirty` walks every mark up to the root
//! — so one node needing measure anywhere makes every node's cached
//! measurement stale at once. Counted on a desktop scroll: 23,231 full
//! invalidations against 23,296 working frames. Essentially every frame that
//! does any work discards the whole tree's layout cache.
//!
//! Whether that is expensive is a different question, and the evidence is
//! against it: a viewport sweep on another screen changed the number of rows
//! laid out roughly tenfold and moved layout time not at all. So this prices
//! the invalidation directly — the same tree measured with the cache warm and
//! with it thrown away, across a range of sizes.
//!
//! ```sh
//! cargo run --release -p cranpose-ui --example layout_invalidation_cost
//! ```
//!
//! Read the SLOPE against node count, not the absolute times. If the dirty arm
//! grows with the tree and the clean arm does not, the epoch is an O(nodes)
//! per-frame term and scoping it is worth building. If both are flat, or the
//! gap does not grow, re-measuring an unchanged node is already cheap and the
//! epoch is a curiosity.

use std::time::{Duration, Instant};

use cranpose_ui::{
    Color, Column, ColumnSpec, Modifier, Size, TextStyle,
    layout::{MeasureLayoutOptions, measure_layout_with_options},
    widgets::{Box as UiBox, BoxSpec, Text},
};

const NODE_COUNTS: &[usize] = &[25, 50, 100, 200, 400, 800];
const WARMUP: usize = 5;
const ITERATIONS: usize = 40;

fn median(mut values: Vec<Duration>) -> f64 {
    values.sort_unstable();
    values[values.len() / 2].as_secs_f64() * 1000.0
}

/// Least-squares slope of (nodes, ms).
fn slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    let sum_x: f64 = points.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = points.iter().map(|(_, y)| y).sum();
    let sum_xx: f64 = points.iter().map(|(x, _)| x * x).sum();
    let sum_xy: f64 = points.iter().map(|(x, y)| x * y).sum();
    (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x)
}

fn main() {
    println!("rows   clean_ms   dirty_ms   invalidation_ms   per_node_us");
    let mut clean_points = Vec::new();
    let mut dirty_points = Vec::new();

    for &rows in NODE_COUNTS {
        let mut composition = cranpose_ui::run_test_composition(move || {
            Column(
                Modifier::empty().size_points(390.0, 844.0),
                ColumnSpec::default(),
                move || {
                    for index in 0..rows {
                        UiBox(
                            Modifier::empty()
                                .size_points(390.0, 72.0)
                                .background(Color(0.2, 0.3, 0.5, 1.0)),
                            BoxSpec::default(),
                            move || {
                                Text(
                                    format!("row {index}"),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    }
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 390.0,
            height: 844.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);

        // PRODUCTION CONFIGURATION -- see the note on the clean arm below.
        let bare = MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: false,
        };

        // Warm: the first measure builds every cache entry.
        for _ in 0..WARMUP {
            measure_layout_with_options(&mut applier, root, viewport, bare).expect("warm");
        }

        // The shell calls measure_layout_with_options with BOTH flags false --
        // it builds neither the layout tree nor the semantics tree during the
        // frame's layout phase. `compute_layout`, the LayoutEngine convenience
        // method, defaults both to true, so pricing through it measures a path
        // no frame takes: semantics construction alone is 70-79% of that call.
        let mut cleans = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            let start = Instant::now();
            measure_layout_with_options(&mut applier, root, viewport, bare).expect("clean measure");
            cleans.push(start.elapsed());
        }

        // Dirty arm: mark ONE leaf as needing measure. It bubbles to the root,
        // the root mints a fresh epoch, and the whole tree's cache goes stale
        // — which is what happens on essentially every real working frame.
        let mut dirties = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            cranpose_core::bubble_measure_dirty(&mut *applier, root);
            let start = Instant::now();
            measure_layout_with_options(&mut applier, root, viewport, bare).expect("dirty measure");
            dirties.push(start.elapsed());
        }
        applier.clear_runtime_handle();

        let (clean, dirty) = (median(cleans), median(dirties));
        let delta = dirty - clean;
        println!(
            "{rows:>4}   {clean:>8.3}   {dirty:>8.3}   {delta:>15.3}   {:>11.3}",
            delta * 1000.0 / rows as f64
        );
        clean_points.push((rows as f64, clean));
        dirty_points.push((rows as f64, dirty));
    }

    let clean_slope = slope(&clean_points);
    let dirty_slope = slope(&dirty_points);
    println!("\nslope against row count:");
    println!("  cache warm  {:>8.2} us per row", clean_slope * 1000.0);
    println!("  cache dumped{:>8.2} us per row", dirty_slope * 1000.0);
    println!(
        "  difference  {:>8.2} us per row",
        (dirty_slope - clean_slope) * 1000.0
    );
    println!(
        "\nThe difference is what a full invalidation costs per node in the tree.\n\
         Multiply by the node count of a real screen to size scoping the epoch."
    );
}
