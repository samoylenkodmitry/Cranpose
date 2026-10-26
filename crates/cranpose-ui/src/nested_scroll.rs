//! Nested scrolling: a container takes part in the scrolls of the scrollers
//! inside it, as Compose's `Modifier.nestedScroll` and
//! `NestedScrollConnection` do. Pull to refresh and collapsing headers are
//! built on it.
//!
//! A [`NestedScroll`] container provides its connection to the scrollers
//! composed inside it: `vertical_scroll`, `horizontal_scroll` and the lazy
//! lists. Before a scroller moves, the containers around it may take part of
//! the movement, outermost first; after it moved, they may take what it left,
//! innermost first; and when a drag ends, they may take part of its velocity
//! before the scroller flings.

use std::rc::Rc;

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_macros::composable;
use cranpose_ui_layout::Axis;

/// What moved a scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NestedScrollSource {
    /// A finger or a held mouse button.
    Drag,
    /// The motion that continues after a drag.
    Fling,
    /// A mouse wheel or a trackpad.
    Wheel,
}

/// A container's part in the scrolls inside it.
///
/// Amounts follow the content: a positive amount moves the content toward
/// the end of the axis, as a finger dragging down or right does, so a list
/// at its top that is dragged further down has a positive amount left over.
/// Each method returns the part it consumed, of the same sign.
pub trait NestedScrollConnection {
    /// Takes part of `available` before the scroller moves.
    fn on_pre_scroll(&self, _axis: Axis, _available: f32, _source: NestedScrollSource) -> f32 {
        0.0
    }

    /// Takes part of `available`, what the scroller left after it consumed
    /// `consumed`.
    fn on_post_scroll(
        &self,
        _axis: Axis,
        _consumed: f32,
        _available: f32,
        _source: NestedScrollSource,
    ) -> f32 {
        0.0
    }

    /// Takes part of the `velocity` a drag ended with, in content units per
    /// second, before the scroller flings with the rest.
    fn on_pre_fling(&self, _axis: Axis, _velocity: f32) -> f32 {
        0.0
    }
}

/// The containers around a scroller, innermost first.
#[derive(Clone)]
pub(crate) struct NestedScrollParent {
    connection: Rc<dyn NestedScrollConnection>,
    outer: Option<Rc<Self>>,
}

/// The same connections in the same order: a recomposed container that
/// keeps its connection changes nothing under it.
impl PartialEq for NestedScrollParent {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.connection, &other.connection) && self.outer == other.outer
    }
}

impl NestedScrollParent {
    /// An identity for gesture keys: a scroller restarts its gesture when it
    /// moves under another container.
    pub(crate) fn identity(&self) -> usize {
        Rc::as_ptr(&self.connection).cast::<()>() as usize
    }

    /// The outer containers first, then this one.
    pub(crate) fn pre_scroll(&self, axis: Axis, available: f32, source: NestedScrollSource) -> f32 {
        let outer = self
            .outer
            .as_ref()
            .map_or(0.0, |outer| outer.pre_scroll(axis, available, source));
        outer
            + self
                .connection
                .on_pre_scroll(axis, available - outer, source)
    }

    /// This container first, then the outer ones.
    pub(crate) fn post_scroll(
        &self,
        axis: Axis,
        consumed: f32,
        available: f32,
        source: NestedScrollSource,
    ) -> f32 {
        let own = self
            .connection
            .on_post_scroll(axis, consumed, available, source);
        own + self.outer.as_ref().map_or(0.0, |outer| {
            outer.post_scroll(axis, consumed + own, available - own, source)
        })
    }

    /// The outer containers first, then this one.
    pub(crate) fn pre_fling(&self, axis: Axis, velocity: f32) -> f32 {
        let outer = self
            .outer
            .as_ref()
            .map_or(0.0, |outer| outer.pre_fling(axis, velocity));
        outer + self.connection.on_pre_fling(axis, velocity - outer)
    }

    /// `apply` moves the scroller by what the containers leave of `available`
    /// and returns what it consumed; the result is what the containers and
    /// the scroller consumed together.
    pub(crate) fn dispatch(
        &self,
        axis: Axis,
        available: f32,
        source: NestedScrollSource,
        apply: impl FnOnce(f32) -> f32,
    ) -> f32 {
        let before = self.pre_scroll(axis, available, source);
        let consumed = apply(available - before);
        let after = self.post_scroll(axis, consumed, available - before - consumed, source);
        before + consumed + after
    }
}

fn local_nested_scroll() -> CompositionLocal<Option<NestedScrollParent>> {
    crate::thread_state::with_environment(|locals| {
        locals
            .nested_scroll
            .get_or_init(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// The containers around a scroller built in the running composition; none
/// outside a composition.
pub(crate) fn current_nested_scroll_parent() -> Option<NestedScrollParent> {
    cranpose_core::composer_context::current_composer()?;
    local_nested_scroll().current()
}

/// Gives the scrollers composed in `content` the container `connection`,
/// inside the containers around this call.
#[composable]
pub fn NestedScroll(connection: Rc<dyn NestedScrollConnection>, content: impl FnOnce()) {
    let local = local_nested_scroll();
    let parent = NestedScrollParent {
        connection,
        outer: local.current().map(Rc::new),
    };
    CompositionLocalProvider(vec![local.provides(Some(parent))], content);
}
