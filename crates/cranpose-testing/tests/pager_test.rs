//! A pager comes to rest on a page when the user lets go: the next page past
//! half a page or after a flick, the same page short of that, and one page
//! at a time however fast the fling, through the shell's pointer input.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::{robot::TestRenderer, robot_assertions::find_semantics_node};
use cranpose_ui::{
    Box, BoxSpec, HorizontalPager, Modifier, PaddingValues, PageSize, PagerSpec, PagerState,
    VerticalPager, composable, rememberPagerState,
};

/// A composition counter that stays equal to itself while it counts, so it
/// does not make its reader recompose.
#[derive(Clone, Default)]
struct Reads(Rc<Cell<usize>>);

impl PartialEq for Reads {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[composable]
fn SettledReader(pager: PagerState, reads: Reads) {
    let _ = pager.settled_page();
    reads.0.set(reads.0.get() + 1);
}

const WIDTH: f32 = 300.0;
const HEIGHT: f32 = 200.0;
const PAGES: usize = 5;

/// The point at `main` along the pager's axis, in the middle of the other.
fn point(horizontal: bool, main: f32) -> (f32, f32) {
    if horizontal {
        (main, HEIGHT / 2.0)
    } else {
        (WIDTH / 2.0, main)
    }
}

struct Pager {
    shell: AppShell<TestRenderer>,
    state: Rc<Cell<Option<PagerState>>>,
    now_ms: i64,
    frame_ns: u64,
}

impl Pager {
    fn new(horizontal: bool) -> Self {
        Self::with(horizontal, PagerSpec::default())
    }

    fn with(horizontal: bool, spec: PagerSpec) -> Self {
        Self::reading(horizontal, spec, Reads::default())
    }

    /// A pager beside a composable that reads its settled page and counts
    /// its compositions in `settled_reads`.
    fn reading(horizontal: bool, spec: PagerSpec, settled_reads: Reads) -> Self {
        let state = Rc::new(Cell::new(None));
        let captured = Rc::clone(&state);
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let pager = rememberPagerState(0);
                captured.set(Some(pager));
                let size = Modifier::empty().size_points(WIDTH, HEIGHT);
                let page = |page: usize| {
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .content_description(format!("page {page}")),
                        BoxSpec::default(),
                        || {},
                    );
                };
                if horizontal {
                    HorizontalPager(size, pager, spec, PAGES, page);
                } else {
                    VerticalPager(size, pager, spec, PAGES, page);
                }
                SettledReader(pager, settled_reads.clone());
            },
        );
        shell.set_viewport(WIDTH, HEIGHT);
        shell.set_buffer_size(WIDTH as u32, HEIGHT as u32);
        shell.set_semantics_enabled(true);
        let mut pager = Self {
            shell,
            state,
            now_ms: 1_000_000,
            frame_ns: 16_000_000,
        };
        pager.frames(2);
        pager
    }

    fn state(&self) -> PagerState {
        self.state.get().expect("composed pager")
    }

    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.frame_ns += 16_000_000;
            self.shell.update_at_frame_time_nanos(self.frame_ns);
        }
    }

    /// Drags along the main axis from `from` to `to` in `millis`, lets go,
    /// and runs the frames the settle takes.
    fn drag(&mut self, horizontal: bool, from: f32, to: f32, millis: i64) {
        self.press_and_move(horizontal, from, to, millis);
        self.release(horizontal, to);
        self.frames(90);
    }

    /// Presses at `from` and moves to `to` in `millis` without letting go.
    fn press_and_move(&mut self, horizontal: bool, from: f32, to: f32, millis: i64) {
        let (x, y) = point(horizontal, from);
        self.shell.set_cursor_at_time(x, y, Some(self.now_ms));
        self.shell.pointer_pressed_at_time(Some(self.now_ms));
        let steps = 10;
        for step in 1..=steps {
            self.now_ms += millis / steps;
            let (x, y) = point(horizontal, from + (to - from) * step as f32 / steps as f32);
            self.shell.set_cursor_at_time(x, y, Some(self.now_ms));
            self.frames(1);
        }
    }

    fn release(&mut self, horizontal: bool, at: f32) {
        self.now_ms += 8;
        let (x, y) = point(horizontal, at);
        self.shell
            .pointer_released_at_position_time(x, y, Some(self.now_ms));
    }

    /// Where page `page` starts along the main axis, or `None` when it is
    /// not placed.
    fn start(&mut self, horizontal: bool, page: usize) -> Option<f32> {
        let tree = self.shell.semantics_tree().expect("semantics");
        let name = format!("page {page}");
        find_semantics_node(tree.root(), &|node| {
            node.description.as_deref() == Some(name.as_str())
        })
        .map(|node| {
            if horizontal {
                node.bounds.x
            } else {
                node.bounds.y
            }
        })
    }

    /// Runs a screen reader's page action, which scrolls most of a page,
    /// and the frames that follow; answers whether the pager took it.
    fn reader_page(&mut self, horizontal: bool, forward: bool) -> bool {
        let tree = self.shell.semantics_tree().expect("semantics");
        let scroll_by =
            find_semantics_node(tree.root(), &|node| node.details().scroll_by.is_some())
                .and_then(|node| node.details().scroll_by.clone())
                .expect("the pager scrolls for a reader");
        let main = if horizontal { WIDTH } else { HEIGHT };
        let step = if forward { 0.9 * main } else { -0.9 * main };
        let (dx, dy) = if horizontal { (step, 0.0) } else { (0.0, step) };
        let taken = self
            .shell
            .app_context()
            .enter(|| cranpose_core::run_in_mutable_snapshot(|| scroll_by.invoke(dx, dy)))
            .expect("reader action");
        self.frames(90);
        taken
    }

    fn rests_on(&mut self, horizontal: bool, page: usize) -> bool {
        self.state().current_page() == page
            && self
                .start(horizontal, page)
                .is_some_and(|start| start.abs() < 0.5)
    }
}

#[test]
fn a_slow_drag_past_half_a_page_turns_it_and_one_short_of_half_goes_back() {
    let mut pager = Pager::new(true);
    pager.drag(true, 250.0, 200.0, 1_000);
    assert!(pager.rests_on(true, 0), "a short slow drag goes back");
    pager.drag(true, 250.0, 80.0, 1_000);
    assert!(pager.rests_on(true, 1), "past half a page turns it");
}

#[test]
fn a_quick_flick_turns_the_page_however_short() {
    let mut pager = Pager::new(true);
    pager.drag(true, 250.0, 220.0, 30);
    assert!(pager.rests_on(true, 1), "a flick toward the next page");
    pager.drag(true, 80.0, 110.0, 30);
    assert!(pager.rests_on(true, 0), "a flick back");
}

#[test]
fn a_long_fast_fling_turns_one_page() {
    let mut pager = Pager::new(true);
    pager.drag(true, 290.0, 10.0, 40);
    assert!(pager.rests_on(true, 1));
}

#[test]
fn scroll_to_page_shows_the_page_and_a_mid_drag_reports_its_fraction() {
    let mut pager = Pager::new(true);
    pager.state().scroll_to_page(3);
    pager.frames(2);
    assert!(pager.rests_on(true, 3));
    pager
        .shell
        .set_cursor_at_time(250.0, 100.0, Some(pager.now_ms));
    pager.shell.pointer_pressed_at_time(Some(pager.now_ms));
    for x in [240.0, 220.0, 190.0] {
        pager.now_ms += 100;
        pager.shell.set_cursor_at_time(x, 100.0, Some(pager.now_ms));
        pager.frames(1);
    }
    let state = pager.state();
    assert_eq!(state.current_page(), 3);
    assert!(
        (state.current_page_offset_fraction() - 0.2).abs() < 0.05,
        "60 of 300 pixels is a fifth of a page, read {}",
        state.current_page_offset_fraction()
    );
}

#[test]
fn a_vertical_pager_turns_its_pages_up() {
    let mut pager = Pager::new(false);
    pager.drag(false, 180.0, 60.0, 1_000);
    assert!(pager.rests_on(false, 1));
}

#[test]
fn animate_scroll_to_page_springs_there_and_a_press_stops_it_where_it_is() {
    let mut pager = Pager::new(true);
    pager.state().animate_scroll_to_page(3);
    pager.frames(4);
    let moving = pager.state().lazy_list_state().first_visible_item_index();
    assert!(
        moving < 3,
        "the spring takes frames, at page {moving} after four"
    );
    pager.frames(120);
    assert!(pager.rests_on(true, 3));

    pager.state().animate_scroll_to_page(0);
    pager.frames(4);
    pager
        .shell
        .set_cursor_at_time(150.0, 100.0, Some(pager.now_ms));
    pager.shell.pointer_pressed_at_time(Some(pager.now_ms));
    let held = pager
        .state()
        .lazy_list_state()
        .first_visible_item_scroll_offset();
    let held_page = pager.state().lazy_list_state().first_visible_item_index();
    pager.frames(30);
    assert_eq!(
        (
            pager.state().lazy_list_state().first_visible_item_index(),
            pager
                .state()
                .lazy_list_state()
                .first_visible_item_scroll_offset()
        ),
        (held_page, held),
        "the press stopped the spring"
    );
    pager.now_ms += 16;
    pager
        .shell
        .pointer_released_at_position_time(150.0, 100.0, Some(pager.now_ms));
    pager.frames(90);
    let rested = pager.state().current_page();
    assert!(
        rested > 0 && pager.rests_on(true, rested),
        "it settles where it stopped, on {rested}"
    );
}

#[test]
fn a_screen_reader_page_turns_whole_pages() {
    for horizontal in [true, false] {
        let mut pager = Pager::new(horizontal);
        assert!(pager.reader_page(horizontal, true));
        assert!(pager.rests_on(horizontal, 1), "forward to the next page");
        assert!(pager.reader_page(horizontal, true));
        assert!(pager.rests_on(horizontal, 2));
        assert!(pager.reader_page(horizontal, false));
        assert!(pager.rests_on(horizontal, 1), "back to the page before");
        assert!(pager.reader_page(horizontal, false));
        assert!(pager.rests_on(horizontal, 0));
        assert!(
            !pager.reader_page(horizontal, false),
            "nothing before the first page"
        );
        assert!(pager.rests_on(horizontal, 0));
    }
}

#[test]
fn fixed_pages_with_spacing_show_the_next_page_and_turn_by_a_page_and_a_gap() {
    let spec = PagerSpec {
        page_size: PageSize::Fixed(200.0),
        page_spacing: 10.0,
        ..PagerSpec::default()
    };
    let mut pager = Pager::with(true, spec);
    let start = |pager: &mut Pager, page| {
        pager
            .start(true, page)
            .map(|start| (start * 100.0).round() / 100.0)
    };
    assert_eq!(start(&mut pager, 1), Some(210.0), "the next page beside");
    pager.drag(true, 250.0, 130.0, 1_000);
    assert!(pager.rests_on(true, 1), "past half of 210 pixels turns it");
    assert_eq!(start(&mut pager, 2), Some(210.0));
    assert!(pager.state().current_page_offset_fraction().abs() < 0.001);
}

#[test]
fn the_settled_page_stays_until_a_swipe_or_an_animation_comes_to_rest() {
    let mut pager = Pager::new(true);
    assert!(!pager.state().is_scroll_in_progress());
    pager.press_and_move(true, 250.0, 80.0, 1_000);
    let state = pager.state();
    assert!(state.is_scroll_in_progress(), "a drag scrolls");
    assert_eq!(state.current_page(), 1, "past half a page");
    assert_eq!(state.settled_page(), 0, "the drag began on page 0");
    pager.release(true, 80.0);
    pager.frames(90);
    assert!(!state.is_scroll_in_progress(), "the settle came to rest");
    assert_eq!(state.settled_page(), 1);

    state.animate_scroll_to_page(3);
    pager.frames(4);
    assert!(state.is_scroll_in_progress(), "an animation scrolls");
    assert_eq!(state.settled_page(), 1, "the animation began on page 1");
    pager.frames(120);
    assert!(!state.is_scroll_in_progress());
    assert_eq!(state.settled_page(), 3);
}

#[test]
fn a_reader_of_the_settled_page_recomposes_when_a_swipe_starts_and_ends() {
    let reads = Reads::default();
    let mut pager = Pager::reading(true, PagerSpec::default(), reads.clone());
    let before = reads.0.get();
    pager.drag(true, 250.0, 80.0, 1_000);
    assert!(pager.rests_on(true, 1));
    let swipe = reads.0.get() - before;
    assert!(
        (1..=3).contains(&swipe),
        "a swipe of 10 moves and 90 settle frames recomposed the reader {swipe} times"
    );
}

#[test]
fn content_padding_rests_a_page_after_the_start_padding_with_its_neighbours_beside() {
    let spec = PagerSpec {
        content_padding: PaddingValues::new(30.0, 0.0, 30.0, 0.0),
        ..PagerSpec::default()
    };
    let mut pager = Pager::with(true, spec);
    let start = |pager: &mut Pager, page| {
        pager
            .start(true, page)
            .map(|start| (start * 100.0).round() / 100.0)
    };
    assert_eq!(start(&mut pager, 0), Some(30.0), "after the start padding");
    assert_eq!(
        start(&mut pager, 1),
        Some(270.0),
        "the next page in the end padding"
    );
    pager.drag(true, 250.0, 120.0, 1_000);
    assert_eq!(pager.state().settled_page(), 1);
    assert_eq!(
        start(&mut pager, 1),
        Some(30.0),
        "page 1 rests after the padding"
    );
    assert_eq!(
        start(&mut pager, 0),
        Some(-210.0),
        "page 0 shows in the start padding"
    );
    assert_eq!(start(&mut pager, 2), Some(270.0));
}

#[test]
fn the_target_page_is_where_an_animation_or_a_released_flick_goes() {
    let mut pager = Pager::new(true);
    let state = pager.state();
    state.animate_scroll_to_page(3);
    pager.frames(4);
    assert!(state.current_page() < 3, "the spring takes frames");
    assert_eq!(state.target_page(), 3, "the animation goes to page 3");
    pager.frames(120);
    assert_eq!((state.current_page(), state.target_page()), (3, 3));

    pager.press_and_move(true, 80.0, 110.0, 30);
    assert!(state.is_scroll_in_progress());
    assert!(state.lazy_list_state().is_dragged());
    assert_eq!(
        state.target_page(),
        state.current_page(),
        "while dragged it is the current page"
    );
    pager.release(true, 110.0);
    pager.frames(1);
    assert!(!state.lazy_list_state().is_dragged(), "the finger is up");
    assert_eq!(state.current_page(), 3, "the page has not turned yet");
    assert_eq!(state.target_page(), 2, "the flick back goes to page 2");
    pager.frames(90);
    assert_eq!((state.current_page(), state.target_page()), (2, 2));
}
