use cranpose_core::{Key, MemoryApplier, NodeId, location_key};
use cranpose_ui_graphics::{Color, DrawPrimitive, Point, Rect, Size};

use crate::{
    Composition,
    layout::{LayoutEngine, LayoutTree},
    renderer::{HeadlessRenderer, RenderOp},
    text_selection::HandleKind,
    widgets::{PopupHost, SelectionHandle},
};

fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> LayoutTree {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    layout
}

fn settle(composition: &mut Composition<MemoryApplier>, key: Key, content: &mut dyn FnMut()) {
    for _ in 0..16 {
        if !composition.should_render() {
            break;
        }
        composition
            .reconcile(key, &mut *content)
            .expect("reconcile");
    }
}

fn drawn_rects(scene: &crate::renderer::RecordedRenderScene) -> Vec<Rect> {
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Primitive {
                primitive: DrawPrimitive::Image { rect, .. },
                ..
            }
            | RenderOp::Primitive {
                primitive: DrawPrimitive::Rect { rect, .. },
                ..
            }
            | RenderOp::Primitive {
                primitive: DrawPrimitive::RoundRect { rect, .. },
                ..
            } => Some(*rect),
            _ => None,
        })
        .collect()
}

fn contains(rect: &Rect, p: Point) -> bool {
    p.x >= rect.x - 0.5
        && p.x <= rect.x + rect.width + 0.5
        && p.y >= rect.y - 0.5
        && p.y <= rect.y + rect.height + 0.5
}

#[test]
fn selection_handle_renders_in_overlay_at_its_tip() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let tip = Point { x: 120.0, y: 90.0 };
    let mut content = move || {
        PopupHost(move || {
            SelectionHandle(
                HandleKind::Cursor,
                tip,
                20.0,
                8.0,
                Color(0.2, 0.5, 0.9, 1.0),
                |_pos| {},
                || {},
                || {},
                || {},
            );
        });
    };

    composition
        .render(key, &mut content)
        .expect("initial render");
    settle(&mut composition, key, &mut content);

    let root = composition.root().expect("root");
    let layout = compute_layout(&mut composition, root);
    let scene = HeadlessRenderer::new().render(&layout);

    let rects = drawn_rects(&scene);
    assert!(
        !rects.is_empty(),
        "the selection handle should draw at least one primitive in the overlay"
    );
    assert!(
        rects.iter().any(|rect| contains(rect, tip)),
        "handle box must contain its tip {tip:?}; drawn rects: {rects:?}"
    );
    assert!(
        rects.iter().any(|rect| rect.y + rect.height >= tip.y),
        "the handle bulb should hang at/below the tip"
    );
}

mod long_press {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use cranpose_core::{DefaultScheduler, Runtime};
    use cranpose_foundation::{
        BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
        PointerEventKind,
    };
    use cranpose_ui_graphics::Point;

    use crate::{
        Modifier, collect_modifier_slices,
        text_selection::HandleKind,
        widgets::selection_handle::{
            HANDLE_LONG_PRESS_TIMEOUT_MS, is_handle_long_press, selection_handle_pointer_input,
        },
    };

    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn event(kind: PointerEventKind, x: f32, y: f32, time_ms: i64) -> PointerEvent {
        PointerEvent::new(kind, point(x, y), point(x, y))
            .with_buttons(PointerButtons::new().with(PointerButton::Primary))
            .with_time_ms(Some(time_ms))
    }

    struct Harness {
        _runtime: Runtime,
        handler: Rc<dyn Fn(PointerEvent)>,
        _chain: ModifierNodeChain,
        drags: Rc<Cell<usize>>,
        drag_ends: Rc<Cell<usize>>,
        long_presses: Rc<Cell<usize>>,
        taps: Rc<Cell<usize>>,
    }

    impl Harness {
        fn new() -> Self {
            let runtime = Runtime::new(Arc::new(DefaultScheduler));
            let drags = Rc::new(Cell::new(0));
            let drag_ends = Rc::new(Cell::new(0));
            let long_presses = Rc::new(Cell::new(0));
            let taps = Rc::new(Cell::new(0));

            let on_drag: Rc<dyn Fn(Point)> = {
                let drags = Rc::clone(&drags);
                Rc::new(move |_p| drags.set(drags.get() + 1))
            };
            let on_drag_end: Rc<dyn Fn()> = {
                let drag_ends = Rc::clone(&drag_ends);
                Rc::new(move || drag_ends.set(drag_ends.get() + 1))
            };
            let on_long_press: Rc<dyn Fn()> = {
                let long_presses = Rc::clone(&long_presses);
                Rc::new(move || long_presses.set(long_presses.get() + 1))
            };
            let on_tap: Rc<dyn Fn()> = {
                let taps = Rc::clone(&taps);
                Rc::new(move || taps.set(taps.get() + 1))
            };

            let modifier: Modifier = selection_handle_pointer_input(
                HandleKind::SelectionStart,
                on_drag,
                on_drag_end,
                on_long_press,
                on_tap,
            );
            let elements = modifier.elements();
            let mut chain = ModifierNodeChain::new();
            let mut context = BasicModifierNodeContext::new();
            chain.update_from_slice(&elements, &mut context);
            let slices = collect_modifier_slices(&chain);
            let handler = slices
                .pointer_inputs()
                .first()
                .cloned()
                .expect("selection handle should provide a pointer input handler");

            Self {
                _runtime: runtime,
                handler,
                _chain: chain,
                drags,
                drag_ends,
                long_presses,
                taps,
            }
        }

        fn send(&self, e: PointerEvent) {
            (self.handler)(e);
        }
    }

    #[test]
    fn holding_a_handle_in_place_reports_a_long_press() {
        let _app_context = crate::render_state::app_context_test_scope();
        let h = Harness::new();

        h.send(event(PointerEventKind::Down, 100.0, 100.0, 0));
        h.send(event(
            PointerEventKind::Move,
            101.0,
            100.0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 20,
        ));

        assert_eq!(
            h.long_presses.get(),
            1,
            "a finger resting on the handle past the timeout should report one long-press"
        );

        h.send(event(
            PointerEventKind::Move,
            101.0,
            100.0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 200,
        ));
        h.send(event(
            PointerEventKind::Up,
            101.0,
            100.0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 260,
        ));
        assert_eq!(
            h.long_presses.get(),
            1,
            "long-press should fire at most once per press, got {}",
            h.long_presses.get()
        );
        assert_eq!(
            h.drag_ends.get(),
            1,
            "the lift should still settle the drag"
        );
    }

    #[test]
    fn a_held_handle_reports_the_long_press_on_lift() {
        let _app_context = crate::render_state::app_context_test_scope();
        let h = Harness::new();

        h.send(event(PointerEventKind::Down, 100.0, 100.0, 0));
        h.send(event(
            PointerEventKind::Up,
            100.0,
            100.0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 5,
        ));

        assert_eq!(
            h.long_presses.get(),
            1,
            "lifting after a long hold should report a long-press"
        );
        assert_eq!(h.drag_ends.get(), 1);
    }

    #[test]
    fn a_quick_tap_is_not_a_long_press() {
        let _app_context = crate::render_state::app_context_test_scope();
        let h = Harness::new();

        h.send(event(PointerEventKind::Down, 100.0, 100.0, 0));
        h.send(event(PointerEventKind::Up, 100.0, 100.0, 80));

        assert_eq!(
            h.long_presses.get(),
            0,
            "a quick tap on the handle must not be treated as a long-press"
        );
        assert_eq!(h.drag_ends.get(), 1);
        assert_eq!(
            h.taps.get(),
            1,
            "a quick tap on the handle must report exactly one tap"
        );
    }

    #[test]
    fn dragging_a_handle_is_not_a_tap() {
        let _app_context = crate::render_state::app_context_test_scope();
        let h = Harness::new();

        h.send(event(PointerEventKind::Down, 100.0, 100.0, 0));
        h.send(event(PointerEventKind::Move, 160.0, 130.0, 40));
        h.send(event(PointerEventKind::Up, 160.0, 130.0, 60));

        assert_eq!(h.taps.get(), 0, "dragging the handle must not report a tap");
        assert_eq!(h.drag_ends.get(), 1);
    }

    #[test]
    fn dragging_a_handle_is_not_a_long_press() {
        let _app_context = crate::render_state::app_context_test_scope();
        let h = Harness::new();

        h.send(event(PointerEventKind::Down, 100.0, 100.0, 0));
        h.send(event(
            PointerEventKind::Move,
            180.0,
            140.0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 50,
        ));
        h.send(event(
            PointerEventKind::Up,
            180.0,
            140.0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 90,
        ));

        assert_eq!(
            h.long_presses.get(),
            0,
            "dragging the handle must not report a long-press"
        );
        assert!(h.drags.get() >= 2, "the drag positions should be reported");
    }

    #[test]
    fn is_handle_long_press_classifies_hold_vs_tap_vs_drag() {
        assert!(is_handle_long_press(
            0,
            HANDLE_LONG_PRESS_TIMEOUT_MS,
            point(10.0, 10.0),
            point(14.0, 12.0),
        ));
        assert!(!is_handle_long_press(
            0,
            HANDLE_LONG_PRESS_TIMEOUT_MS - 1,
            point(10.0, 10.0),
            point(10.0, 10.0),
        ));
        assert!(!is_handle_long_press(
            0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 100,
            point(10.0, 10.0),
            point(60.0, 10.0),
        ));
    }
}

mod kind_restart {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use cranpose_core::{DefaultScheduler, Runtime};
    use cranpose_foundation::{
        BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
        PointerEventKind,
    };
    use cranpose_ui_graphics::Point;

    use crate::{
        Modifier, collect_modifier_slices, text_selection::HandleKind,
        widgets::selection_handle::selection_handle_pointer_input,
    };

    fn down() -> PointerEvent {
        let p = Point { x: 100.0, y: 100.0 };
        PointerEvent::new(PointerEventKind::Down, p, p)
            .with_buttons(PointerButtons::new().with(PointerButton::Primary))
            .with_time_ms(Some(0))
    }

    fn handle_modifier(kind: HandleKind, on_drag: Rc<dyn Fn(Point)>) -> Modifier {
        let noop_end: Rc<dyn Fn()> = Rc::new(|| {});
        let noop_lp: Rc<dyn Fn()> = Rc::new(|| {});
        let noop_tap: Rc<dyn Fn()> = Rc::new(|| {});
        selection_handle_pointer_input(kind, on_drag, noop_end, noop_lp, noop_tap)
    }

    fn active_handler(chain: &ModifierNodeChain) -> Rc<dyn Fn(PointerEvent)> {
        collect_modifier_slices(chain)
            .pointer_inputs()
            .first()
            .cloned()
            .expect("handle provides a pointer input handler")
    }

    #[test]
    fn reusing_the_cursor_slot_as_the_start_handle_runs_the_edge_drag_not_the_caret_drag() {
        let _app_context = crate::render_state::app_context_test_scope();
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));

        let caret_drags = Rc::new(Cell::new(0usize));
        let edge_drags = Rc::new(Cell::new(0usize));

        let caret_drag: Rc<dyn Fn(Point)> = {
            let c = Rc::clone(&caret_drags);
            Rc::new(move |_| c.set(c.get() + 1))
        };
        let edge_drag: Rc<dyn Fn(Point)> = {
            let e = Rc::clone(&edge_drags);
            Rc::new(move |_| e.set(e.get() + 1))
        };

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();

        chain.update_from_slice(
            &handle_modifier(HandleKind::Cursor, Rc::clone(&caret_drag)).elements(),
            &mut context,
        );

        chain.update_from_slice(
            &handle_modifier(HandleKind::SelectionStart, Rc::clone(&edge_drag)).elements(),
            &mut context,
        );

        active_handler(&chain)(down());

        assert_eq!(
            edge_drags.get(),
            1,
            "after the slot's kind changed to SelectionStart, grabbing it must run the \
             edge drag (which keeps the selection), got {} edge / {} caret",
            edge_drags.get(),
            caret_drags.get()
        );
        assert_eq!(
            caret_drags.get(),
            0,
            "the stale cursor caret-collapse drag must not run once the kind changed"
        );
    }

    #[test]
    fn end_handle_runs_its_own_edge_drag() {
        let _app_context = crate::render_state::app_context_test_scope();
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));

        let end_drags = Rc::new(Cell::new(0usize));
        let end_drag: Rc<dyn Fn(Point)> = {
            let e = Rc::clone(&end_drags);
            Rc::new(move |_| e.set(e.get() + 1))
        };

        let mut chain = ModifierNodeChain::new();
        let mut context = BasicModifierNodeContext::new();
        chain.update_from_slice(
            &handle_modifier(HandleKind::SelectionEnd, Rc::clone(&end_drag)).elements(),
            &mut context,
        );
        active_handler(&chain)(down());

        assert_eq!(
            end_drags.get(),
            1,
            "the end handle must run its edge drag on grab"
        );
    }
}
