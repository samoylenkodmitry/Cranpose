use super::*;

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    fn pointer_event(
        &self,
        kind: PointerEventKind,
        position: Point,
        global_position: Point,
        event_time: PointerEventTime,
    ) -> PointerEvent {
        let mut event = PointerEvent::new(kind, position, global_position)
            .with_time_ms(event_time.platform_time_ms)
            .with_animation_time_nanos(event_time.animation_time_nanos);
        // Unlike `source` (stamped per call site because it varies per
        // event -- a touch vs. a mouse sample), modifiers track continuously
        // and every PointerEvent the shell builds goes through this one
        // constructor, so this is the single choke point to stamp it from:
        // no call site below can forget it, including ones added later.
        event.modifiers = self.modifiers;
        event
    }

    fn resolve_gesture_targets(
        &self,
        pointer: PointerId,
    ) -> Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget> {
        self.resolve_hit_path(pointer)
    }

    /// Resolves cached NodeIds to fresh HitTargets from the current scene.
    ///
    /// This is the key to avoiding stale geometry during scroll/layout changes:
    /// - We cache NodeIds on PointerDown (stable identity)
    /// - On Move/Up/Cancel, we call find_target() to get fresh geometry
    /// - Handler closures are preserved (same Rc), so gesture state survives
    fn resolve_hit_path(
        &self,
        pointer: PointerId,
    ) -> Vec<<<R as Renderer>::Scene as RenderScene>::HitTarget> {
        let Some(node_ids) = self.hit_path_tracker.dispatch_order(pointer) else {
            return Vec::new();
        };

        let scene = self.renderer.scene();
        let targets: Vec<_> = node_ids
            .iter()
            .filter_map(|&id| scene.find_target(id))
            .collect();
        log::trace!(
            target: "cranpose::input",
            "resolve_hit_path pointer={pointer:?} cached={node_ids:?} resolved_count={}",
            targets.len()
        );
        targets
    }

    fn dispatch_targets<I>(&mut self, targets: I, event: PointerEvent, stop_on_consume: bool)
    where
        I: IntoIterator<Item = <<R as Renderer>::Scene as RenderScene>::HitTarget>,
    {
        let mut applier = self.composition.applier_mut();
        for target in targets {
            let node_id = target.node_id();
            target.dispatch_with_applier(&mut applier, event.clone());
            log::trace!(
                target: "cranpose::input",
                "dispatch {:?} node={} consumed={} stop_on_consume={}",
                event.kind,
                node_id,
                event.is_consumed(),
                stop_on_consume,
            );
            if stop_on_consume && event.is_consumed() {
                break;
            }
        }
        event.finish_post_dispatch();
    }

    /// Sets the device source (touch/mouse/stylus) of the pointer sample that
    /// the platform is about to dispatch. Call this before `set_cursor` /
    /// `pointer_pressed` / `pointer_released` so the resulting `PointerEvent`s
    /// carry the source so consumers can preserve device-specific gesture
    /// details without changing shared pointer UI.
    pub fn set_pointer_source(&mut self, source: PointerSource) {
        self.pointer_source = source;
    }

    /// The device source of the most recent pointer sample.
    pub fn pointer_source(&self) -> PointerSource {
        self.pointer_source
    }

    /// Sets the keyboard modifiers held right now, so the platform's live
    /// modifier state (winit's `ModifiersChanged`, a DOM event's
    /// `shiftKey`/`ctrlKey`/`altKey`/`metaKey`) reaches every `PointerEvent`
    /// the shell dispatches from here on -- the same state the wheel path
    /// already carries via [`WheelScroll::with_modifiers`](crate::WheelScroll::with_modifiers).
    /// A platform that never calls this leaves pointer events reporting
    /// `None` (see [`PointerEvent::modifiers`]) rather than a silently wrong
    /// "nothing held".
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = Some(modifiers);
    }

    /// The keyboard modifiers most recently set via
    /// [`set_modifiers`](Self::set_modifiers), or `None` if the platform has
    /// never reported them.
    pub fn modifiers(&self) -> Option<Modifiers> {
        self.modifiers
    }

    pub fn set_cursor(&mut self, x: f32, y: f32) -> bool {
        self.set_cursor_at_time(x, y, None)
    }

    /// Like [`set_cursor`](Self::set_cursor), but carries the platform input
    /// timestamp (milliseconds, platform-specific time base) of the sample.
    ///
    /// Platforms that deliver input batched/frame-aligned (Android) must use
    /// this so gesture velocity is computed from real event times instead of
    /// delivery times.
    pub fn set_cursor_at_time(&mut self, x: f32, y: f32, time_ms: Option<i64>) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.set_cursor_at_event_time(x, y, event_time)
    }

    /// Set the cursor using a timestamp already resolved into both clock domains.
    pub fn set_cursor_at_event_time(
        &mut self,
        x: f32,
        y: f32,
        event_time: PointerEventTime,
    ) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| self.set_cursor_inner(x, y, event_time)).unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "set_cursor ({x:.2},{y:.2}) time_ms={:?} animation_time_nanos={} -> {result}",
            event_time.platform_time_ms,
            event_time.animation_time_nanos,
        );
        result
    }

    fn set_cursor_inner(&mut self, x: f32, y: f32, event_time: PointerEventTime) -> bool {
        self.cursor = (x, y);

        // During a gesture (button pressed), ONLY dispatch to the tracked hit path.
        // Never fall back to hover hit-testing while buttons are down.
        // This maintains the invariant: the path that receives Down must receive Move and Up/Cancel.
        if self.buttons_pressed != PointerButtons::NONE {
            if self.hit_path_tracker.has_path(PointerId::PRIMARY) {
                let targets = self.resolve_gesture_targets(PointerId::PRIMARY);
                if !targets.is_empty() {
                    let event = self
                        .pointer_event(
                            PointerEventKind::Move,
                            Point { x, y },
                            Point { x, y },
                            event_time,
                        )
                        .with_buttons(self.buttons_pressed)
                        .with_source(self.pointer_source);
                    self.dispatch_targets(targets, event, false);
                    return true;
                }

                return false;
            }

            // Button is down but we have no recorded path inside this app
            // (e.g. drag started outside). Do not dispatch anything.
            return false;
        }

        // No gesture in progress: regular hover move using hit-test.
        // Diff against previous hover set to synthesize Enter/Exit events.
        let hits = self.renderer.scene().hit_test(x, y);
        let new_ids: Vec<NodeId> = hits.iter().map(|h| h.node_id()).collect();

        // Dispatch Exit to nodes that are no longer hovered
        let pos = Point { x, y };
        let previously_hovered = self.hovered_nodes.clone();
        for old_id in previously_hovered {
            if !new_ids.contains(&old_id)
                && let Some(target) = self.renderer.scene().find_target(old_id)
            {
                let exit_event = self
                    .pointer_event(PointerEventKind::Exit, pos, pos, event_time)
                    .with_buttons(self.buttons_pressed)
                    .with_source(self.pointer_source);
                self.dispatch_targets(std::iter::once(target), exit_event, false);
            }
        }

        // Dispatch Enter to newly hovered nodes
        for hit in &hits {
            if !self.hovered_nodes.contains(&hit.node_id()) {
                let enter_event = self
                    .pointer_event(PointerEventKind::Enter, pos, pos, event_time)
                    .with_buttons(self.buttons_pressed)
                    .with_source(self.pointer_source);
                self.dispatch_targets(std::iter::once(hit.clone()), enter_event, false);
            }
        }

        self.hovered_nodes = new_ids;

        if !hits.is_empty() {
            let event = self
                .pointer_event(PointerEventKind::Move, pos, pos, event_time)
                .with_buttons(self.buttons_pressed)
                .with_source(self.pointer_source);
            self.dispatch_targets(hits, event, true);
            true
        } else {
            false
        }
    }

    pub fn pointer_pressed(&mut self) -> bool {
        self.pointer_pressed_at_time(None)
    }

    /// Like [`pointer_pressed`](Self::pointer_pressed), but carries the
    /// platform input timestamp (milliseconds) of the press sample.
    pub fn pointer_pressed_at_time(&mut self, time_ms: Option<i64>) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.pointer_pressed_at_event_time(event_time)
    }

    /// Dispatch primary-button down with an already resolved event timestamp.
    pub fn pointer_pressed_at_event_time(&mut self, event_time: PointerEventTime) -> bool {
        // The dev overlay is drawn over the composition and is not part of it,
        // so it gets the press first and keeps it. Nothing below it is armed:
        // no button state, no hit path, so the matching release is inert.
        if self.dev_overlay_press(self.cursor.0, self.cursor.1) {
            return true;
        }
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| self.pointer_pressed_inner(event_time)).unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_pressed time_ms={:?} animation_time_nanos={} -> {result}",
            event_time.platform_time_ms,
            event_time.animation_time_nanos,
        );
        result
    }

    fn pointer_pressed_inner(&mut self, event_time: PointerEventTime) -> bool {
        // Track button state
        self.buttons_pressed.insert(PointerButton::Primary);

        // Hit-test against the current (last rendered) scene.
        // Even if the app is dirty, this scene is what the user actually saw and clicked.
        // Frame N is rendered → user sees frame N and taps → we hit-test frame N's geometry.
        // The pointer event may mark dirty → next frame runs update() → renders N+1.

        // Perform hit test and cache the NodeIds (not geometry!)
        // The key insight from Jetpack Compose: cache identity, resolve fresh geometry per dispatch
        let hits = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1);
        if hits.is_empty() {
            self.hit_path_tracker.remove_path(PointerId::PRIMARY);
            false
        } else {
            let event = self
                .pointer_event(
                    PointerEventKind::Down,
                    Point {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    },
                    Point {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    },
                    event_time,
                )
                .with_buttons(self.buttons_pressed)
                .with_source(self.pointer_source);

            let mut delivered_capture_paths = Vec::new();
            let mut applier = self.composition.applier_mut();
            for hit in hits {
                let node_id = hit.node_id();
                delivered_capture_paths.push(hit.capture_path());
                hit.dispatch_with_applier(&mut applier, event.clone());
                log::trace!(
                    target: "cranpose::input",
                    "dispatch {:?} node={} consumed={} stop_on_consume=true",
                    event.kind,
                    node_id,
                    event.is_consumed(),
                );
                if event.is_consumed() {
                    break;
                }
            }

            self.hit_path_tracker
                .add_hit_path(PointerId::PRIMARY, delivered_capture_paths);
            log::trace!(
                target: "cranpose::input",
                "pointer_pressed_inner cached_hit_path={:?}",
                self.hit_path_tracker.get_path(PointerId::PRIMARY),
            );

            true
        }
    }

    pub fn pointer_released(&mut self) -> bool {
        self.pointer_released_at_time(None)
    }

    /// Releases the pointer at the position carried by the platform's release
    /// sample (Android `ACTION_UP`, web `pointerup`/`touchend`).
    ///
    /// The cursor is moved to `(x, y)` WITHOUT dispatching a Move event, then
    /// the Up event is dispatched at that position. Platforms whose release
    /// events carry their own coordinates must use this instead of
    /// `set_cursor* + pointer_released*`: lift-off samples routinely roll back
    /// a few dp against the travel direction as the finger peels off, and
    /// feeding that jitter into gesture velocity trackers as a final Move
    /// sample can flip the sign of the computed fling velocity (flings that
    /// suddenly go the opposite way). Jetpack Compose likewise never feeds the
    /// up sample into velocity tracking.
    pub fn pointer_released_at_position(&mut self, x: f32, y: f32) -> bool {
        self.pointer_released_at_position_time(x, y, None)
    }

    /// Like [`pointer_released_at_position`](Self::pointer_released_at_position),
    /// but carries the platform input timestamp (milliseconds) of the release
    /// sample.
    pub fn pointer_released_at_position_time(
        &mut self,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.pointer_released_at_position_event_time(x, y, event_time)
    }

    /// Release at a position with an already resolved event timestamp.
    pub fn pointer_released_at_position_event_time(
        &mut self,
        x: f32,
        y: f32,
        event_time: PointerEventTime,
    ) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| {
                self.cursor = (x, y);
                self.pointer_released_inner(event_time)
            })
            .unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_released_at_position ({x:.2},{y:.2}) time_ms={:?} animation_time_nanos={} -> {result}",
            event_time.platform_time_ms,
            event_time.animation_time_nanos,
        );
        result
    }

    /// Like [`pointer_released`](Self::pointer_released), but carries the
    /// platform input timestamp (milliseconds) of the release sample.
    pub fn pointer_released_at_time(&mut self, time_ms: Option<i64>) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.pointer_released_at_event_time(event_time)
    }

    /// Dispatch primary-button up with an already resolved event timestamp.
    pub fn pointer_released_at_event_time(&mut self, event_time: PointerEventTime) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| self.pointer_released_inner(event_time)).unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_released time_ms={:?} animation_time_nanos={} -> {result}",
            event_time.platform_time_ms,
            event_time.animation_time_nanos,
        );
        result
    }

    fn pointer_released_inner(&mut self, event_time: PointerEventTime) -> bool {
        // UP events report buttons as "currently pressed" (after release),
        // matching typical platform semantics where primary is already gone.
        self.buttons_pressed.remove(PointerButton::Primary);
        let corrected_buttons = self.buttons_pressed;
        let targets = self.resolve_gesture_targets(PointerId::PRIMARY);

        // Always remove the path, even if targets is empty (node may have been removed)
        self.hit_path_tracker.remove_path(PointerId::PRIMARY);

        if !targets.is_empty() {
            let event = self
                .pointer_event(
                    PointerEventKind::Up,
                    Point {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    },
                    Point {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    },
                    event_time,
                )
                .with_buttons(corrected_buttons)
                .with_source(self.pointer_source);

            self.dispatch_targets(targets, event, false);
            true
        } else {
            false
        }
    }

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
        pointer_id: u64,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.dispatch_secondary_pointer(PointerEventKind::Down, pointer_id, x, y, event_time)
    }

    /// Move counterpart of [`secondary_pointer_pressed`](Self::secondary_pointer_pressed).
    pub fn secondary_pointer_moved(
        &mut self,
        pointer_id: u64,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.dispatch_secondary_pointer(PointerEventKind::Move, pointer_id, x, y, event_time)
    }

    /// Release counterpart of [`secondary_pointer_pressed`](Self::secondary_pointer_pressed).
    pub fn secondary_pointer_released(
        &mut self,
        pointer_id: u64,
        x: f32,
        y: f32,
        time_ms: Option<i64>,
    ) -> bool {
        let event_time = self.realtime_pointer_event_time(time_ms);
        self.dispatch_secondary_pointer(PointerEventKind::Up, pointer_id, x, y, event_time)
    }

    fn dispatch_secondary_pointer(
        &mut self,
        kind: PointerEventKind,
        pointer_id: u64,
        x: f32,
        y: f32,
        event_time: PointerEventTime,
    ) -> bool {
        if pointer_id == 0 {
            log::warn!(
                target: "cranpose::input",
                "secondary pointer dispatch called with the primary pointer id"
            );
            return false;
        }

        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| {
                if !self.hit_path_tracker.has_path(PointerId::PRIMARY) {
                    return false;
                }
                let targets = self.resolve_gesture_targets(PointerId::PRIMARY);
                if targets.is_empty() {
                    return false;
                }
                let pos = Point { x, y };
                let event = self
                    .pointer_event(kind, pos, pos, event_time)
                    .with_buttons(self.buttons_pressed)
                    .with_id(pointer_id)
                    .with_source(self.pointer_source);
                self.dispatch_targets(targets, event, false);
                true
            })
            .unwrap_or(false)
        });
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

    /// Dispatches a discrete zoom step (desktop ctrl+wheel, browser pinch)
    /// to the pointer handlers under the cursor.
    ///
    /// `zoom_factor` is multiplicative: `> 1.0` zooms in, `< 1.0` zooms out.
    /// Returns `true` if a handler consumed the event.
    pub fn pointer_zoomed(&mut self, zoom_factor: f32) -> bool {
        let event_time = self.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| self.pointer_zoomed_inner(zoom_factor, event_time))
                .unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_zoomed factor={zoom_factor:.4} -> {result}"
        );
        result
    }

    fn pointer_zoomed_inner(&mut self, zoom_factor: f32, event_time: PointerEventTime) -> bool {
        if !zoom_factor.is_finite() || zoom_factor <= 0.0 || zoom_factor == 1.0 {
            return false;
        }

        let hits = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1);
        if hits.is_empty() {
            return false;
        }

        let pos = Point {
            x: self.cursor.0,
            y: self.cursor.1,
        };
        let event = self
            .pointer_event(PointerEventKind::Zoom, pos, pos, event_time)
            .with_buttons(self.buttons_pressed)
            .with_zoom_delta(zoom_factor)
            .with_source(self.pointer_source);

        let capture_paths = hits
            .iter()
            .map(|hit| hit.capture_path())
            .collect::<Vec<_>>();
        let targets = crate::hit_path_tracker::dispatch_order_for_paths(&capture_paths)
            .into_iter()
            .filter_map(|node_id| self.renderer.scene().find_target(node_id))
            .collect::<Vec<_>>();

        self.dispatch_targets(targets, event.clone(), true);

        event.is_consumed()
    }

    /// Dispatches one mouse-wheel / trackpad sample through the whole wheel
    /// policy, and returns `true` when something consumed it.
    ///
    /// This is the single entry point every host with a wheel calls, after
    /// placing the cursor. A wheel sample is not just a scroll — it is whichever
    /// of four things the modifiers and the tree make it, in this order:
    ///
    /// 1. **Zoom** when ctrl is held. That is the desktop convention and the
    ///    way browsers deliver a trackpad pinch, so both arrive here as the
    ///    same gesture.
    /// 2. **Rotary**, offered to [`rotary_scrolled`](Self::rotary_scrolled)
    ///    before anything else can take it, so the Wear OS crown stack is
    ///    developable on a machine with a wheel. Nothing consumes rotary unless
    ///    the app opts in via `Modifier::on_rotary_scroll_event` or
    ///    [`set_on_rotary_scroll`](Self::set_on_rotary_scroll), so ordinary
    ///    scrolling is unaffected.
    /// 3. **Horizontal scroll** when alt is held on a wheel that only reports a
    ///    vertical axis.
    /// 4. **Scroll**, to the hovered scrollable.
    ///
    /// Hosts must not re-implement this order. Doing so is how the browser
    /// ended up scrolling backwards and never delivering rotary at all: the
    /// policy lived in the desktop event loop, and the second host that grew a
    /// wheel reimplemented the parts of it that were obvious from the outside.
    pub fn wheel_scrolled(&mut self, wheel: crate::WheelScroll) -> bool {
        if wheel.is_zoom() {
            let zoom_factor = wheel.zoom_factor();
            log::trace!(
                target: "cranpose::input",
                "wheel zoom factor={zoom_factor:.4}"
            );
            return self.pointer_zoomed(zoom_factor);
        }

        let rotary =
            RotaryScrollEvent::from_wheel_pixels(wheel.delta.y, wheel.delta.x, wheel.uptime_millis);
        if self.rotary_scrolled(rotary) {
            return true;
        }

        let delta = wheel.scroll_delta();
        log::trace!(
            target: "cranpose::input",
            "wheel delta ({:.2},{:.2}) alt={}",
            delta.x,
            delta.y,
            wheel.modifiers.alt
        );
        self.pointer_scrolled(delta.x, delta.y)
    }

    /// Dispatches a mouse wheel / trackpad scroll event to hovered pointer handlers.
    ///
    /// Returns `true` if a handler consumed the event.
    ///
    /// This is the last step of the wheel policy, not its entry point: hosts
    /// call [`wheel_scrolled`](Self::wheel_scrolled), which reaches here once
    /// zoom and rotary have declined the sample.
    pub fn pointer_scrolled(&mut self, delta_x: f32, delta_y: f32) -> bool {
        let event_time = self.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| self.pointer_scrolled_inner(delta_x, delta_y, event_time))
                .unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "pointer_scrolled ({delta_x:.2},{delta_y:.2}) -> {result}"
        );
        result
    }

    fn pointer_scrolled_inner(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        event_time: PointerEventTime,
    ) -> bool {
        if delta_x.abs() <= f32::EPSILON && delta_y.abs() <= f32::EPSILON {
            return false;
        }

        let hits = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1);
        if hits.is_empty() {
            return false;
        }

        let event = self
            .pointer_event(
                PointerEventKind::Scroll,
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
                Point {
                    x: self.cursor.0,
                    y: self.cursor.1,
                },
                event_time,
            )
            .with_buttons(self.buttons_pressed)
            .with_scroll_delta(Point {
                x: delta_x,
                y: delta_y,
            })
            .with_source(self.pointer_source);

        let capture_paths = hits
            .iter()
            .map(|hit| hit.capture_path())
            .collect::<Vec<_>>();
        let targets = crate::hit_path_tracker::dispatch_order_for_paths(&capture_paths)
            .into_iter()
            .filter_map(|node_id| self.renderer.scene().find_target(node_id))
            .collect::<Vec<_>>();

        self.dispatch_targets(targets, event.clone(), true);

        event.is_consumed()
    }

    /// Installs the window-level rotary (Wear OS crown / rotating bezel)
    /// handler — the low-level escape hatch.
    ///
    /// The handler runs only after the routed modifier chain has declined the
    /// event (see [`rotary_scrolled`](Self::rotary_scrolled)), so an app that
    /// draws everything into a single canvas receives every rotary delta
    /// without registering a focus target or a modifier. Returning `true`
    /// reports the event as consumed to the platform.
    ///
    /// Passing a new handler replaces the previous one.
    pub fn set_on_rotary_scroll<F>(&mut self, handler: F)
    where
        F: Fn(RotaryScrollEvent) -> bool + 'static,
    {
        self.on_rotary_scroll = Some(Rc::new(handler));
    }

    /// Removes the window-level rotary handler, if one is installed.
    pub fn clear_on_rotary_scroll(&mut self) {
        self.on_rotary_scroll = None;
    }

    /// Pixels per rotary detent used by
    /// [`rotary_scrolled_by_detents`](Self::rotary_scrolled_by_detents).
    pub fn rotary_scroll_factor(&self) -> f32 {
        self.rotary_scroll_factor
    }

    /// Sets the pixels-per-detent factor for rotary input.
    ///
    /// On Wear OS this must be `ViewConfiguration.getScaledVerticalScrollFactor()`
    /// for pixel-exact parity with Compose. The host activity can read it over
    /// JNI once at startup and push it here; when it does not, the shell falls
    /// back to [`DEFAULT_ROTARY_SCROLL_FACTOR_DP`] scaled by display density.
    ///
    /// Non-finite or non-positive values are ignored.
    pub fn set_rotary_scroll_factor(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.rotary_scroll_factor = factor;
        }
    }

    /// Dispatches a rotary scroll expressed in raw detents (Android
    /// `AXIS_SCROLL`), converting to pixels with the configured scroll factor.
    ///
    /// Applies Compose's sign convention: a positive detent value (crown turned
    /// up/away) produces a negative `vertical_scroll_pixels`.
    pub fn rotary_scrolled_by_detents(&mut self, detents: f32, uptime_millis: u64) -> bool {
        let factor = self.rotary_scroll_factor;
        self.rotary_scrolled(RotaryScrollEvent::from_detents(
            detents,
            factor,
            factor,
            uptime_millis,
        ))
    }

    /// Dispatches a rotary scroll event (Wear OS crown, Galaxy Watch bezel, or
    /// a desktop mouse wheel standing in for one during development).
    ///
    /// Routing mirrors Compose's `RotaryInputModifierNode` contract:
    ///
    /// 1. Resolve the target chain. When a focus target is registered
    ///    (`cranpose_ui::focus_dispatch::active_focus_target`) and still
    ///    exists in the current scene, its capture path is used, so rotary goes
    ///    to the focused node exactly as on Wear OS. Cranpose does not yet wire
    ///    focus automatically, so in practice this falls back to the chain
    ///    under the current cursor position.
    /// 2. **Capture pass**, root to leaf, invoking `on_pre_rotary_scroll_event`
    ///    handlers.
    /// 3. **Bubble pass**, leaf to root, invoking `on_rotary_scroll_event`
    ///    handlers.
    /// 4. If still unconsumed, the window-level handler installed by
    ///    [`set_on_rotary_scroll`](Self::set_on_rotary_scroll).
    ///
    /// The first handler returning `true` consumes the event and stops every
    /// remaining step. Returns `true` when the event was consumed.
    pub fn rotary_scrolled(&mut self, event: RotaryScrollEvent) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let result = app_context.enter(|| {
            run_in_mutable_snapshot(|| self.rotary_scrolled_inner(event)).unwrap_or(false)
        });
        if result {
            self.mark_dirty();
        }
        log::trace!(
            target: "cranpose::input",
            "rotary_scrolled v={:.2} h={:.2} uptime={} -> {result}",
            event.vertical_scroll_pixels,
            event.horizontal_scroll_pixels,
            event.uptime_millis,
        );
        result
    }

    fn rotary_scrolled_inner(&mut self, rotary: RotaryScrollEvent) -> bool {
        if rotary.is_empty() {
            return false;
        }

        // Leaf-first dispatch order (children before their ancestors), the
        // same ordering pointer events use.
        let bubble_order = self.rotary_dispatch_order();
        let position = Point {
            x: self.cursor.0,
            y: self.cursor.1,
        };

        if !bubble_order.is_empty() {
            // Capture pass: root -> leaf, so ancestors can intercept first.
            let capture_targets = bubble_order
                .iter()
                .rev()
                .filter_map(|&node_id| self.renderer.scene().find_target(node_id))
                .collect::<Vec<_>>();
            let mut capture_event =
                PointerEvent::rotary(PointerEventKind::RotaryScrollPre, rotary, position);
            capture_event.modifiers = self.modifiers;
            self.dispatch_targets(capture_targets, capture_event.clone(), true);
            if capture_event.is_consumed() {
                return true;
            }

            // Bubble pass: leaf -> root.
            let bubble_targets = bubble_order
                .iter()
                .filter_map(|&node_id| self.renderer.scene().find_target(node_id))
                .collect::<Vec<_>>();
            let mut bubble_event =
                PointerEvent::rotary(PointerEventKind::RotaryScroll, rotary, position);
            bubble_event.modifiers = self.modifiers;
            self.dispatch_targets(bubble_targets, bubble_event.clone(), true);
            if bubble_event.is_consumed() {
                return true;
            }
        }

        // Window-level escape hatch for single-canvas apps.
        if let Some(handler) = self.on_rotary_scroll.clone() {
            return handler(rotary);
        }

        false
    }

    /// Resolves the leaf-to-root node order rotary events are dispatched over.
    ///
    /// Prefers the focused node's capture path; falls back to the chain under
    /// the current cursor so rotary remains usable on a build where nothing has
    /// claimed focus (the common case today).
    fn rotary_dispatch_order(&self) -> Vec<NodeId> {
        if let Some(focused) = cranpose_ui::active_focus_target()
            && let Some(target) = self.renderer.scene().find_target(focused)
        {
            let path = target.capture_path();
            if !path.is_empty() {
                return crate::hit_path_tracker::dispatch_order_for_paths(&[path]);
            }
        }

        let hits = self.renderer.scene().hit_test(self.cursor.0, self.cursor.1);
        if hits.is_empty() {
            return Vec::new();
        }
        let capture_paths = hits
            .iter()
            .map(|hit| hit.capture_path())
            .collect::<Vec<_>>();
        crate::hit_path_tracker::dispatch_order_for_paths(&capture_paths)
    }

    /// Cancels any active gesture, dispatching Cancel events to cached targets.
    /// Call this when:
    /// - Window loses focus
    /// - Mouse leaves window while button pressed
    /// - Any other gesture abort scenario
    pub fn cancel_gesture(&mut self) {
        let event_time = self.realtime_pointer_event_time(None);
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        let _ = app_context.enter(|| {
            run_in_mutable_snapshot(|| {
                self.cancel_gesture_inner(event_time);
            })
        });
    }

    fn cancel_gesture_inner(&mut self, event_time: PointerEventTime) {
        let targets = self.resolve_gesture_targets(PointerId::PRIMARY);

        // Clear tracker and button state
        self.hit_path_tracker.clear();
        self.buttons_pressed = PointerButtons::NONE;

        if !targets.is_empty() {
            let event = self
                .pointer_event(
                    PointerEventKind::Cancel,
                    Point {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    },
                    Point {
                        x: self.cursor.0,
                        y: self.cursor.1,
                    },
                    event_time,
                )
                .with_source(self.pointer_source);

            self.dispatch_targets(targets, event, false);
        }

        // Dispatch Exit to all previously hovered nodes
        let pos = Point {
            x: self.cursor.0,
            y: self.cursor.1,
        };
        let hovered_nodes = self.hovered_nodes.clone();
        for node_id in hovered_nodes {
            if let Some(target) = self.renderer.scene().find_target(node_id) {
                let exit_event = self
                    .pointer_event(PointerEventKind::Exit, pos, pos, event_time)
                    .with_source(self.pointer_source);
                self.dispatch_targets(std::iter::once(target), exit_event, false);
            }
        }
        self.hovered_nodes.clear();
    }

    /// Installs the platform soft-keyboard handler for this shell's app context.
    ///
    /// The handler is invoked when a text field gains focus (`show_keyboard`)
    /// or when text-field focus is cleared or goes stale (`hide_keyboard`).
    /// Platform runtimes with an on-screen keyboard (Android, iOS) call this
    /// once after creating the shell.
    pub fn set_platform_text_input(
        &mut self,
        handler: Rc<dyn cranpose_ui::PlatformTextInputHandler>,
    ) {
        let app_context = Rc::clone(&self.app_context);
        app_context
            .enter(|| cranpose_ui::text_input_session::set_platform_text_input_handler(handler));
    }

    /// Notifies the framework that the host app was paused/backgrounded.
    ///
    /// Withdraws any outstanding soft-keyboard request (and hides the keyboard)
    /// so the "keyboard shown" state does not survive across the pause and get
    /// restored on resume with no focused field. Platform runtimes call this
    /// from their pause lifecycle event.
    pub fn notify_app_paused(&mut self) {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::text_input_session::notify_app_paused);
    }

    /// Notifies the framework that the host app resumed/foregrounded.
    ///
    /// Never auto-shows the soft keyboard, even for a still-focused field: a
    /// warm resume keeps the caret but must not resurrect the keyboard (the user
    /// taps the field to bring it back). Always returns `false` so the platform
    /// runtime force-hides the OS-restored keyboard. Platform runtimes call this
    /// from their resume lifecycle event.
    pub fn notify_app_resumed(&mut self) -> bool {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::text_input_session::notify_app_resumed)
    }

    /// Routes a keyboard event to the focused text field, if any.
    ///
    /// Returns `true` if the event was consumed by a text field.
    ///
    /// On desktop, Ctrl+C/X/V are handled here when native clipboard support is enabled.
    /// On web, these keys are NOT handled here - they bubble to browser for native copy/paste events.
    pub fn on_key_event(&mut self, event: &KeyEvent) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_key_event_inner(event))
    }

    /// Internal keyboard event handler wrapped by on_key_event.
    fn on_key_event_inner(&mut self, event: &KeyEvent) -> bool {
        use KeyEventType::KeyDown;

        // Only process KeyDown events for clipboard shortcuts
        if event.event_type == KeyDown && event.modifiers.command_or_ctrl() {
            #[cfg(all(
                feature = "clipboard-native",
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            {
                match event.key_code {
                    // Ctrl+C - Copy
                    KeyCode::C => {
                        if let Some(text) = self.on_copy_inner() {
                            cranpose_ui::clipboard_session::clipboard_write_text(&text);
                            return true;
                        }
                    }
                    // Ctrl+X - Cut
                    KeyCode::X => {
                        if let Some(text) = self.on_cut_inner() {
                            cranpose_ui::clipboard_session::clipboard_write_text(&text);
                            self.mark_dirty();
                            self.request_layout_pass();
                            return true;
                        }
                    }
                    // Ctrl+V - Paste
                    KeyCode::V => {
                        if let Some(text) = cranpose_ui::clipboard_session::clipboard_read_text()
                            && self.on_paste_inner(&text)
                        {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Pure O(1) dispatch - no tree walking needed
        if !cranpose_ui::text_field_focus::has_focused_field() {
            return false;
        }

        // Wrap key event handling in a mutable snapshot so changes are atomically applied.
        // This ensures keyboard input modifications are visible to subsequent snapshot contexts
        // (like button click handlers that run in their own mutable snapshots).
        let handled = run_in_mutable_snapshot(|| {
            // O(1) dispatch via stored handler - handles ALL text input key events
            // No fallback needed since handler now handles arrows, Home/End, word nav
            cranpose_ui::text_field_focus::dispatch_key_event(event)
        })
        .unwrap_or(false);

        if handled {
            // Mark both dirty (for redraw) and request a layout pass to rebuild semantics.
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }

    /// Handles paste event from platform clipboard.
    /// Returns `true` if the paste was consumed by a focused text field.
    /// O(1) operation using stored handler.
    pub fn on_paste(&mut self, text: &str) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_paste_inner(text))
    }

    fn on_paste_inner(&mut self, text: &str) -> bool {
        // Wrap paste in a mutable snapshot so changes are atomically applied.
        // This ensures paste modifications are visible to subsequent snapshot contexts
        // (like button click handlers that run in their own mutable snapshots).
        let handled =
            run_in_mutable_snapshot(|| cranpose_ui::text_field_focus::dispatch_paste(text))
                .unwrap_or(false);

        if handled {
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }

    /// Handles copy request from platform.
    /// Returns the selected text from focused text field, or None.
    /// O(1) operation using stored handler.
    pub fn on_copy(&mut self) -> Option<String> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_copy_inner())
    }

    fn on_copy_inner(&mut self) -> Option<String> {
        // Use O(1) dispatch instead of tree scan
        cranpose_ui::text_field_focus::dispatch_copy()
    }

    /// Handles cut request from platform.
    /// Returns the cut text from focused text field, or None.
    /// O(1) operation using stored handler.
    pub fn on_cut(&mut self) -> Option<String> {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_cut_inner())
    }

    fn on_cut_inner(&mut self) -> Option<String> {
        let text =
            run_in_mutable_snapshot(cranpose_ui::text_field_focus::dispatch_cut).unwrap_or(None);

        if text.is_some() {
            self.mark_dirty();
            self.request_layout_pass();
        }

        text
    }

    /// Sets the Linux primary selection (for middle-click paste).
    /// This is called when text is selected in a text field.
    /// On non-Linux platforms, this is a no-op.
    #[cfg(all(
        feature = "clipboard-native",
        target_os = "linux",
        not(target_arch = "wasm32")
    ))]
    pub fn set_primary_selection(&mut self, text: &str) {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        if let Some(ref mut clipboard) = self.clipboard {
            let result = clipboard
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text.to_string());
            if let Err(e) = result {
                // Primary selection may not be available on all systems
                log::debug!("Primary selection set failed: {:?}", e);
            }
        }
    }

    #[cfg(not(all(
        feature = "clipboard-native",
        target_os = "linux",
        not(target_arch = "wasm32")
    )))]
    pub fn set_primary_selection(&mut self, _text: &str) {}

    /// Gets text from the Linux primary selection (for middle-click paste).
    /// On non-Linux platforms, returns None.
    #[cfg(all(
        feature = "clipboard-native",
        target_os = "linux",
        not(target_arch = "wasm32")
    ))]
    pub fn get_primary_selection(&mut self) -> Option<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        if let Some(ref mut clipboard) = self.clipboard {
            clipboard
                .get()
                .clipboard(LinuxClipboardKind::Primary)
                .text()
                .ok()
        } else {
            None
        }
    }

    #[cfg(not(all(
        feature = "clipboard-native",
        target_os = "linux",
        not(target_arch = "wasm32")
    )))]
    pub fn get_primary_selection(&mut self) -> Option<String> {
        None
    }

    /// Syncs the current text field selection to PRIMARY (Linux X11).
    /// Call this when selection changes in a text field.
    pub fn sync_selection_to_primary(&mut self) {
        #[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
        {
            if let Some(text) = self.on_copy() {
                self.set_primary_selection(&text);
            }
        }
    }

    /// Handles IME preedit (composition) events.
    /// Called when the input method is composing text (e.g., typing CJK characters).
    ///
    /// - `text`: The current preedit text (empty to clear composition state)
    /// - `cursor`: Optional cursor position within the preedit text (start, end)
    ///
    /// Returns `true` if a text field consumed the event.
    pub fn on_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_ime_preedit_inner(text, cursor))
    }

    fn on_ime_preedit_inner(&mut self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        // Wrap in mutable snapshot for atomic changes
        let handled = run_in_mutable_snapshot(|| {
            cranpose_ui::text_field_focus::dispatch_ime_preedit(text, cursor)
        })
        .unwrap_or(false);

        if handled {
            self.mark_dirty();
            // IME composition changes the visible text, needs layout update
            self.request_layout_pass();
        }

        handled
    }

    /// Finishes the active IME composition, keeping the composed text as
    /// committed text (Android `finishComposingText` semantics).
    /// Returns `true` if a text field consumed the event.
    pub fn on_ime_finish_composing(&mut self) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_ime_finish_composing_inner())
    }

    fn on_ime_finish_composing_inner(&mut self) -> bool {
        let handled =
            run_in_mutable_snapshot(cranpose_ui::text_field_focus::dispatch_ime_finish_composing)
                .unwrap_or(false);

        if handled {
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }

    /// Marks existing text in the focused field as the composing region
    /// without changing it (Android `setComposingRegion` semantics). Offsets
    /// are UTF-8 bytes. Returns `true` if a text field consumed the event.
    pub fn on_ime_set_composing_region(&mut self, start_bytes: usize, end_bytes: usize) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| {
            let handled = run_in_mutable_snapshot(|| {
                cranpose_ui::text_field_focus::dispatch_ime_set_composing_region(
                    start_bytes,
                    end_bytes,
                )
            })
            .unwrap_or(false);

            if handled {
                self.mark_dirty();
                self.request_layout_pass();
            }

            handled
        })
    }

    /// Moves the focused field's selection/caret to `[start_bytes, end_bytes)`
    /// without editing text (Android `InputConnection.setSelection`; the path
    /// Gboard's spacebar-swipe uses to scrub the cursor). Offsets are UTF-8
    /// bytes. Returns `true` if a text field consumed the event.
    pub fn on_ime_set_selection(&mut self, start_bytes: usize, end_bytes: usize) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| {
            let handled = run_in_mutable_snapshot(|| {
                cranpose_ui::text_field_focus::dispatch_ime_set_selection(start_bytes, end_bytes)
            })
            .unwrap_or(false);

            // A selection-only change never reflows text, so it needs a redraw
            // but not a layout pass.
            if handled {
                self.mark_dirty();
            }

            handled
        })
    }

    /// Returns a snapshot of the focused text field's editable state for
    /// platform IMEs (text, selection and composition in UTF-8 bytes), or
    /// `None` when no text field is focused.
    pub fn ime_editor_state(&mut self) -> Option<cranpose_ui::text_field_focus::ImeEditorState> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::text_field_focus::focused_editor_state)
    }

    /// Window-space caret geometry of the focused field for coordinate-based
    /// platform text input (iOS trackpad cursor + tap-to-position), or `None`
    /// when no text field is focused.
    pub fn ime_caret_geometry(
        &mut self,
    ) -> Option<cranpose_ui::text_field_focus::ImeCaretGeometry> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::text_field_focus::focused_caret_geometry)
    }

    /// Clears text-field focus (used by platform IME actions such as
    /// Android's Done). The focus-loss notification hides the soft keyboard.
    pub fn clear_text_field_focus(&mut self) {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(cranpose_ui::text_field_focus::clear_focus);
        self.mark_dirty();
        self.request_layout_pass();
    }

    /// Handles IME delete-surrounding events.
    /// Returns `true` if a text field consumed the event.
    pub fn on_ime_delete_surrounding(&mut self, before_bytes: usize, after_bytes: usize) -> bool {
        let _event_handler = enter_event_handler_scope();
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.on_ime_delete_surrounding_inner(before_bytes, after_bytes))
    }

    fn on_ime_delete_surrounding_inner(&mut self, before_bytes: usize, after_bytes: usize) -> bool {
        let handled = run_in_mutable_snapshot(|| {
            cranpose_ui::text_field_focus::dispatch_delete_surrounding(before_bytes, after_bytes)
        })
        .unwrap_or(false);

        if handled {
            self.mark_dirty();
            self.request_layout_pass();
        }

        handled
    }
}
