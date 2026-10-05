//! A keyed column that gains rows at its top every frame and drops as many
//! at its bottom, as a trade tape does, with more content after it in the
//! slot table. Each new row inserts its groups before every later group.

use std::{hint::black_box, time::Instant};

use cranpose_core::{Composition, MemoryApplier, MutableState, key, location_key, remember};
use cranpose_macros::composable;

const ROWS: usize = 40;
const NEW_ROWS_PER_FRAME: usize = 2;
const TRAILING_CELLS: usize = 400;
const WARMUP_FRAMES: usize = 64;
const FRAMES: usize = 4096;

#[composable]
fn Tape(newest: MutableState<usize>) {
    let newest = newest.get();
    for id in (newest.saturating_sub(ROWS - 1)..=newest).rev() {
        key(id, || Trade(id));
    }
}

#[composable]
fn Trade(id: usize) {
    let price = remember(|| id * 3).with(|price| *price);
    Cell(price);
    Cell(id);
}

#[composable]
fn Cell(value: usize) {
    black_box(remember(|| value).with(|value| *value));
}

#[composable]
fn Trailing() {
    for index in 0..TRAILING_CELLS {
        Cell(index);
    }
}

fn main() {
    let mut composition = Composition::new(MemoryApplier::new());
    let newest = MutableState::with_runtime(ROWS, composition.runtime_handle());
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Tape(newest);
            Trailing();
        })
        .expect("initial composition");
    let mut next = ROWS;
    let mut frame = || {
        next += NEW_ROWS_PER_FRAME;
        newest.set(next);
        composition
            .process_invalid_scopes()
            .expect("tape recomposition");
    };
    for _ in 0..WARMUP_FRAMES {
        frame();
    }
    let start = Instant::now();
    for _ in 0..FRAMES {
        frame();
    }
    let elapsed = start.elapsed().as_nanos();
    println!(
        "{{\"benchmark\":\"keyed_front_inserts\",\"rows\":{ROWS},\"new_rows_per_frame\":{NEW_ROWS_PER_FRAME},\"trailing_cells\":{TRAILING_CELLS},\"frames\":{FRAMES},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
        elapsed / FRAMES as u128,
    );
}
