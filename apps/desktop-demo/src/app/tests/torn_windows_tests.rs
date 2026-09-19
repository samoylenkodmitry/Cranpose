use super::*;

const ORIGIN: Point = Point { x: 100.0, y: 100.0 };
const SIZE: Size = Size {
    width: 400.0,
    height: 300.0,
};
const PANE: Size = Size {
    width: 200.0,
    height: 100.0,
};

fn tabs() -> Rules {
    Rules::tabs("t", SIZE, 30.0)
}

fn stack() -> Rules {
    Rules::stack("s", PANE, Axis::Vertical, 10.0)
}

fn declared(panes: &[u64]) -> Vec<(u64, Size)> {
    panes.iter().map(|pane| (*pane, SIZE)).collect()
}

fn rect(window: u64, origin: Point) -> Rect {
    Rect {
        window,
        origin,
        size: SIZE,
    }
}

fn two_panes_in_one_window() -> (TornWindows, u64) {
    let mut model = TornWindows::new();
    model.reconcile(&declared(&[1, 2]), &tabs());
    let window = model.windows()[0].id;
    (model, window)
}

fn second_tab_pressed() -> (TornWindows, u64, [Rect; 1]) {
    let (mut model, window) = two_panes_in_one_window();
    model.press(2, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
    (model, window, [rect(window, ORIGIN)])
}

#[test]
fn declared_panes_open_in_one_window() {
    let (model, window) = two_panes_in_one_window();
    assert_eq!(model.windows().len(), 1);
    assert_eq!(
        model.window(window).map(|w| w.panes.clone()),
        Some(vec![1, 2])
    );
}

#[test]
fn an_undeclared_pane_leaves_and_an_empty_window_closes() {
    let (mut model, _) = two_panes_in_one_window();
    assert!(model.reconcile(&declared(&[2]), &tabs()));
    assert_eq!(model.window_of(1), None);
    assert!(model.reconcile(&[], &tabs()));
    assert!(model.windows().is_empty());
}

#[test]
fn reconciling_with_nothing_new_changes_nothing() {
    let (mut model, _) = two_panes_in_one_window();
    assert!(!model.reconcile(&declared(&[1, 2]), &tabs()));
}

#[test]
fn a_placement_hint_sends_a_new_pane_to_that_window() {
    let (mut model, first) = two_panes_in_one_window();
    model.press(2, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
    let rects = [rect(first, ORIGIN)];
    model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
    model.release();
    let second = model.window_of(2).expect("torn window");
    model.place_next_in(3, second);
    model.reconcile(&declared(&[1, 2, 3]), &tabs());
    assert_eq!(model.window_of(3), Some(second));
}

#[test]
fn a_press_on_a_pane_no_window_holds_begins_nothing() {
    let (mut model, _) = two_panes_in_one_window();
    assert!(!model.press(9, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs()));
    assert_eq!(model.drag(), None);
}

#[test]
fn pulling_inside_the_strip_does_not_tear() {
    let (mut model, _, rects) = second_tab_pressed();
    let step = model.drag_to(Point::new(260.0, 120.0), &rects, &tabs());
    assert_eq!(step, Step::Rest);
    assert_eq!(model.windows().len(), 1);
}

#[test]
fn pulling_out_of_the_strip_tears_into_a_carried_window() {
    let (mut model, window, rects) = second_tab_pressed();
    let step = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
    let Step::Carry(carried, at) = step else {
        panic!("expected a carry, got {step:?}");
    };
    assert_ne!(carried, window);
    assert_eq!(model.window_of(2), Some(carried));
    assert_eq!(at, minus(Point::new(180.0, 300.0), tabs().tear_grab));
    assert_eq!(model.window(carried).map(|w| w.origin), Some(at));
    assert_eq!(model.drag().and_then(|d| d.carrying), Some(carried));
}

#[test]
fn a_carried_pane_joins_the_window_whose_strip_it_enters() {
    let (mut model, window, rects) = second_tab_pressed();
    let Step::Carry(carried, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs()) else {
        panic!("expected a carry");
    };
    let rects = [
        rect(window, ORIGIN),
        rect(carried, Point::new(600.0, 600.0)),
    ];
    let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
    assert_eq!(step, Step::Join(window, Side::After));
    assert_eq!(model.window_of(2), Some(window));
    assert!(model.window(carried).is_none());
    assert_eq!(model.drag().and_then(|d| d.carrying), None);
}

#[test]
fn a_lone_pane_carries_its_own_window_and_parks_it_on_joining() {
    let mut model = TornWindows::new();
    model.reconcile(&declared(&[1]), &tabs());
    let first = model.windows()[0].id;
    model.press(1, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
    let rects = [rect(first, ORIGIN)];
    model.drag_to(Point::new(180.0, 300.0), &rects, &tabs());
    model.release();
    let second = model.window_of(1).expect("lone window");
    assert_eq!(second, first);

    model.reconcile(&declared(&[1, 2]), &tabs());
    let other = model.window_of(2).expect("second window");
    assert_eq!(other, first);
    model.drag_to(Point::new(0.0, 0.0), &[], &tabs());
    let step = model.press(1, Point::new(10.0, 10.0), Point::new(110.0, 110.0), &tabs());
    assert!(step);
}

#[test]
fn a_window_the_os_moved_is_adopted_before_the_next_step() {
    let (mut model, window) = two_panes_in_one_window();
    let moved = Point::new(700.0, 50.0);
    assert!(model.adopt(&[rect(window, moved)]));
    assert_eq!(model.window(window).map(|w| w.origin), Some(moved));
    assert!(!model.adopt(&[rect(window, moved)]));
}

fn torn_out_then_carried_back() -> (TornWindows, u64, u64, [Rect; 2]) {
    let mut model = TornWindows::new();
    model.reconcile(&declared(&[1, 2]), &tabs());
    let first = model.windows()[0].id;
    model.press(2, Point::new(80.0, 15.0), Point::new(180.0, 115.0), &tabs());
    let rects = [rect(first, ORIGIN)];
    let Step::Carry(second, _) = model.drag_to(Point::new(180.0, 300.0), &rects, &tabs()) else {
        panic!("expected a carry");
    };
    model.release();

    model.press(2, Point::new(20.0, 15.0), Point::new(620.0, 615.0), &tabs());
    let rects = [rect(first, ORIGIN), rect(second, Point::new(600.0, 600.0))];
    let step = model.drag_to(Point::new(620.0, 700.0), &rects, &tabs());
    assert_eq!(step, Step::Carry(second, Point::new(600.0, 685.0)));
    (model, first, second, rects)
}

#[test]
fn a_lone_window_that_joins_another_is_parked_until_release() {
    let (mut model, first, second, rects) = torn_out_then_carried_back();
    let step = model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
    assert_eq!(step, Step::Join(first, Side::After));
    assert_eq!(model.window(second).map(|w| w.parked), Some(true));
    assert_eq!(model.windows().len(), 2);
    model.release();
    assert_eq!(model.windows().len(), 1);
}

#[test]
fn tearing_again_reuses_the_parked_source_window() {
    let (mut model, first, second, rects) = torn_out_then_carried_back();
    model.drag_to(Point::new(300.0, 110.0), &rects, &tabs());
    let rects = [rect(first, ORIGIN)];
    let step = model.drag_to(Point::new(300.0, 400.0), &rects, &tabs());
    let Step::Carry(reused, _) = step else {
        panic!("expected a carry, got {step:?}");
    };
    assert_eq!(reused, second);
    assert_eq!(model.window(second).map(|w| w.parked), Some(false));
}

#[test]
fn closing_the_shown_pane_shows_its_neighbour() {
    let mut model = TornWindows::new();
    model.reconcile(&declared(&[1, 2, 3]), &tabs());
    let window = model.windows()[0].id;
    model.activate(2);
    model.reconcile(&declared(&[1, 3]), &tabs());
    assert_eq!(model.window(window).map(|w| w.active), Some(3));
}

fn sized(panes: &[(u64, f32)]) -> Vec<(u64, Size)> {
    panes
        .iter()
        .map(|(n, height)| (*n, Size::new(200.0, *height)))
        .collect()
}

fn three_stacked() -> (TornWindows, u64) {
    let mut model = TornWindows::new();
    model.reconcile(&sized(&[(1, 100.0), (2, 100.0), (3, 200.0)]), &stack());
    let window = model.windows()[0].id;
    model.adopt(&[Rect {
        window,
        origin: ORIGIN,
        size: model.window_size(window, &stack()),
    }]);
    (model, window)
}

fn stack_rects(model: &TornWindows) -> Vec<Rect> {
    model
        .windows()
        .iter()
        .map(|window| Rect {
            window: window.id,
            origin: window.origin,
            size: model.window_size(window.id, &stack()),
        })
        .collect()
}

#[test]
fn a_stacked_window_is_as_large_as_its_panes_together() {
    let (model, window) = three_stacked();
    assert_eq!(model.window_size(window, &stack()), Size::new(200.0, 400.0));
    assert_eq!(model.pane_offset(3, &stack()), Point::new(0.0, 200.0));
}

#[test]
fn tearing_the_first_pane_leaves_the_others_where_they_were() {
    let (mut model, window) = three_stacked();
    model.press(
        1,
        Point::new(100.0, 10.0),
        Point::new(200.0, 110.0),
        &stack(),
    );
    let step = model.drag_to(Point::new(200.0, 140.0), &stack_rects(&model), &stack());
    let Step::Carry(carried, at) = step else {
        panic!("expected a carry, got {step:?}");
    };
    assert_eq!(
        at,
        Point::new(100.0, 130.0),
        "the pane stays under the pointer"
    );
    assert_eq!(model.window_of(1), Some(carried));
    assert_eq!(
        model.window(window).map(|w| (w.panes.clone(), w.origin)),
        Some((vec![2, 3], Point::new(100.0, 200.0))),
        "the rest of the stack does not jump up into the gap"
    );
}

#[test]
fn tearing_a_middle_pane_splits_the_stack_in_two() {
    let (mut model, window) = three_stacked();
    model.press(
        2,
        Point::new(100.0, 110.0),
        Point::new(200.0, 210.0),
        &stack(),
    );
    let step = model.drag_to(Point::new(230.0, 210.0), &stack_rects(&model), &stack());
    let Step::Carry(carried, at) = step else {
        panic!("expected a carry, got {step:?}");
    };
    assert_eq!(at, Point::new(130.0, 200.0));
    assert_eq!(model.windows().len(), 3);
    assert_eq!(
        model.window(window).map(|w| (w.panes.clone(), w.origin)),
        Some((vec![1], ORIGIN))
    );
    let tail = model.window_of(3).expect("a window for the tail");
    assert_ne!(tail, carried);
    assert_eq!(
        model.window(tail).map(|w| w.origin),
        Some(Point::new(100.0, 300.0)),
        "the panes below the gap stay where they were"
    );
}

#[test]
fn closing_a_middle_pane_splits_the_stack_too() {
    let (mut model, window) = three_stacked();
    assert!(model.reconcile(&sized(&[(1, 100.0), (3, 200.0)]), &stack()));
    assert_eq!(model.window(window).map(|w| w.panes.clone()), Some(vec![1]));
    let tail = model.window_of(3).expect("a window for the tail");
    assert_eq!(
        model.window(tail).map(|w| w.origin),
        Some(Point::new(100.0, 300.0))
    );
}

fn a_pane_alone_beside_a_stack() -> (TornWindows, u64, u64) {
    let mut model = TornWindows::new();
    model.reconcile(&sized(&[(1, 100.0)]), &stack());
    let first = model.windows()[0].id;
    model.place_next_in(2, u64::MAX);
    model.reconcile(&sized(&[(1, 100.0), (2, 100.0)]), &stack());
    let second = model.windows()[0].id;
    assert_eq!(
        first, second,
        "a missing placement falls back to the first window"
    );
    model.press(
        2,
        Point::new(100.0, 110.0),
        Point::new(200.0, 210.0),
        &stack(),
    );
    model.adopt(&[rect(first, ORIGIN)]);
    let step = model.drag_to(Point::new(600.0, 610.0), &stack_rects(&model), &stack());
    let Step::Carry(carried, _) = step else {
        panic!("expected a carry, got {step:?}");
    };
    (model, first, carried)
}

#[test]
fn a_carried_pane_snaps_under_the_window_it_nears() {
    let (mut model, first, carried) = a_pane_alone_beside_a_stack();
    let step = model.drag_to(Point::new(205.0, 215.0), &stack_rects(&model), &stack());
    assert_eq!(step, Step::Join(first, Side::After));
    assert_eq!(
        model.window(first).map(|w| (w.panes.clone(), w.origin)),
        Some((vec![1, 2], ORIGIN))
    );
    assert_eq!(model.window_size(first, &stack()), Size::new(200.0, 200.0));
    assert!(
        model.window(carried).is_none(),
        "the carried window was not the one pressed, so nothing routes to it"
    );
}

#[test]
fn a_carried_pane_snaps_above_a_window_which_grows_upward() {
    let (mut model, first, _) = a_pane_alone_beside_a_stack();
    let step = model.drag_to(Point::new(205.0, 5.0), &stack_rects(&model), &stack());
    assert_eq!(step, Step::Join(first, Side::Before));
    assert_eq!(
        model.window(first).map(|w| (w.panes.clone(), w.origin)),
        Some((vec![2, 1], Point::new(100.0, 0.0))),
        "the window's top moves up by the pane's height so pane 1 stays put"
    );
}

#[test]
fn a_carried_pane_beside_a_window_does_not_snap() {
    let (mut model, first, carried) = a_pane_alone_beside_a_stack();
    let step = model.drag_to(Point::new(450.0, 215.0), &stack_rects(&model), &stack());
    assert_eq!(step, Step::Carry(carried, Point::new(350.0, 205.0)));
    assert_eq!(model.window(first).map(|w| w.panes.len()), Some(1));
}

#[test]
fn a_carried_pane_overlapping_a_window_by_a_sliver_does_not_snap() {
    let (mut model, first, carried) = a_pane_alone_beside_a_stack();
    let step = model.drag_to(Point::new(390.0, 215.0), &stack_rects(&model), &stack());
    assert_eq!(
        step,
        Step::Carry(carried, Point::new(290.0, 205.0)),
        "joining would drag the pane 190 pixels sideways into the stack"
    );
    assert_eq!(model.window(first).map(|w| w.panes.len()), Some(1));
}

#[test]
fn a_carried_pane_nearly_in_line_with_a_window_still_snaps() {
    let (mut model, first, _) = a_pane_alone_beside_a_stack();
    let step = model.drag_to(Point::new(208.0, 215.0), &stack_rects(&model), &stack());
    assert_eq!(step, Step::Join(first, Side::After));
}

#[test]
fn a_torn_pane_does_not_snap_back_onto_the_stack_it_left() {
    let (mut model, window) = three_stacked();
    model.press(
        2,
        Point::new(100.0, 110.0),
        Point::new(200.0, 210.0),
        &stack(),
    );
    let step = model.drag_to(Point::new(200.0, 225.0), &stack_rects(&model), &stack());
    let Step::Carry(carried, _) = step else {
        panic!("expected a carry, got {step:?}");
    };
    let step = model.drag_to(Point::new(200.0, 226.0), &stack_rects(&model), &stack());
    assert_eq!(
        step,
        Step::Carry(carried, Point::new(100.0, 216.0)),
        "a pane torn off starts edge to edge with the stack and must first get clear of it"
    );
    model.drag_to(Point::new(200.0, 400.0), &stack_rects(&model), &stack());
    let step = model.drag_to(Point::new(200.0, 215.0), &stack_rects(&model), &stack());
    assert_eq!(step, Step::Join(window, Side::After));
}

#[test]
fn a_pane_torn_off_a_second_time_stays_under_the_pointer() {
    let (mut model, window) = three_stacked();
    model.press(
        3,
        Point::new(100.0, 210.0),
        Point::new(200.0, 310.0),
        &stack(),
    );
    model.drag_to(Point::new(200.0, 340.0), &stack_rects(&model), &stack());
    model.drag_to(Point::new(200.0, 500.0), &stack_rects(&model), &stack());
    model.drag_to(Point::new(200.0, 315.0), &stack_rects(&model), &stack());
    assert_eq!(model.window_of(3), Some(window), "snapped back on");
    let step = model.drag_to(Point::new(200.0, 340.0), &stack_rects(&model), &stack());
    assert_eq!(
        step,
        Step::Carry(
            model.window_of(3).expect("carried"),
            Point::new(100.0, 330.0)
        ),
        "the grab is the pointer's place in the pane, whatever window holds it"
    );
}
