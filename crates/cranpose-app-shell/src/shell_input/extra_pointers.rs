//! Pointers other than the primary one: the fingers that join the primary pointer's gesture, the
//! rays that point into a 3D scene, and the pointers that act on their own.

use std::fmt::Debug;

use cranpose_core::{NodeId, enter_event_handler_scope, run_in_mutable_snapshot};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};
use cranpose_ui_graphics::Point;

use super::Stop;
use crate::{AppShell, PointerEventTime, PointerId, SurfaceMut};

// What a pointer's hover knows when it moves: the pointer, the nodes it hovered, where it is now,
// its held buttons and the time of the move.
#[derive(Clone, Copy)]
pub(super) struct Hover<'a> {
    pub(super) pointer: PointerId,
    pub(super) hovered: &'a [NodeId],
    pub(super) at: Point,
    pub(super) buttons: PointerButtons,
    pub(super) time: PointerEventTime,
}

// A pointer that acts on its own: a controller's or a hand's ray in a headset. It has its own
// place, held buttons and hovered nodes. The surface keeps its pressed gesture in the hit path
// tracker, as it does for the other pointers.
#[derive(Debug)]
struct IndependentPointer {
    id: PointerId,
    at: Point,
    buttons: PointerButtons,
    hovered: Vec<NodeId>,
}

// The independent pointers of a surface. A headset has two at most, so a list that is searched is
// the right size.
#[derive(Debug, Default)]
pub(crate) struct IndependentPointers(Vec<IndependentPointer>);

impl IndependentPointers {
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    fn find(&self, id: PointerId) -> Option<&IndependentPointer> {
        self.0.iter().find(|pointer| pointer.id == id)
    }

    fn find_mut(&mut self, id: PointerId) -> Option<&mut IndependentPointer> {
        self.0.iter_mut().find(|pointer| pointer.id == id)
    }

    // The pointer `id`, which a first call makes.
    fn entry(&mut self, id: PointerId) -> &mut IndependentPointer {
        let index = self
            .0
            .iter()
            .position(|pointer| pointer.id == id)
            .unwrap_or_else(|| {
                self.0.push(IndependentPointer {
                    id,
                    at: Point::default(),
                    buttons: PointerButtons::NONE,
                    hovered: Vec::new(),
                });
                self.0.len() - 1
            });
        &mut self.0[index]
    }

    fn forget(&mut self, id: PointerId) -> Option<IndependentPointer> {
        let index = self.0.iter().position(|pointer| pointer.id == id)?;
        Some(self.0.swap_remove(index))
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> SurfaceMut<'_, R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Dispatches an event for a secondary pointer (`pointer_id != 0`).
    ///
    /// Multi-touch gestures act on the element the first finger grabbed, so
    /// secondary pointers are routed to the hit path captured by the primary
    /// pointer's Down. They carry no hover/click semantics and are ignored
    /// when no primary gesture is in progress.
    ///
    /// Returns `true` when the event was dispatched to at least one target.
    pub fn secondary_pointer_pressed(
        &mut self,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.shell.app.realtime_pointer_event_time(time_ms);
        self.dispatch_secondary_pointer(PointerEventKind::Down, pointer_id, x, y, event_time)
    }

    /// Move counterpart of [`secondary_pointer_pressed`](Self::secondary_pointer_pressed).
    pub fn secondary_pointer_moved(
        &mut self,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.shell.app.realtime_pointer_event_time(time_ms);
        self.dispatch_secondary_pointer(PointerEventKind::Move, pointer_id, x, y, event_time)
    }

    /// Release counterpart of [`secondary_pointer_pressed`](Self::secondary_pointer_pressed).
    pub fn secondary_pointer_released(
        &mut self,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.shell.app.realtime_pointer_event_time(time_ms);
        self.dispatch_secondary_pointer(PointerEventKind::Up, pointer_id, x, y, event_time)
    }

    fn dispatch_secondary_pointer(
        &mut self,
        kind: PointerEventKind,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        event_time: PointerEventTime,
    ) -> bool {
        if pointer_id.is_primary() {
            log::warn!(
                target: "cranpose::input",
                "secondary pointer dispatch called with the primary pointer id"
            );
            return false;
        }

        let _event_handler = enter_event_handler_scope();
        let result = {
            let _app_context = self.shell.app.app_context.enter_scope();
            run_in_mutable_snapshot(|| {
                if !self.surface().hit_path_tracker.has_path(PointerId::PRIMARY) {
                    return false;
                }
                let targets = self.resolve_gesture_targets(PointerId::PRIMARY);
                if targets.is_empty() {
                    return false;
                }
                let pos = Point { x, y };
                let event = self
                    .pointer_event(kind, pos, pos, event_time)
                    .with_buttons(self.surface().buttons_pressed)
                    .with_id(pointer_id)
                    .with_source(self.surface().pointer_source);
                self.dispatch_targets(targets, event, Stop::Never);
                true
            })
            .unwrap_or(false)
        };
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "secondary_pointer {kind:?} id={pointer_id} ({x:.2},{y:.2}) time_ms={:?} animation_time_nanos={} -> {result}",
            event_time.platform_time_ms,
            event_time.animation_time_nanos,
        );
        result
    }

    /// Points pointer `pointer_id` along `ray` in the 3D scene the node
    /// `scene` shows: a controller, a hand or a gaze, in the coordinates of
    /// the space the scene's eye stands in. A `Down` reaches the content of
    /// the mesh surface the ray meets first and keeps its targets as the
    /// pointer's gesture, which the pointer's `Move`s and its `Up` or
    /// `Cancel` reach wherever the ray points then. `pointer_id`, not 0,
    /// tells the pointer from the window's others. Returns `true` when a
    /// target received the event.
    pub fn ray_pointer(
        &mut self,
        (pointer_id, scene): (PointerId, NodeId),
        ray: cranpose_ui_graphics::space::Ray,
        kind: PointerEventKind,
    ) -> bool {
        if pointer_id.is_primary() {
            log::warn!(
                target: "cranpose::input",
                "ray pointer dispatch called with the primary pointer id"
            );
            return false;
        }
        let event_time = self.shell.app.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let result = {
            let _app_context = self.shell.app.app_context.enter_scope();
            run_in_mutable_snapshot(|| {
                let pointer = pointer_id;
                let event = self
                    .pointer_event(kind, Point::default(), Point::default(), event_time)
                    .with_id(pointer)
                    .with_ray(ray);
                if kind == PointerEventKind::Down {
                    let hits = self.surface().renderer.scene().hit_test_ray(scene, ray);
                    return self.press_hits(hits, event, pointer);
                }
                let targets = self.resolve_gesture_targets(pointer);
                if matches!(kind, PointerEventKind::Up | PointerEventKind::Cancel) {
                    self.surface_mut().hit_path_tracker.remove_path(pointer);
                }
                if targets.is_empty() {
                    return false;
                }
                self.dispatch_targets(targets, event, Stop::Never);
                true
            })
            .unwrap_or(false)
        };
        if result {
            self.mark_dirty();
        }
        result
    }
    /// Moves the independent pointer `pointer` to `(x, y)` in logical pixels. A pointer that
    /// acts on its own has its own place, hover, held buttons and gesture, so it neither follows
    /// nor disturbs the primary pointer or another independent pointer: a controller in each hand
    /// of a headset is two of them.
    ///
    /// While one of its buttons is down, the move reaches the nodes that took the press, wherever
    /// the pointer is. Otherwise the pointer hovers: the nodes it leaves see an exit, the nodes it
    /// reaches an enter, and the nodes under it the move. Every event carries `pointer` as its id.
    /// The first move of a pointer makes it. Jetpack Compose has no such call, because its
    /// `PointerInputChange` of a pointer exists only with an event that the platform sends. Returns
    /// `true` when a node took the move.
    pub fn independent_pointer_moved(&mut self, pointer: PointerId, x: f32, y: f32) -> bool {
        self.independent_change(pointer, |surface, event_time| {
            surface.independent_move(pointer, Point { x, y }, event_time)
        })
    }

    /// Presses `button` of the independent pointer `pointer` at its place. The first button down
    /// takes the nodes under the pointer as the pointer's gesture: the primary button reaches
    /// them as a [`PointerEventKind::Down`], another as a [`PointerEventKind::ButtonDown`]. A
    /// button that goes down while another is down reaches the nodes of that gesture. Returns
    /// `true` when a node received the press.
    pub fn independent_pointer_pressed(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
    ) -> bool {
        self.independent_change(pointer, |surface, event_time| {
            surface.independent_press(pointer, button, event_time)
        })
    }

    /// Releases `button` of the independent pointer `pointer`. The nodes of the pointer's gesture
    /// see an [`Up`](PointerEventKind::Up) for the primary button, or a
    /// [`ButtonUp`](PointerEventKind::ButtonUp) for another. The gesture ends with its last
    /// button. Returns `true` when a node received the release.
    pub fn independent_pointer_released(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
    ) -> bool {
        self.independent_change(pointer, |surface, event_time| {
            surface.independent_release(pointer, button, event_time)
        })
    }

    /// Scrolls the nodes under the independent pointer `pointer` by `(delta_x, delta_y)` in
    /// logical pixels, as a wheel does at the pointer's place. A positive `delta_y` is the
    /// direction that a wheel turned away from the user gives. Returns `true` when a node took the
    /// scroll.
    pub fn independent_pointer_scrolled(
        &mut self,
        pointer: PointerId,
        delta_x: f32,
        delta_y: f32,
    ) -> bool {
        self.independent_change(pointer, |surface, event_time| {
            surface.independent_scroll(pointer, Point::new(delta_x, delta_y), event_time)
        })
    }

    /// Takes the independent pointer `pointer` away, as when its controller is lost or its ray
    /// meets nothing. A gesture that it holds is cancelled, and every node that it hovers sees an
    /// exit. Returns `true` when a node saw either.
    pub fn independent_pointer_exited(&mut self, pointer: PointerId) -> bool {
        self.independent_change(pointer, |surface, event_time| {
            surface.independent_exit(pointer, event_time)
        })
    }

    // Runs `change` as the handling of an event: with the app's scope and a snapshot, and with the
    // surface marked for a frame when it changed something. The primary pointer is not
    // independent, and its own calls serve it.
    fn independent_change(
        &mut self,
        pointer: PointerId,
        change: impl FnOnce(&mut Self, PointerEventTime) -> bool,
    ) -> bool {
        if pointer.is_primary() {
            log::warn!(
                target: "cranpose::input",
                "independent pointer called with the primary pointer id"
            );
            return false;
        }
        self.mouse_button_change(change)
    }

    // An event of `kind` from `pointer` at its place, with the buttons that it holds.
    fn independent_event(
        &self,
        pointer: PointerId,
        kind: PointerEventKind,
        event_time: PointerEventTime,
    ) -> Option<PointerEvent> {
        let state = self.surface().independent_pointers.find(pointer)?;
        Some(
            self.pointer_event(kind, state.at, state.at, event_time)
                .with_buttons(state.buttons)
                .with_source(self.surface().pointer_source)
                .with_id(pointer),
        )
    }

    fn independent_move(
        &mut self,
        pointer: PointerId,
        at: Point,
        event_time: PointerEventTime,
    ) -> bool {
        let state = self.surface_mut().independent_pointers.entry(pointer);
        state.at = at;
        let buttons = state.buttons;
        if buttons != PointerButtons::NONE {
            let targets = self.resolve_gesture_targets(pointer);
            let Some(event) = self.independent_event(pointer, PointerEventKind::Move, event_time)
            else {
                return false;
            };
            if targets.is_empty() {
                return false;
            }
            self.dispatch_targets(targets, event, Stop::Never);
            return true;
        }
        let hits = self.surface().renderer.scene().hit_test(at.x, at.y);
        let hovered = self
            .surface_mut()
            .independent_pointers
            .find_mut(pointer)
            .map_or_else(Vec::new, |state| std::mem::take(&mut state.hovered));
        self.send_hover_changes(
            Hover {
                pointer,
                hovered: &hovered,
                at,
                buttons,
                time: event_time,
            },
            &hits,
        );
        let mut hovered = hovered;
        hovered.clear();
        hovered.extend(hits.iter().map(HitTestTarget::node_id));
        if let Some(state) = self.surface_mut().independent_pointers.find_mut(pointer) {
            state.hovered = hovered;
        }
        let Some(event) = self.independent_event(pointer, PointerEventKind::Move, event_time)
        else {
            return false;
        };
        if hits.is_empty() {
            return false;
        }
        self.dispatch_targets(hits, event, Stop::AtConsumer);
        true
    }

    fn independent_press(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
        event_time: PointerEventTime,
    ) -> bool {
        self.activate();
        self.note_focus_moved_by_keyboard(false);
        let state = self.surface_mut().independent_pointers.entry(pointer);
        let first = state.buttons == PointerButtons::NONE;
        state.buttons.insert(button);
        let at = state.at;
        let kind = match button {
            PointerButton::Primary => PointerEventKind::Down,
            other => PointerEventKind::ButtonDown(other),
        };
        let Some(event) = self.independent_event(pointer, kind, event_time) else {
            return false;
        };
        if first {
            let hits = self.surface().renderer.scene().hit_test(at.x, at.y);
            return self.press_hits(hits, event, pointer);
        }
        let targets = self.resolve_gesture_targets(pointer);
        if targets.is_empty() {
            return false;
        }
        self.dispatch_targets(targets, event, Stop::Never);
        true
    }

    fn independent_release(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
        event_time: PointerEventTime,
    ) -> bool {
        let held = self.surface().independent_pointers.find(pointer);
        if !held.is_some_and(|state| state.buttons.contains(button)) {
            return false;
        }
        let targets = self.resolve_gesture_targets(pointer);
        let kind = match button {
            PointerButton::Primary => PointerEventKind::Up,
            other => PointerEventKind::ButtonUp(other),
        };
        let Some(state) = self.surface_mut().independent_pointers.find_mut(pointer) else {
            return false;
        };
        state.buttons.remove(button);
        let gesture_ends = state.buttons == PointerButtons::NONE;
        if gesture_ends {
            self.surface_mut().hit_path_tracker.remove_path(pointer);
        }
        let Some(event) = self.independent_event(pointer, kind, event_time) else {
            return false;
        };
        if targets.is_empty() {
            return false;
        }
        self.dispatch_targets(targets, event, Stop::Never);
        true
    }

    fn independent_scroll(
        &mut self,
        pointer: PointerId,
        delta: Point,
        event_time: PointerEventTime,
    ) -> bool {
        if delta.x.abs() <= f32::EPSILON && delta.y.abs() <= f32::EPSILON {
            return false;
        }
        let Some(at) = self
            .surface()
            .independent_pointers
            .find(pointer)
            .map(|state| state.at)
        else {
            return false;
        };
        let hits = self.surface().renderer.scene().hit_test(at.x, at.y);
        if hits.is_empty() {
            return false;
        }
        let Some(event) = self.independent_event(pointer, PointerEventKind::Scroll, event_time)
        else {
            return false;
        };
        self.dispatch_to_hits(&hits, event.with_scroll_delta(delta))
    }

    fn independent_exit(&mut self, pointer: PointerId, event_time: PointerEventTime) -> bool {
        let Some(gone) = self.surface_mut().independent_pointers.forget(pointer) else {
            return false;
        };
        let mut changed = false;
        if gone.buttons != PointerButtons::NONE {
            let targets = self.resolve_gesture_targets(pointer);
            self.surface_mut().hit_path_tracker.remove_path(pointer);
            changed |= !targets.is_empty();
            let event = self
                .pointer_event(PointerEventKind::Cancel, gone.at, gone.at, event_time)
                .with_source(self.surface().pointer_source)
                .with_id(pointer);
            self.dispatch_targets(targets, event, Stop::Never);
        }
        for node in gone.hovered {
            if let Some(target) = self.surface().renderer.scene().find_target(node) {
                let event = self
                    .pointer_event(PointerEventKind::Exit, gone.at, gone.at, event_time)
                    .with_source(self.surface().pointer_source)
                    .with_id(pointer);
                self.dispatch_targets(std::iter::once(target), event, Stop::Never);
                changed = true;
            }
        }
        changed
    }

    // Sends the exits and enters that a pointer's move makes: an exit to each node of
    // `hover.hovered` that is not under the pointer now, and an enter to each node under it that
    // was not hovered.
    pub(super) fn send_hover_changes(
        &mut self,
        hover: Hover<'_>,
        hits: &[<<R as Renderer>::Scene as RenderScene>::HitTarget],
    ) {
        let Hover {
            pointer,
            hovered,
            at,
            buttons,
            time,
        } = hover;
        let event = |surface: &Self, kind| {
            surface
                .pointer_event(kind, at, at, time)
                .with_buttons(buttons)
                .with_source(surface.surface().pointer_source)
                .with_id(pointer)
        };
        for &old_id in hovered {
            if !hits.iter().any(|hit| hit.node_id() == old_id)
                && let Some(target) = self.surface().renderer.scene().find_target(old_id)
            {
                let exit_event = event(self, PointerEventKind::Exit);
                self.dispatch_targets(std::iter::once(target), exit_event, Stop::Never);
            }
        }
        for hit in hits {
            if !hovered.contains(&hit.node_id()) {
                let enter_event = event(self, PointerEventKind::Enter);
                self.dispatch_targets(std::iter::once(hit.clone()), enter_event, Stop::Never);
            }
        }
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per shell concern")]
impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    /// Primary-surface form of [`SurfaceMut::secondary_pointer_pressed`].
    pub fn secondary_pointer_pressed(
        &mut self,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        self.primary()
            .secondary_pointer_pressed(pointer_id, x, y, time_ms)
    }

    /// Primary-surface form of [`SurfaceMut::ray_pointer`].
    pub fn ray_pointer(
        &mut self,
        (pointer_id, scene): (PointerId, NodeId),
        ray: cranpose_ui_graphics::space::Ray,
        kind: PointerEventKind,
    ) -> bool {
        self.primary().ray_pointer((pointer_id, scene), ray, kind)
    }

    /// Primary-surface form of [`SurfaceMut::secondary_pointer_moved`].
    pub fn secondary_pointer_moved(
        &mut self,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        self.primary()
            .secondary_pointer_moved(pointer_id, x, y, time_ms)
    }

    /// Primary-surface form of [`SurfaceMut::secondary_pointer_released`].
    pub fn secondary_pointer_released(
        &mut self,
        pointer_id: PointerId,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        self.primary()
            .secondary_pointer_released(pointer_id, x, y, time_ms)
    }
    /// Primary-surface form of [`SurfaceMut::independent_pointer_moved`].
    pub fn independent_pointer_moved(&mut self, pointer: PointerId, x: f32, y: f32) -> bool {
        self.primary().independent_pointer_moved(pointer, x, y)
    }

    /// Primary-surface form of [`SurfaceMut::independent_pointer_pressed`].
    pub fn independent_pointer_pressed(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
    ) -> bool {
        self.primary().independent_pointer_pressed(pointer, button)
    }

    /// Primary-surface form of [`SurfaceMut::independent_pointer_released`].
    pub fn independent_pointer_released(
        &mut self,
        pointer: PointerId,
        button: PointerButton,
    ) -> bool {
        self.primary().independent_pointer_released(pointer, button)
    }

    /// Primary-surface form of [`SurfaceMut::independent_pointer_scrolled`].
    pub fn independent_pointer_scrolled(
        &mut self,
        pointer: PointerId,
        delta_x: f32,
        delta_y: f32,
    ) -> bool {
        self.primary()
            .independent_pointer_scrolled(pointer, delta_x, delta_y)
    }

    /// Primary-surface form of [`SurfaceMut::independent_pointer_exited`].
    pub fn independent_pointer_exited(&mut self, pointer: PointerId) -> bool {
        self.primary().independent_pointer_exited(pointer)
    }
}
