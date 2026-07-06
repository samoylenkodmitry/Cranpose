//! Tests that a [`SelectionHandle`] renders its teardrop in the top-level
//! overlay, anchored so its tip lands on the given text endpoint.

use crate::layout::{LayoutEngine, LayoutTree};
use crate::renderer::{HeadlessRenderer, RenderOp};
use crate::text_selection::HandleKind;
use crate::widgets::{PopupHost, SelectionHandle};
use crate::Composition;
use cranpose_core::{location_key, Key, MemoryApplier, NodeId};
use cranpose_ui_graphics::{Color, DrawPrimitive, Point, Rect, Size};

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

/// Any drawn primitive's bounding rectangle (the teardrop rasterizes to an
/// image mask; backgrounds/rects are also matched so the test is robust to the
/// exact draw technique).
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
                8.0,
                Color(0.2, 0.5, 0.9, 1.0),
                |_pos| {},
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
    // The handle's drawn box must contain the tip (120, 90): the teardrop's tip
    // points exactly at the text endpoint.
    assert!(
        rects.iter().any(|rect| contains(rect, tip)),
        "handle box must contain its tip {tip:?}; drawn rects: {rects:?}"
    );
    // And it must render well below y=0 (i.e. it is not clamped to the top of
    // any parent) — the bulb hangs below the tip.
    assert!(
        rects.iter().any(|rect| rect.y + rect.height >= tip.y),
        "the handle bulb should hang at/below the tip"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Long-press → (re)open the contextual menu
//
// A selection handle reports a long-press (finger held on the handle without
// dragging it) so the field can re-show the Copy/Cut/Paste/Select-all menu even
// when the selection range has not changed. These drive the real handle gesture
// modifier synchronously, the same way the zoom gesture tests do.
// ─────────────────────────────────────────────────────────────────────────────

mod long_press {
    use crate::widgets::selection_handle::{
        is_handle_long_press, selection_handle_pointer_input, HANDLE_LONG_PRESS_TIMEOUT_MS,
    };
    use crate::{collect_modifier_slices, Modifier};
    use cranpose_core::{DefaultScheduler, Runtime};
    use cranpose_foundation::{
        BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
        PointerEventKind,
    };
    use cranpose_ui_graphics::Point;
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn event(kind: PointerEventKind, x: f32, y: f32, time_ms: i64) -> PointerEvent {
        PointerEvent::new(kind, point(x, y), point(x, y))
            .with_buttons(PointerButtons::new().with(PointerButton::Primary))
            .with_time_ms(Some(time_ms))
    }

    /// Builds the real selection-handle gesture modifier and returns a handler
    /// that synchronously drives its async pointer loop, plus the chain (kept
    /// alive so the task keeps running). Counters record the fired callbacks.
    struct Harness {
        _runtime: Runtime,
        handler: Rc<dyn Fn(PointerEvent)>,
        _chain: ModifierNodeChain,
        drags: Rc<Cell<usize>>,
        drag_ends: Rc<Cell<usize>>,
        long_presses: Rc<Cell<usize>>,
    }

    impl Harness {
        fn new() -> Self {
            let runtime = Runtime::new(Arc::new(DefaultScheduler));
            let drags = Rc::new(Cell::new(0));
            let drag_ends = Rc::new(Cell::new(0));
            let long_presses = Rc::new(Cell::new(0));

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

            let modifier: Modifier =
                selection_handle_pointer_input(on_drag, on_drag_end, on_long_press);
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

        // Finger goes down, rests in place, then a later sample (as a real touch
        // history emits) crosses the long-press timeout without moving.
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

        // It must fire only once per press even as more samples arrive.
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

        // A perfectly still finger emits no Move samples; the elapsed hold is
        // only observable when it lifts. That lift must still report the
        // long-press (and settle) so the menu can re-open.
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
    }

    #[test]
    fn dragging_a_handle_is_not_a_long_press() {
        let _app_context = crate::render_state::app_context_test_scope();
        let h = Harness::new();

        // Held long enough, but the finger travels far — that is a drag to
        // adjust the selection, not a long-press.
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
        // Held long enough, barely moved → long-press.
        assert!(is_handle_long_press(
            0,
            HANDLE_LONG_PRESS_TIMEOUT_MS,
            point(10.0, 10.0),
            point(14.0, 12.0),
        ));
        // Not held long enough → not a long-press.
        assert!(!is_handle_long_press(
            0,
            HANDLE_LONG_PRESS_TIMEOUT_MS - 1,
            point(10.0, 10.0),
            point(10.0, 10.0),
        ));
        // Held long enough but moved far → a drag, not a long-press.
        assert!(!is_handle_long_press(
            0,
            HANDLE_LONG_PRESS_TIMEOUT_MS + 100,
            point(10.0, 10.0),
            point(60.0, 10.0),
        ));
    }
}
