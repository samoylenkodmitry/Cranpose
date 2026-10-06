//! What a frame costs when a few texts deep in a screen change and keep
//! their width: `ROWS` rows of eight texts under a `BoxWithConstraints`,
//! where the first `CHANGED` rows change their texts each frame. Each frame
//! recomposes and lays the screen out. Run it under `/usr/bin/time -l` with
//! `FRAMES=0` and `FRAMES=1000` and divide the difference in retired
//! instructions by 1000.

use std::rc::Rc;

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};
use cranpose_ui::{
    AppContext, BoxWithConstraints, Column, ColumnSpec, Modifier, Row, RowSpec, Size, Text,
    TextStyle, composable,
    layout::{MeasureLayoutOptions, measure_layout_with_options},
};

#[composable]
fn Screen(tick: MutableState<usize>, rows: usize, changed: usize) {
    BoxWithConstraints(Modifier::empty().fill_max_size(), move |_scope| {
        Column(
            Modifier::empty().fill_max_width(),
            ColumnSpec::default(),
            move || {
                for row in 0..rows {
                    BenchRow(tick, row < changed);
                }
            },
        );
    });
}

#[composable]
fn BenchRow(tick: MutableState<usize>, changes: bool) {
    let value = if changes { tick.get() } else { 0 };
    Row(
        Modifier::empty().fill_max_width().padding(2.0),
        RowSpec::default(),
        move || {
            for cell in 0..8 {
                Text(
                    format!("{:>4}", (value + cell) % 1000),
                    Modifier::empty().weight(1.0),
                    TextStyle::default(),
                );
            }
        },
    );
}

fn env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let (rows, changed, frames) = (env("ROWS", 100), env("CHANGED", 10), env("FRAMES", 500));
    let app_context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let tick = MutableState::with_runtime(0usize, composition.runtime_handle());
    let layout = |composition: &mut Composition<MemoryApplier>| {
        let root = composition.root().expect("composition root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        measure_layout_with_options(
            &mut applier,
            root,
            Size::new(1080.0, 1920.0),
            MeasureLayoutOptions {
                collect_semantics: false,
                build_layout_tree: false,
            },
        )
        .expect("layout pass");
        applier.clear_runtime_handle();
    };
    Rc::clone(&app_context).enter(|| {
        composition
            .render(location_key(file!(), line!(), column!()), || {
                Screen(tick, rows, changed);
            })
            .expect("initial composition");
        layout(&mut composition);
        for frame in 1..=frames {
            tick.set(frame);
            while composition.process_invalid_scopes().expect("recomposition") {}
            layout(&mut composition);
        }
    });
    println!("rows {rows} changed {changed} frames {frames}");
}
