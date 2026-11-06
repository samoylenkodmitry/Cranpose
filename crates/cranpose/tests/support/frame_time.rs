// The milliseconds since a benchmark screen's first frame. Every value on
// such a screen is a function of them, so each app that builds the screen
// shows the same picture at the same time.

use cranpose::prelude::*;
use cranpose_core::{LaunchedEffect, MutableState, rememberMutableStateOf};

// The milliseconds since the first frame, read where a value changes.
pub(super) type Time = MutableState<u64>;

// The time, set as each frame starts; `on_frame` then takes it.
#[composable]
pub(super) fn frame_time(on_frame: impl Fn(u64) + 'static) -> Time {
    let time = rememberMutableStateOf(|| 0u64);
    LaunchedEffect((), async move |scope| {
        let clock = scope.runtime().frame_clock();
        let start = clock.next_frame().await;
        loop {
            let now = clock.next_frame().await;
            let t = now.saturating_sub(start) / 1_000_000;
            time.set(t);
            on_frame(t);
        }
    });
    time
}
