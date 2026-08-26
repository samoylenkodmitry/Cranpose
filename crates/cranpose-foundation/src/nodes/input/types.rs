use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_ui_graphics::Point;

use super::rotary::RotaryScrollEvent;

pub type PointerId = u64;

type PostDispatchAction = Box<dyn FnOnce() -> bool>;

#[derive(Clone)]
struct DeferredPostDispatch {
    action: Rc<RefCell<Option<PostDispatchAction>>>,
}

impl std::fmt::Debug for DeferredPostDispatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeferredPostDispatch")
            .field("is_pending", &self.action.borrow().is_some())
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Start,
    Move,
    End,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEventKind {
    Down,
    Move,
    Up,
    Cancel,
    Scroll,
    /// Discrete zoom step (desktop ctrl+wheel, browser pinch-trackpad).
    /// The multiplicative factor is carried in [`PointerEvent::zoom_delta`].
    Zoom,
    /// Rotary scroll (Wear OS crown / rotating bezel) during the **capture**
    /// pass, which runs root-to-focused so ancestors can intercept the event
    /// before the focused node sees it.
    ///
    /// The scroll amounts are carried in [`PointerEvent::scroll_delta`] (`y` =
    /// vertical pixels, `x` = horizontal pixels) and the rotary uptime in
    /// [`PointerEvent::time_ms`]; use
    /// [`PointerEvent::rotary_scroll_event`] to read them back as a
    /// [`RotaryScrollEvent`]. Mirrors Compose's `onPreRotaryScrollEvent`.
    RotaryScrollPre,
    /// Rotary scroll during the **bubble** pass, which runs focused-to-root.
    /// Mirrors Compose's `onRotaryScrollEvent`.
    RotaryScroll,
    Enter,
    Exit,
}

impl PointerEventKind {
    /// Returns true for the two rotary passes.
    pub fn is_rotary(self) -> bool {
        matches!(self, Self::RotaryScrollPre | Self::RotaryScroll)
    }
}

/// The kind of physical device that produced a pointer event.
///
/// Threaded from the platform layer (Android `MotionEvent` tool type, winit
/// `PointerSource`/`ButtonSource`, web `PointerEvent.pointerType`) so input
/// consumers can preserve device-specific gesture details while keeping shared
/// direct-manipulation behavior source-independent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerSource {
    /// A mouse or other indirect precise pointer (desktop, web `"mouse"`).
    Mouse,
    /// A finger on a touchscreen (Android finger, winit touch, web `"touch"`).
    Touch,
    /// A stylus/pen (Android stylus/eraser, winit tablet tool, web `"pen"`).
    Stylus,
    /// The platform did not report a device type.
    #[default]
    Unknown,
}

impl PointerSource {
    /// Whether this source is a direct-touch device (finger or stylus), used
    /// for contact-specific release and velocity semantics.
    pub fn is_touch_like(self) -> bool {
        matches!(self, PointerSource::Touch | PointerSource::Stylus)
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary = 0,
    Secondary = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerButtons(u8);

impl PointerButtons {
    pub const NONE: Self = Self(0);

    pub fn new() -> Self {
        Self::NONE
    }

    pub fn with(mut self, button: PointerButton) -> Self {
        self.insert(button);
        self
    }

    pub fn insert(&mut self, button: PointerButton) {
        self.0 |= 1 << (button as u8);
    }

    pub fn remove(&mut self, button: PointerButton) {
        self.0 &= !(1 << (button as u8));
    }

    pub fn contains(&self, button: PointerButton) -> bool {
        (self.0 & (1 << (button as u8))) != 0
    }
}

impl Default for PointerButtons {
    fn default() -> Self {
        Self::NONE
    }
}

/// Keyboard modifier keys held during an input sample.
///
/// Lives here (rather than up in `cranpose-ui`, where the keyboard `KeyEvent`
/// type lives) because [`PointerEvent`] needs it too and `cranpose-foundation`
/// sits below `cranpose-ui` in the dependency graph — this is the one crate
/// both a key event and a pointer event can share it from. `cranpose-ui`
/// re-exports this type rather than defining its own, so there is exactly one
/// `Modifiers` in the framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Shift key is pressed.
    pub shift: bool,
    /// Control key is pressed (Cmd on macOS).
    pub ctrl: bool,
    /// Alt key is pressed (Option on macOS).
    pub alt: bool,
    /// Meta/Super key is pressed (Windows key, Cmd on macOS).
    pub meta: bool,
}

impl Modifiers {
    /// No modifiers pressed.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// Returns true if any modifier is pressed.
    pub fn any(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }

    /// Returns true if Ctrl (or Cmd on macOS) is pressed.
    pub fn command_or_ctrl(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.meta
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.ctrl
        }
    }
}

/// Pointer event with consumption tracking for gesture disambiguation.
///
/// Events can be consumed by handlers (e.g., scroll) to prevent other handlers
/// (e.g., clicks) from receiving them. This enables proper gesture disambiguation
/// matching Jetpack Compose's event consumption pattern.
#[derive(Clone, Debug)]
pub struct PointerEvent {
    pub id: PointerId,
    pub kind: PointerEventKind,
    pub phase: PointerPhase,
    pub position: Point,
    pub global_position: Point,
    /// Scroll delta in logical pixels.
    ///
    /// This is non-zero for [`PointerEventKind::Scroll`] events and zero for
    /// button/move events.
    pub scroll_delta: Point,
    pub buttons: PointerButtons,
    /// Platform timestamp of the input sample in milliseconds, when available.
    ///
    /// The time base is platform specific (e.g. Android's uptime clock); only
    /// differences between events of the same gesture are meaningful. Gesture
    /// velocity trackers must prefer this over the delivery time because
    /// platforms like Android deliver input batched/frame-aligned: several
    /// samples arrive back-to-back and delivery-time stamping makes computed
    /// velocities wildly wrong.
    pub time_ms: Option<i64>,
    /// Timestamp in the animation frame-clock domain at input dispatch.
    /// Unlike `time_ms`, this has the same origin as frame callbacks and can
    /// anchor input-driven animations without a wall/platform clock conversion.
    pub animation_time_nanos: Option<u64>,
    /// Multiplicative zoom factor for [`PointerEventKind::Zoom`] events
    /// (`> 1.0` zooms in, `< 1.0` zooms out). `1.0` for all other events.
    pub zoom_delta: f32,
    /// The kind of device that produced this event (touch, mouse, stylus), when
    /// the platform reports it. Defaults to [`PointerSource::Unknown`].
    pub source: PointerSource,
    /// Keyboard modifiers held at the time of this sample, when the platform
    /// can report them.
    ///
    /// `None` means the platform never told the shell what the keyboard state
    /// was — touch-only Android/iOS input has no channel for it today — and is
    /// deliberately distinct from `Some(Modifiers::NONE)`, which means the
    /// platform looked and nothing was held. An app that wants shift/ctrl-click
    /// multi-select reads this field directly; it must not treat `None` as
    /// "nothing held" or it silently drops the gesture on the platforms that
    /// cannot yet report it instead of visibly doing nothing.
    pub modifiers: Option<Modifiers>,
    /// Tracks whether this event has been consumed by a handler.
    /// Shared via `Rc<Cell>` so consumption can be tracked across copies.
    consumed: Rc<Cell<bool>>,
    deferred_post_dispatch: DeferredPostDispatch,
}

impl PointerEvent {
    pub fn new(kind: PointerEventKind, position: Point, global_position: Point) -> Self {
        Self {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move | PointerEventKind::Enter | PointerEventKind::Exit => {
                    PointerPhase::Move
                }
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
                PointerEventKind::Scroll
                | PointerEventKind::Zoom
                | PointerEventKind::RotaryScrollPre
                | PointerEventKind::RotaryScroll => PointerPhase::Move,
            },
            position,
            global_position,
            scroll_delta: Point { x: 0.0, y: 0.0 },
            buttons: PointerButtons::NONE,
            time_ms: None,
            animation_time_nanos: None,
            zoom_delta: 1.0,
            source: PointerSource::Unknown,
            modifiers: None,
            consumed: Rc::new(Cell::new(false)),
            deferred_post_dispatch: DeferredPostDispatch {
                action: Rc::new(RefCell::new(None)),
            },
        }
    }

    /// Set the pointer id for this event (`0` is the primary pointer).
    pub fn with_id(mut self, id: PointerId) -> Self {
        self.id = id;
        self
    }

    /// Set the multiplicative zoom factor for a [`PointerEventKind::Zoom`] event.
    pub fn with_zoom_delta(mut self, zoom_delta: f32) -> Self {
        self.zoom_delta = zoom_delta;
        self
    }

    /// Set the scroll delta for this event.
    pub fn with_scroll_delta(mut self, scroll_delta: Point) -> Self {
        self.scroll_delta = scroll_delta;
        self
    }

    /// Set the platform timestamp (milliseconds) for this event.
    pub fn with_time_ms(mut self, time_ms: Option<i64>) -> Self {
        self.time_ms = time_ms;
        self
    }

    /// Set the timestamp in the animation frame-clock domain.
    pub fn with_animation_time_nanos(mut self, time_nanos: u64) -> Self {
        self.animation_time_nanos = Some(time_nanos);
        self
    }

    /// Set the buttons state for this event
    pub fn with_buttons(mut self, buttons: PointerButtons) -> Self {
        self.buttons = buttons;
        self
    }

    /// Set the device source (touch/mouse/stylus) for this event.
    pub fn with_source(mut self, source: PointerSource) -> Self {
        self.source = source;
        self
    }

    /// Set the keyboard modifiers held during this event, when the platform
    /// can report them. See the [`modifiers`](Self::modifiers) field docs for
    /// why this takes a concrete [`Modifiers`] rather than an `Option`: the
    /// `None` case is the *absence* of a call to this builder, not a value it
    /// produces.
    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = Some(modifiers);
        self
    }

    /// Builds a rotary pointer event for one dispatch pass.
    ///
    /// `kind` must be [`PointerEventKind::RotaryScrollPre`] (capture) or
    /// [`PointerEventKind::RotaryScroll`] (bubble). The rotary payload rides on
    /// the existing `scroll_delta`/`time_ms` fields so rotary reuses the
    /// pointer dispatch path without widening [`PointerEvent`].
    pub fn rotary(kind: PointerEventKind, rotary: RotaryScrollEvent, position: Point) -> Self {
        debug_assert!(
            kind.is_rotary(),
            "PointerEvent::rotary requires a rotary event kind"
        );
        Self::new(kind, position, position)
            .with_scroll_delta(Point {
                x: rotary.horizontal_scroll_pixels,
                y: rotary.vertical_scroll_pixels,
            })
            .with_time_ms(Some(rotary.uptime_millis as i64))
    }

    /// Reads this event back as a [`RotaryScrollEvent`], or `None` when it is
    /// not a rotary event.
    ///
    /// Copies three scalars out of the event; it never allocates.
    pub fn rotary_scroll_event(&self) -> Option<RotaryScrollEvent> {
        if !self.kind.is_rotary() {
            return None;
        }
        Some(RotaryScrollEvent {
            vertical_scroll_pixels: self.scroll_delta.y,
            horizontal_scroll_pixels: self.scroll_delta.x,
            uptime_millis: self.time_ms.unwrap_or(0).max(0) as u64,
        })
    }

    /// Mark this event as consumed, preventing other handlers from processing it.
    ///
    /// Example: Scroll gestures consume events once dragging starts to prevent
    /// child buttons from firing clicks.
    pub fn consume(&self) {
        self.consumed.set(true);
    }

    /// Check if this event has been consumed by another handler.
    ///
    /// Handlers should check this before processing events. For example,
    /// clickable should not fire if the event was consumed by a scroll gesture.
    pub fn is_consumed(&self) -> bool {
        self.consumed.get()
    }

    pub fn defer_post_dispatch_action<F>(&self, action: F)
    where
        F: FnOnce() -> bool + 'static,
    {
        *self.deferred_post_dispatch.action.borrow_mut() = Some(Box::new(action));
    }

    pub fn finish_post_dispatch(&self) {
        if self.is_consumed() {
            self.deferred_post_dispatch.action.borrow_mut().take();
            return;
        }

        let Some(action) = self.deferred_post_dispatch.action.borrow_mut().take() else {
            return;
        };
        if action() {
            self.consume();
        }
    }

    /// Creates a copy of this event with a new local position, sharing the consumption state.
    pub fn copy_with_local_position(&self, position: Point) -> Self {
        Self {
            id: self.id,
            kind: self.kind,
            phase: self.phase,
            position,
            global_position: self.global_position,
            scroll_delta: self.scroll_delta,
            buttons: self.buttons,
            time_ms: self.time_ms,
            animation_time_nanos: self.animation_time_nanos,
            zoom_delta: self.zoom_delta,
            source: self.source,
            modifiers: self.modifiers,
            consumed: self.consumed.clone(),
            deferred_post_dispatch: self.deferred_post_dispatch.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn pointer_event_clones_share_consumed_state() {
        let event = PointerEvent::new(PointerEventKind::Move, point(1.0, 2.0), point(3.0, 4.0));
        let cloned = event.clone();
        assert!(!event.is_consumed());
        assert!(!cloned.is_consumed());

        cloned.consume();

        assert!(event.is_consumed());
        assert!(cloned.is_consumed());
    }

    #[test]
    fn pointer_event_source_defaults_unknown_and_threads_through_copy() {
        let event = PointerEvent::new(PointerEventKind::Down, point(1.0, 1.0), point(1.0, 1.0));
        assert_eq!(event.source, PointerSource::Unknown);
        assert!(!PointerSource::Unknown.is_touch_like());

        let touch = event.with_source(PointerSource::Touch);
        assert_eq!(touch.source, PointerSource::Touch);
        assert!(PointerSource::Touch.is_touch_like());
        assert!(PointerSource::Stylus.is_touch_like());
        assert!(!PointerSource::Mouse.is_touch_like());

        // Local-position copies (used during hit-test dispatch) keep the source.
        let local = touch.copy_with_local_position(point(5.0, 5.0));
        assert_eq!(local.source, PointerSource::Touch);
    }

    #[test]
    fn modifiers_any_is_true_when_any_field_is_set() {
        assert!(!Modifiers::NONE.any());
        assert!(!Modifiers::default().any());
        assert!(
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            }
            .any()
        );
    }

    #[test]
    fn modifiers_command_or_ctrl_reads_the_platform_appropriate_key() {
        let ctrl_only = Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        };
        let meta_only = Modifiers {
            meta: true,
            ..Modifiers::NONE
        };

        #[cfg(target_os = "macos")]
        {
            assert!(!ctrl_only.command_or_ctrl());
            assert!(meta_only.command_or_ctrl());
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(ctrl_only.command_or_ctrl());
            assert!(!meta_only.command_or_ctrl());
        }
        assert!(!Modifiers::NONE.command_or_ctrl());
    }

    #[test]
    fn pointer_event_modifiers_default_to_unreported_and_thread_through_copy() {
        let event = PointerEvent::new(PointerEventKind::Down, point(1.0, 1.0), point(1.0, 1.0));
        // Unreported (None) must stay visibly distinct from "reported, none
        // held" (Some(Modifiers::NONE)) -- see the field doc on why.
        assert_eq!(event.modifiers, None);

        let shift = event.with_modifiers(Modifiers {
            shift: true,
            ..Modifiers::NONE
        });
        assert_eq!(
            shift.modifiers,
            Some(Modifiers {
                shift: true,
                ..Modifiers::NONE
            })
        );

        // Local-position copies (used during hit-test dispatch) keep the
        // modifiers, exactly like they keep the source.
        let local = shift.copy_with_local_position(point(5.0, 5.0));
        assert_eq!(local.modifiers, shift.modifiers);
    }

    #[test]
    fn rotary_payload_round_trips_through_pointer_event() {
        let rotary = RotaryScrollEvent::new(-64.0, 12.0, 1_234);
        let event = PointerEvent::rotary(PointerEventKind::RotaryScroll, rotary, point(5.0, 6.0));

        assert_eq!(event.phase, PointerPhase::Move);
        assert_eq!(event.scroll_delta, point(12.0, -64.0));
        assert_eq!(event.time_ms, Some(1_234));
        assert_eq!(event.rotary_scroll_event(), Some(rotary));
    }

    #[test]
    fn rotary_payload_survives_local_position_copies() {
        // Dispatch localizes the event per node; the rotary payload must
        // survive that copy or handlers deeper in the chain see zeros.
        let rotary = RotaryScrollEvent::new(-8.0, 0.0, 7);
        let event =
            PointerEvent::rotary(PointerEventKind::RotaryScrollPre, rotary, point(0.0, 0.0));

        let local = event.copy_with_local_position(point(3.0, 4.0));

        assert_eq!(local.rotary_scroll_event(), Some(rotary));
    }

    #[test]
    fn non_rotary_events_have_no_rotary_payload() {
        let scroll = PointerEvent::new(PointerEventKind::Scroll, point(0.0, 0.0), point(0.0, 0.0))
            .with_scroll_delta(point(1.0, 2.0));

        assert_eq!(scroll.rotary_scroll_event(), None);
        assert!(!PointerEventKind::Scroll.is_rotary());
        assert!(PointerEventKind::RotaryScroll.is_rotary());
        assert!(PointerEventKind::RotaryScrollPre.is_rotary());
    }

    #[test]
    fn pointer_event_copy_with_local_position_preserves_consumption_state() {
        let event = PointerEvent::new(PointerEventKind::Down, point(4.0, 5.0), point(4.0, 5.0))
            .with_time_ms(Some(123))
            .with_animation_time_nanos(456_000_000);
        let local = event.copy_with_local_position(point(1.0, 1.0));

        assert_eq!(local.position, point(1.0, 1.0));
        assert_eq!(local.global_position, event.global_position);
        assert_eq!(local.time_ms, Some(123));
        assert_eq!(local.animation_time_nanos, Some(456_000_000));
        assert!(!local.is_consumed());

        event.consume();

        assert!(local.is_consumed());
    }

    #[test]
    fn deferred_post_dispatch_action_consumes_when_it_applies() {
        let event = PointerEvent::new(PointerEventKind::Move, point(0.0, 0.0), point(0.0, 0.0));
        event.defer_post_dispatch_action(|| true);

        event.finish_post_dispatch();

        assert!(event.is_consumed());
    }

    #[test]
    fn deferred_post_dispatch_action_is_replaced_and_discarded_when_consumed() {
        let event = PointerEvent::new(PointerEventKind::Move, point(0.0, 0.0), point(0.0, 0.0));
        let first_called = Rc::new(Cell::new(false));
        let second_called = Rc::new(Cell::new(false));
        let first_called_in_action = first_called.clone();
        let second_called_in_action = second_called.clone();
        event.defer_post_dispatch_action(move || {
            first_called_in_action.set(true);
            true
        });
        event.defer_post_dispatch_action(move || {
            second_called_in_action.set(true);
            true
        });
        event.consume();

        event.finish_post_dispatch();

        assert!(!first_called.get());
        assert!(!second_called.get());
        assert!(event.is_consumed());
    }
}
