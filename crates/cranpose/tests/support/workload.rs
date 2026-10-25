//! A status area that does what common screens do between frames: readings
//! that change at their own rates, a panel that comes and goes, a button
//! whose label counts and a list that scrolls by itself. The ESP32 demo's
//! `workload` feature shows it for the profiles the IRAM lists come from, so
//! the lists hold the code of more screens than the benchmarks give, and the
//! memory tests measure what it keeps. It runs on frame time, so a test that
//! steps frames sees it change as a board does.

use std::time::Duration;

use cranpose::prelude::*;
use cranpose_core::{LaunchedEffectAsync, MutableState, rememberMutableStateOf};

/// A state that counts the `period`s of frame time since its first frame.
#[composable]
fn ticker(period: Duration) -> MutableState<u32> {
    let count = rememberMutableStateOf(|| 0u32);
    LaunchedEffectAsync(period, move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let start = clock.next_frame().await;
            let period = period.as_nanos().max(1);
            loop {
                let now = clock.next_frame().await;
                let ticks = (u128::from(now.saturating_sub(start)) / period) as u32;
                if ticks != count.get() {
                    count.set(ticks);
                }
            }
        })
    });
    count
}

#[composable]
fn Reading(label: &'static str, period: Duration, style: TextStyle) {
    let value = ticker(period);
    Text(
        format!("{label} {}", value.get()),
        Modifier::empty().weight(1.0),
        style,
    );
}

/// The rows scroll forward a little every frame, as a drag up does, and
/// start over at the end.
#[composable]
fn ScrollingRows(style: TextStyle) {
    let state = rememberLazyListState();
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            loop {
                clock.next_frame().await;
                if state.can_scroll_forward() {
                    state.dispatch_scroll_delta(-2.0);
                } else {
                    state.scroll_to_item(0, 0.0);
                }
            }
        })
    });
    LazyColumn(
        Modifier::empty().fill_max_width().height(44.0),
        state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(LazyItems::new(40), move |index| {
                Text(
                    format!("Row {index}"),
                    Modifier::empty().padding(2.0),
                    style.clone(),
                );
            });
        },
    );
}

#[composable]
pub fn Workload(style: TextStyle) {
    let shown = ticker(Duration::from_millis(1500));
    let taps = ticker(Duration::from_millis(700));
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            let row_style = style.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                move || {
                    Reading("T", Duration::from_millis(200), row_style.clone());
                    Reading("H", Duration::from_millis(330), row_style.clone());
                    Reading("P", Duration::from_millis(500), row_style.clone());
                },
            );
            ScrollingRows(style.clone());
            let button_style = style.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                move || {
                    let label_style = button_style.clone();
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        || {},
                        move || {
                            Text(
                                format!("Count {}", taps.get()),
                                Modifier::empty(),
                                label_style.clone(),
                            );
                        },
                    );
                    let panel_style = button_style.clone();
                    AnimatedVisibility(
                        shown.get().is_multiple_of(2),
                        fade_in(),
                        fade_out(),
                        move || {
                            Text(
                                "Panel shown",
                                Modifier::empty().padding(4.0),
                                panel_style.clone(),
                            );
                        },
                    );
                },
            );
        },
    );
}
