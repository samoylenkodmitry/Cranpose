use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::CompositionLocalProvider;
use cranpose_foundation::{PointerEvent, PointerEventKind, PointerId, PointerSource};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, Size};

use super::*;
use crate::{
    Modifier, ModifierNodeSlices, collect_slices_from_modifier, draw::DrawCommand, modifier::Point,
    run_test_composition,
};

const SIZE: Size = Size {
    width: 40.0,
    height: 20.0,
};
const FACE: Color = Color(0.2, 0.25, 0.3, 1.0);

// What the chain's draws record in a node of SIZE: each fill with its rect,
// its color and whether its corners are round.
fn fills(slices: &ModifierNodeSlices) -> Vec<(Rect, Color, bool)> {
    slices
        .draw_commands()
        .iter()
        .flat_map(|command| recorded(command, SIZE))
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Rect {
                rect,
                brush: Brush::Solid(color),
                ..
            } => Some((rect, color, false)),
            DrawPrimitive::RoundRect {
                rect,
                brush: Brush::Solid(color),
                ..
            } => Some((rect, color, true)),
            _ => None,
        })
        .collect()
}

fn recorded(command: &DrawCommand, size: Size) -> Vec<DrawPrimitive> {
    use cranpose_ui_graphics::DrawScope as _;
    let mut recording = cranpose_ui_graphics::CommandRecording::default();
    let mut scope = crate::draw::command_draw_scope(size, &mut recording);
    command.draw(&mut scope);
    scope.into_primitives()
}

fn event(kind: PointerEventKind, x: f32, source: PointerSource) -> PointerEvent {
    let at = Point { x, y: 10.0 };
    PointerEvent::new(kind, at, at).with_source(source)
}

// A button's chain as apps write it: its face with round corners, the padding
// of its label, then the click.
fn button(clicks: &Rc<Cell<u32>>) -> ModifierNodeSlices {
    let clicks = Rc::clone(clicks);
    collect_slices_from_modifier(
        &Modifier::empty()
            .background(FACE)
            .rounded_corners(6.0)
            .padding(4.0)
            .clickable(move |_| clicks.set(clicks.get() + 1)),
    )
}

const WHOLE: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 40.0,
    height: 20.0,
};

#[test]
fn a_hover_lays_the_hover_color_over_the_whole_round_face_and_leaves_with_the_pointer() {
    let _app_context = crate::render_state::app_context_test_scope();
    let slices = button(&Rc::new(Cell::new(0)));
    let hovered = Indication::default()
        .hovered()
        .expect("the default hover shows");
    assert_eq!(fills(&slices), [(WHOLE, FACE, true)]);

    slices.dispatch_pointer_event(event(PointerEventKind::Enter, 20.0, PointerSource::Mouse));
    assert_eq!(
        fills(&slices),
        [(WHOLE, FACE, true), (WHOLE, hovered, true)]
    );

    slices.dispatch_pointer_event(event(PointerEventKind::Exit, 50.0, PointerSource::Mouse));
    assert_eq!(fills(&slices), [(WHOLE, FACE, true)]);
}

#[test]
fn a_press_shows_until_the_release_and_the_release_clicks() {
    let _app_context = crate::render_state::app_context_test_scope();
    let clicks = Rc::new(Cell::new(0));
    let slices = button(&clicks);
    let pressed = Indication::default()
        .pressed()
        .expect("the default press shows");

    slices.dispatch_pointer_event(event(PointerEventKind::Down, 20.0, PointerSource::Mouse));
    assert_eq!(
        fills(&slices),
        [(WHOLE, FACE, true), (WHOLE, pressed, true)]
    );
    // A small drift keeps the press
    slices.dispatch_pointer_event(event(PointerEventKind::Move, 23.0, PointerSource::Mouse));
    assert_eq!(fills(&slices).len(), 2);

    slices.dispatch_pointer_event(event(PointerEventKind::Up, 23.0, PointerSource::Mouse));
    assert_eq!((clicks.get(), fills(&slices).len()), (1, 1));
}

#[test]
fn a_press_that_travels_past_the_drag_threshold_stops_showing_at_once_and_never_clicks() {
    let _app_context = crate::render_state::app_context_test_scope();
    let clicks = Rc::new(Cell::new(0));
    let slices = button(&clicks);

    slices.dispatch_pointer_event(event(PointerEventKind::Down, 10.0, PointerSource::Mouse));
    slices.dispatch_pointer_event(event(PointerEventKind::Move, 30.0, PointerSource::Mouse));
    assert_eq!(
        fills(&slices),
        [(WHOLE, FACE, true)],
        "the cancel shows before the release"
    );
    // Coming back does not press again
    slices.dispatch_pointer_event(event(PointerEventKind::Move, 10.0, PointerSource::Mouse));
    slices.dispatch_pointer_event(event(PointerEventKind::Up, 10.0, PointerSource::Mouse));
    assert_eq!(clicks.get(), 0);
}

#[test]
fn a_finger_rests_on_nothing_so_a_touch_shows_no_hover() {
    let _app_context = crate::render_state::app_context_test_scope();
    let slices = button(&Rc::new(Cell::new(0)));
    slices.dispatch_pointer_event(event(PointerEventKind::Enter, 20.0, PointerSource::Touch));
    assert_eq!(fills(&slices), [(WHOLE, FACE, true)]);
}

#[test]
fn two_pointers_keep_the_hover_until_both_leave() {
    let _app_context = crate::render_state::app_context_test_scope();
    let slices = button(&Rc::new(Cell::new(0)));
    let (left, right) = (PointerId(1), PointerId(2));
    let on = |kind, id| event(kind, 20.0, PointerSource::Unknown).with_id(id);
    slices.dispatch_pointer_event(on(PointerEventKind::Enter, left));
    slices.dispatch_pointer_event(on(PointerEventKind::Enter, right));
    slices.dispatch_pointer_event(on(PointerEventKind::Exit, left));
    assert_eq!(fills(&slices).len(), 2);
    slices.dispatch_pointer_event(on(PointerEventKind::Exit, right));
    assert_eq!(fills(&slices).len(), 1);
}

#[test]
fn a_control_without_a_background_shows_the_indication_over_its_own_rect() {
    let _app_context = crate::render_state::app_context_test_scope();
    let slices = collect_slices_from_modifier(&Modifier::empty().clickable(|_| {}));
    slices.dispatch_pointer_event(event(PointerEventKind::Down, 20.0, PointerSource::Mouse));
    let pressed = Indication::default()
        .pressed()
        .expect("the default press shows");
    assert_eq!(fills(&slices), [(WHOLE, pressed, false)]);
}

#[test]
fn the_indication_of_the_local_reaches_the_clickables_under_it() {
    let light = Indication::overlay(Color(1.0, 1.0, 1.0, 0.1), Color(1.0, 1.0, 1.0, 0.3));
    let made = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&made);
    let composition = run_test_composition(move || {
        sink.borrow_mut().push(Modifier::empty().clickable(|_| {}));
        let (lit, none) = (Rc::clone(&sink), Rc::clone(&sink));
        CompositionLocalProvider([local_indication().provides(light)], move || {
            lit.borrow_mut().push(Modifier::empty().clickable(|_| {}));
        });
        CompositionLocalProvider([local_indication().provides(Indication::NONE)], move || {
            none.borrow_mut().push(Modifier::empty().clickable(|_| {}));
        });
    });
    composition.with_app_context(|| {
        let shown: Vec<Vec<Color>> = made
            .borrow()
            .iter()
            .map(|modifier| {
                let slices = collect_slices_from_modifier(modifier);
                slices.dispatch_pointer_event(event(
                    PointerEventKind::Enter,
                    20.0,
                    PointerSource::Mouse,
                ));
                fills(&slices)
                    .into_iter()
                    .map(|(_, color, _)| color)
                    .collect()
            })
            .collect();
        let default = Indication::default()
            .hovered()
            .expect("the default hover shows");
        assert_eq!(
            shown,
            [vec![default], vec![Color(1.0, 1.0, 1.0, 0.1)], Vec::new()]
        );
    });
}

#[test]
fn a_clickable_with_no_indication_adds_no_draw_to_its_chain() {
    let _app_context = crate::render_state::app_context_test_scope();
    let slices = collect_slices_from_modifier(
        &Modifier::empty()
            .background(FACE)
            .clickable_with_indication(Indication::NONE, |_| {}),
    );
    slices.dispatch_pointer_event(event(PointerEventKind::Down, 20.0, PointerSource::Mouse));
    assert_eq!(fills(&slices), [(WHOLE, FACE, false)]);
}
