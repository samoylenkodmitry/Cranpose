// A lazy column whose items move to other indices scrolls back over them:
// every row on screen moves by the scroll and no further. A log that keeps
// its newest lines moves its items so, as it drops the oldest ones before
// the viewport. A size the list learned for an index holds only for the
// item it measured there.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{Box, BoxSpec, LazyColumn, LazyColumnSpec, LinearArrangement, Modifier};

const WIDTH: u16 = 200;
const HEIGHT: u16 = 300;
const STEP: f32 = 8.0;

// Rows of four heights, so a row and the one three before it differ.
fn row_height(id: u64) -> f32 {
    40.0 + (id % 4) as f32 * 30.0
}

struct Log {
    shell: AppShell<TestRenderer>,
    state: Rc<Cell<Option<LazyListState>>>,
    ids: MutableState<Vec<u64>>,
}

impl Log {
    // A column of the rows `first..first + count`, spaced by 8.
    fn new(first: u64, count: u64) -> Self {
        let state = Rc::new(Cell::new(None));
        let captured = Rc::clone(&state);
        let ids = Rc::new(Cell::new(None));
        let captured_ids = Rc::clone(&ids);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let list_state = rememberLazyListState();
                captured.set(Some(list_state));
                let rows = rememberMutableStateOf(|| (first..first + count).collect::<Vec<u64>>());
                captured_ids.set(Some(rows));
                let shown = rows.get();
                LazyColumn(
                    Modifier::empty().size_points(f32::from(WIDTH), f32::from(HEIGHT)),
                    list_state,
                    LazyColumnSpec::default()
                        .vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                    move |scope| {
                        for &id in &shown {
                            scope.item_keyed(Some(id), None, move || {
                                Box(
                                    Modifier::empty().fill_max_width().height(row_height(id)),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        }
                    },
                );
            },
        );
        shell.set_viewport(f32::from(WIDTH), f32::from(HEIGHT));
        shell.set_buffer_size(u32::from(WIDTH), u32::from(HEIGHT));
        shell.update();
        let ids = ids.get().expect("composed log");
        Self { shell, state, ids }
    }

    fn state(&self) -> LazyListState {
        self.state.get().expect("composed list")
    }

    // The rows on screen: their keys and where they start.
    fn rows(&self) -> Vec<(u64, f32)> {
        self.state()
            .with_layout_info(|info| {
                info.visible_items_info
                    .iter()
                    .map(|item| (item.key, item.offset))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn scroll_by(&mut self, delta: f32) {
        self.state().dispatch_scroll_delta(delta);
        self.shell.update();
    }

    // Drops the `dropped` oldest rows and adds as many new ones at the end.
    fn shift(&mut self, dropped: usize) {
        let mut ids = self.ids.get_non_reactive();
        let next = ids.last().map_or(0, |last| last + 1);
        ids.drain(..dropped);
        ids.extend(next..next + dropped as u64);
        self.ids.set(ids);
        self.shell.update();
    }

    // Scrolls back by `STEP` and returns how far the rows on screen moved
    // beyond the scroll, or `None` once the list rests at its start.
    fn step_back(&mut self) -> Option<f32> {
        let before = self.rows();
        self.scroll_by(STEP);
        if !self.state().can_scroll_backward_non_reactive() {
            return None;
        }
        let moved = self.rows().iter().find_map(|(key, offset)| {
            before
                .iter()
                .find(|(was_key, _)| was_key == key)
                .map(|(_, was)| offset - was)
        });
        // No row that stays on screen moved further than the viewport.
        Some(moved.map_or(f32::INFINITY, |moved| (moved - STEP).abs()))
    }
}

// A log of 30 rows scrolled down past its first eight, whose first rows
// were measured at indices 0 to 4.
fn scrolled_log() -> Log {
    let mut log = Log::new(0, 30);
    for _ in 0..150 {
        log.scroll_by(-STEP);
    }
    assert!(
        log.state().first_visible_item_index_non_reactive() > 8,
        "the log did not scroll past its first eight rows"
    );
    log
}

#[test]
fn rows_move_with_the_scroll_after_the_rows_before_them_left() {
    let mut log = scrolled_log();
    // The three oldest rows go: every row now holds the index of the row
    // three before it, whose size differs.
    log.shift(3);
    let mut worst = 0.0f32;
    for step in 0.. {
        assert!(step < 400, "the list never reached its start");
        let Some(jump) = log.step_back() else { break };
        worst = worst.max(jump);
    }
    assert!(
        worst < 0.5,
        "a row jumped {worst} px beyond the scroll while the list scrolled back"
    );
}

#[test]
fn rows_move_with_the_scroll_when_the_rows_before_them_leave_on_the_way() {
    let mut log = scrolled_log();
    let mut worst = 0.0f32;
    // A row leaves every few steps while the list scrolls back, as lines of
    // a live session do.
    for step in 0.. {
        assert!(step < 400, "the list never reached its start");
        if step % 5 == 0 {
            log.shift(1);
        }
        let Some(jump) = log.step_back() else { break };
        worst = worst.max(jump);
    }
    assert!(
        worst < 0.5,
        "a row jumped {worst} px beyond the scroll while rows before it left"
    );
}
