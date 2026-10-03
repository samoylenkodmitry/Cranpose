use std::{hint::black_box, time::Instant};

use cranpose_core::{
    DefaultScheduler, OwnedMutableState, Runtime, SnapshotStateObserver, scheduler_ref,
};

const FRAMES: usize = 512;
const SCOPES_PER_ROW: usize = 4;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Scope {
    row: usize,
    command: usize,
}

fn run_case(rows: usize, retire: bool) {
    let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
    let state = OwnedMutableState::with_runtime(0, runtime.handle());
    let observer = SnapshotStateObserver::new(|callback| callback());
    let register_row = |row| {
        for command in 0..SCOPES_PER_ROW {
            observer.observe_reads(Scope { row, command }, |_| {}, || black_box(state.get()));
        }
    };
    for row in 0..rows {
        register_row(row);
    }
    let start = Instant::now();
    for frame in 0..FRAMES {
        let row = if retire { frame % rows } else { rows };
        observer.clear_if(|scope| scope.downcast_ref::<Scope>().is_some_and(|s| s.row == row));
        if retire {
            register_row(row);
        }
    }
    let elapsed = start.elapsed().as_nanos();
    println!(
        "{{\"benchmark\":\"observation_cleanup\",\"rows\":{rows},\"scopes_per_row\":{SCOPES_PER_ROW},\"retire\":{retire},\"frames\":{FRAMES},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
        elapsed / FRAMES as u128,
    );
}

fn main() {
    for rows in [32, 128, 512] {
        for retire in [false, true] {
            run_case(rows, retire);
        }
    }
}
