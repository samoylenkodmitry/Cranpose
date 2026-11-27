// Scroll position, animation, fling and indicator state for `WearScalingLazyColumn`.

use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    MutableState, RuntimeCell, internal::FrameCallbackRegistration, remember,
    rememberMutableStateOf,
};
use cranpose_foundation::{RunningScroll, ScrollId};
use cranpose_ui_graphics::Point;

use super::{IndicatorState, ItemHeights, WearScalingItemInfo, WearScalingLayoutInfo};
use crate::{
    composable,
    round_scaling_list::CentreAnchor,
    round_scroll_indicator::{ScalingList, ThumbLength},
};

/// The scroll position of a [`super::WearScalingLazyColumn`].
///
/// It is a centre anchor — which item is on the centre line and by how much it
/// is offset — because that is the coordinate `ScalingLazyListState` keeps and
/// because a scaling list has no meaningful "first visible item": the item at
/// the top of the screen is the one shrunk to nothing.
#[derive(Clone, Copy)]
pub struct WearScalingListState {
    anchor: MutableState<CentreAnchor>,
    inner: RuntimeCell<WearScalingListInner>,
}

pub(super) struct WearScalingListInner {
    pub(super) layout: Rc<RefCell<WearScalingLayoutInfo>>,
    pub(super) items: Rc<RefCell<Vec<WearScalingItemInfo>>>,
    pub(super) heights: Rc<RefCell<ItemHeights>>,
    pub(super) indicator: Rc<RefCell<IndicatorState>>,
    scroll_animation: RefCell<Option<FrameCallbackRegistration>>,
    // The fling of a swipe that moves the list now and how to stop it.
    fling: RefCell<RunningScroll>,
    pub(super) summary: MutableState<WearScalingListSummary>,
}

/// The observable part of a scaling list's layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WearScalingListSummary {
    /// How many rows the list holds.
    pub item_count: usize,
    /// How many of them the viewport can see.
    pub visible_item_count: usize,
    /// Whether there is anything further down to reach.
    pub can_scroll_forward: bool,
    /// Whether there is anything further up to reach.
    pub can_scroll_backward: bool,
}

impl PartialEq for WearScalingListState {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl WearScalingListState {
    pub(super) fn inner(&self) -> Rc<WearScalingListInner> {
        self.inner.get()
    }

    pub(super) fn id(&self) -> u64 {
        let mut hasher = cranpose_core::hash::default::new();
        self.inner.hash(&mut hasher);
        hasher.finish()
    }

    /// The anchor, read reactively — a scope that reads this recomposes when
    /// the list scrolls.
    pub fn anchor(&self) -> CentreAnchor {
        self.anchor.get()
    }

    /// Sets the anchor, which item sits on the center line and how far it is offset from it. This
    /// scrolls the list.
    pub fn set_anchor(&self, anchor: CentreAnchor) {
        self.anchor.set(anchor);
    }

    /// Stops a running [`Self::animate_scroll_to_item`] and the fling of a
    /// swipe.
    ///
    /// Every other way of moving the list calls this first: a finger, a rotary
    /// crown or a snap always wins against an animation the screen started, so
    /// the list never fights the user. A jump stops a fling too, as Compose's
    /// `scrollToItem` does.
    pub fn cancel_scroll_animation(&self) {
        if let Some(inner) = self.inner.try_get() {
            inner.scroll_animation.borrow_mut().take();
        }
        self.stop_fling();
    }

    // Makes the fling that `stop` stops the list's running fling. The fling
    // that ran before stops.
    pub(super) fn hold_fling(&self, stop: impl FnOnce() + 'static) -> Option<ScrollId> {
        let inner = self.inner.try_get()?;
        let (id, previous) = inner.fling.borrow_mut().start(stop);
        if let Some(previous) = previous {
            previous.run();
        }
        Some(id)
    }

    // Forgets the fling `id` once it ended, while the state lives.
    pub(super) fn end_fling(&self, id: ScrollId) {
        if let Some(inner) = self.inner.try_get() {
            inner.fling.borrow_mut().end(id);
        }
    }

    // Stops the list's running fling.
    pub(super) fn stop_fling(&self) {
        let Some(inner) = self.inner.try_get() else {
            return;
        };
        let fling = inner.fling.borrow_mut().take();
        if let Some(fling) = fling {
            fling.run();
        }
    }

    /// Scrolls by a distance in layout points, positive towards the end of the
    /// list, re-anchoring on whichever item the centre line lands in.
    ///
    /// The re-anchoring matters: leaving the index alone and letting the offset
    /// grow works until the anchored item scrolls off, after which the ramp is
    /// computed from an item nobody can see.
    pub fn scroll_by(&self, delta: f32) -> f32 {
        if !delta.is_finite() || !self.anchor.is_alive() {
            return 0.0;
        }
        let Some(inner) = self.inner.try_get() else {
            return 0.0;
        };
        inner.scroll_animation.borrow_mut().take();
        let info = *inner.layout.borrow();
        let anchor = self.anchor.get();
        let available_before = info.scrolled();
        let available_after = (info.travel() - available_before).max(0.0);
        let applied = delta.clamp(-available_before, available_after);
        if applied == 0.0 {
            return 0.0;
        }
        let items = inner.items.borrow();
        let target = info.viewport * 0.5 + applied;
        let next = items
            .iter()
            .min_by(|left, right| {
                (left.centre - target)
                    .abs()
                    .total_cmp(&(right.centre - target).abs())
            })
            .map_or_else(
                || re_anchor(anchor, applied, info),
                |item| CentreAnchor {
                    index: item.index,
                    offset: info.viewport * 0.5 - item.centre + applied,
                },
            );
        drop(items);
        self.set_anchor(next);
        applied
    }

    /// Scrolls by `delta` layout points, as [`Self::scroll_by`] does, and returns the distance
    /// actually scrolled.
    pub fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        self.scroll_by(delta)
    }

    /// What the last measure pass worked out.
    ///
    /// Read outside composition — during draw, input, or a diagnostic — because
    /// every field of it moves on every scroll frame. A screen reacting to the
    /// list reads [`Self::summary`] and its named parts instead, which change
    /// only when they mean something different.
    pub fn layout_info(&self) -> WearScalingLayoutInfo {
        *self.inner().layout.borrow()
    }

    /// The facts about the list a screen reacts to. Reactive: a scope that
    /// reads this recomposes when one of them changes, and not while the list
    /// merely scrolls.
    pub fn summary(&self) -> WearScalingListSummary {
        self.inner().summary.value()
    }

    /// How many rows the list holds. Reactive.
    pub fn item_count(&self) -> usize {
        self.summary().item_count
    }

    /// How many rows the viewport can see. Reactive.
    pub fn visible_item_count(&self) -> usize {
        self.summary().visible_item_count
    }

    /// Whether there is anything further down to reach. Reactive — this is what
    /// a "scroll to top" button and an end-of-list loader watch.
    pub fn can_scroll_forward(&self) -> bool {
        self.summary().can_scroll_forward
    }

    /// Whether there is anything further up to reach. Reactive.
    pub fn can_scroll_backward(&self) -> bool {
        self.summary().can_scroll_backward
    }

    /// Puts `index` on the centre line at once.
    ///
    /// The position of a scaling list *is* a centre anchor, so this is exact
    /// whether or not the target has ever been measured: the next measure pass
    /// lays the list out around it. `offset` shifts the row off the centre line
    /// by that many points, positive downwards.
    pub fn scroll_to_item(&self, index: usize, offset: f32) {
        self.cancel_scroll_animation();
        let count = self.inner().heights.borrow().len();
        let index = if count == 0 {
            index
        } else {
            index.min(count - 1)
        };
        self.set_anchor(CentreAnchor {
            index,
            offset: if offset.is_finite() { offset } else { 0.0 },
        });
    }

    /// How far `index` is from the centre line right now, in points.
    ///
    /// Exact for a row the last measure pass placed. For a row further away
    /// than that, the distance is estimated from the heights the list has seen,
    /// which is what makes an animated scroll to a far row start moving in the
    /// right direction and at a sensible speed; each frame re-asks, so the
    /// estimate is corrected as the real rows arrive.
    pub fn distance_to_item(&self, index: usize, offset: f32) -> f32 {
        let inner = self.inner();
        let info = *inner.layout.borrow();
        let centre_line = info.viewport * 0.5;
        let offset = if offset.is_finite() { offset } else { 0.0 };
        if let Some(item) = inner
            .items
            .borrow()
            .iter()
            .find(|item| item.index == index)
            .copied()
        {
            return item.centre - offset - centre_line;
        }

        let anchor = self.anchor.get_non_reactive();
        let heights = inner.heights.borrow();
        let spacing_free = signed_span(&heights, anchor.index, index);
        spacing_free - anchor.offset - offset
    }

    /// Whether `index` is a row this list has.
    pub fn contains_item(&self, index: usize) -> bool {
        index < self.inner().heights.borrow().len()
    }

    /// Brings `index` to the centre line over several frames.
    ///
    /// Each frame asks again how far away the row is and closes a fixed
    /// fraction of what is left, so the animation corrects itself as the real
    /// rows between here and there are measured — a list scrolled to its
    /// thousandth row does not have to guess right on the first frame. Any
    /// other scroll cancels it; see [`Self::cancel_scroll_animation`].
    pub fn animate_scroll_to_item(&self, index: usize, offset: f32) {
        self.cancel_scroll_animation();
        if !self.contains_item(index) {
            return;
        }
        self.step_scroll_animation(index, if offset.is_finite() { offset } else { 0.0 });
    }

    fn step_scroll_animation(&self, index: usize, offset: f32) {
        let Some(runtime) = cranpose_core::current_runtime_handle() else {
            self.scroll_to_item(index, offset);
            return;
        };
        let state = *self;
        let registration = runtime.frame_clock().with_frame_nanos(move |_| {
            let Some(inner) = state.inner.try_get() else {
                return;
            };
            if !state.anchor.is_alive() {
                return;
            }
            inner.scroll_animation.borrow_mut().take();
            let remaining = state.distance_to_item(index, offset);
            if remaining.abs() <= SCROLL_ANIMATION_EPSILON {
                state.scroll_to_item(index, offset);
                return;
            }
            let step = remaining * SCROLL_ANIMATION_FRACTION;
            let step = if step.abs() < SCROLL_ANIMATION_MIN_STEP {
                SCROLL_ANIMATION_MIN_STEP.copysign(remaining)
            } else {
                step
            };
            let applied = state.scroll_by(step);
            if applied == 0.0 {
                return;
            }
            state.step_scroll_animation(index, offset);
        });
        if let Some(inner) = self.inner.try_get() {
            *inner.scroll_animation.borrow_mut() = Some(registration);
        }
    }

    /// Rows placed by the last measure pass, with their transformed bounds.
    pub fn visible_items(&self) -> Vec<WearScalingItemInfo> {
        self.inner().items.borrow().clone()
    }

    /// Returns the topmost placed row containing `point`.
    pub fn item_at(&self, point: Point) -> Option<WearScalingItemInfo> {
        self.inner()
            .items
            .borrow()
            .iter()
            .rev()
            .copied()
            .find(|item| item.contains(point))
    }

    /// Reads the list the way `ScalingLazyColumnStateAdapter` reads one, for
    /// [`crate::round_scroll_indicator::scaling_list_geometry`].
    ///
    /// The window is lent to a closure rather than returned because it is a
    /// buffer the measure pass owns and refills; copying it out would put an
    /// allocation per frame inside a draw. The [`ThumbLength`] comes with it
    /// because it is the adapter's own field rather than the caller's — the
    /// thumb is measured once per list and kept — and one list has exactly one.
    pub fn with_indicator_list<R>(
        &self,
        read: impl FnOnce(&mut ThumbLength, ScalingList<'_>) -> R,
    ) -> R {
        let inner = self.inner();
        let mut indicator = inner.indicator.borrow_mut();
        let IndicatorState {
            visible,
            total,
            viewport,
            before_padding,
            after_padding,
            thumb,
        } = &mut *indicator;
        read(
            thumb,
            ScalingList {
                visible: visible.as_slice(),
                total: *total,
                viewport: *viewport,
                before_padding: *before_padding,
                after_padding: *after_padding,
            },
        )
    }
}

// Moves an anchor by `delta` and settles it on the nearest item.
// Kept as a free function so the arithmetic is testable without composing.
pub(super) fn re_anchor(
    anchor: CentreAnchor,
    delta: f32,
    info: WearScalingLayoutInfo,
) -> CentreAnchor {
    let mut anchor = CentreAnchor {
        index: anchor.index,
        offset: anchor.offset + delta,
    };
    if info.item_count == 0 {
        return anchor;
    }
    let travelled = info.scrolled() + delta;
    let travel = info.travel();
    if travelled < 0.0 {
        anchor.offset -= travelled;
    } else if travelled > travel {
        anchor.offset -= travelled - travel;
    }
    anchor
}

// How much of the remaining distance an animated scroll closes each frame.
// A geometric approach settles quickly without the overshoot a spring would
// add to a list whose rows are still being measured underneath it.
const SCROLL_ANIMATION_FRACTION: f32 = 0.22;
// The slowest an animated scroll may crawl, so the last few points do not take
// a visible tail of frames.
const SCROLL_ANIMATION_MIN_STEP: f32 = 1.0;
// Close enough to the centre line to snap and stop.
const SCROLL_ANIMATION_EPSILON: f32 = 0.5;

// The distance from the top of item `from` to the top of item `to`, positive
// when `to` is further down the list. Heights the list has not measured are
// the mean of the ones it has, matching `ScalingLazyListState`.
pub(super) fn signed_span(heights: &ItemHeights, from: usize, to: usize) -> f32 {
    if from == to {
        return 0.0;
    }
    let (low, high) = if from < to { (from, to) } else { (to, from) };
    let span: f32 = (low..high).map(|index| heights.height_of(index)).sum();
    if from < to { span } else { -span }
}

/// Remembers a scaling list's scroll position.
#[composable]
#[track_caller]
pub fn rememberWearScalingListState(initial: CentreAnchor) -> WearScalingListState {
    let anchor = rememberMutableStateOf(move || initial);
    let inner = remember(|| {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("rememberWearScalingListState requires an active runtime");
        RuntimeCell::with_runtime(
            WearScalingListInner {
                layout: Rc::new(RefCell::new(WearScalingLayoutInfo::default())),
                items: Rc::new(RefCell::new(Vec::new())),
                heights: Rc::new(RefCell::new(ItemHeights::default())),
                indicator: Rc::new(RefCell::new(IndicatorState::default())),
                scroll_animation: RefCell::new(None),
                fling: RefCell::new(RunningScroll::default()),
                summary: MutableState::with_runtime(
                    WearScalingListSummary::default(),
                    runtime.clone(),
                ),
            },
            runtime,
        )
    })
    .with(|state| *state);
    WearScalingListState { anchor, inner }
}
