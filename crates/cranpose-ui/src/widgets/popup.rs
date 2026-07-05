//! Top-level overlay / `Popup` primitive.
//!
//! Compose parity with `androidx.compose.ui.window.Popup`: content composed
//! inside a [`Popup`] renders in a top-level overlay that draws above all
//! normal content and is **not** clipped by the bounds of the ancestor the
//! call site sits under. This is what lets a text field's selection handles
//! hang below the last line, and a contextual menu float above a selection,
//! without being cut off by a scrolling parent's clip rectangle.
//!
//! # How it works
//!
//! Composition is single-rooted and both paint order and hit-test order are
//! derived purely from tree position (later sibling = on top, and a node is
//! only clipped by an ancestor that opted into `clip_to_bounds`). Therefore the
//! only way for content to draw above *everything* and escape *any* ancestor
//! clip is to be composed as a last-order sibling directly under an unclipped
//! root. [`PopupHost`] provides exactly that root: it wraps the whole app in an
//! unclipped, viewport-filling [`Box`] and renders every registered popup as a
//! trailing child, positioned absolutely at its anchor.
//!
//! [`Popup`] itself emits **no node at its call site**. Instead it registers
//! its `(position, content)` into a [`PopupRegistry`] carried down the tree by
//! a [`CompositionLocal`]; the enclosing [`PopupHost`] reads that registry and
//! composes the content at the root. Registration/teardown is reactive:
//! adding, removing, or moving a popup invalidates the host so it recomposes.

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::composable;
use crate::modifier::Modifier;
use cranpose_core::{
    mutableStateOf, remember, staticCompositionLocalOf, CompositionLocalProvider, MutableState,
    SideEffect, StaticCompositionLocal,
};
use cranpose_ui_graphics::{Point, Rect};

use super::box_widget::{Box, BoxSpec};

/// One registered popup: a stable id, its absolute top-left position (logical
/// px, in [`PopupHost`] space, i.e. window coordinates) and its content.
#[derive(Clone)]
struct PopupEntry {
    id: u64,
    position: Point,
    content: Rc<dyn Fn()>,
}

struct PopupRegistryState {
    entries: RefCell<Vec<PopupEntry>>,
    next_id: Cell<u64>,
    /// Reactive dirtiness signal. `Some` for a hosted registry (created by a
    /// [`PopupHost`]); `None` for the detached default registry used when no
    /// host is present, so `Popup` calls without a host are inert instead of
    /// panicking.
    revision: Option<MutableState<u64>>,
}

/// Shared, cheaply-cloneable handle to the popup registry provided by the
/// nearest [`PopupHost`].
#[derive(Clone)]
pub struct PopupRegistry {
    inner: Rc<PopupRegistryState>,
}

impl PopupRegistry {
    fn hosted() -> Self {
        Self {
            inner: Rc::new(PopupRegistryState {
                entries: RefCell::new(Vec::new()),
                next_id: Cell::new(0),
                revision: Some(mutableStateOf(0u64)),
            }),
        }
    }

    /// The default registry when no [`PopupHost`] is installed: it accepts
    /// registrations but is never rendered, so stray `Popup` calls are no-ops.
    fn detached() -> Self {
        Self {
            inner: Rc::new(PopupRegistryState {
                entries: RefCell::new(Vec::new()),
                next_id: Cell::new(0),
                revision: None,
            }),
        }
    }

    fn allocate_id(&self) -> u64 {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id.wrapping_add(1));
        id
    }

    /// Marks the registry dirty so the host recomposes. Safe to call from a
    /// [`SideEffect`]/dispose callback (uses a non-subscribing update).
    fn bump(&self) {
        if let Some(revision) = self.inner.revision.as_ref() {
            revision.update(|value| *value = value.wrapping_add(1));
        }
    }

    /// Inserts a new popup or updates an existing one. Only structural changes
    /// (a new id) or a moved position dirty the host; refreshing the (stable,
    /// remembered) content closure in place does not, so a resting frame does
    /// not spin recomposition.
    fn upsert(&self, id: u64, position: Point, content: Rc<dyn Fn()>) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(existing) = entries.iter_mut().find(|entry| entry.id == id) {
            let moved = existing.position != position;
            existing.position = position;
            existing.content = content;
            drop(entries);
            if moved {
                self.bump();
            }
        } else {
            entries.push(PopupEntry {
                id,
                position,
                content,
            });
            drop(entries);
            self.bump();
        }
    }

    fn remove(&self, id: u64) {
        let mut entries = self.inner.entries.borrow_mut();
        let before = entries.len();
        entries.retain(|entry| entry.id != id);
        let changed = entries.len() != before;
        drop(entries);
        if changed {
            self.bump();
        }
    }

    /// Subscribes the current recompose scope to add/remove/move events.
    fn subscribe(&self) {
        if let Some(revision) = self.inner.revision.as_ref() {
            let _ = revision.value();
        }
    }

    fn snapshot(&self) -> Vec<PopupEntry> {
        self.inner.entries.borrow().clone()
    }
}

/// The [`CompositionLocal`](cranpose_core::CompositionLocal) carrying the active
/// [`PopupRegistry`] down the tree. One shared static local per thread.
fn local_popup_registry() -> StaticCompositionLocal<PopupRegistry> {
    thread_local! {
        static LOCAL: RefCell<Option<StaticCompositionLocal<PopupRegistry>>> =
            const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| staticCompositionLocalOf(PopupRegistry::detached))
            .clone()
    })
}

/// Installs the top-level overlay layer and composes `content` beneath it.
///
/// Wrap an application's root content in a single `PopupHost` so that any
/// [`Popup`] composed anywhere inside `content` renders in the overlay, above
/// everything and clipped only by the viewport. The host itself is an
/// unclipped, viewport-filling [`Box`]; the app content is its first child and
/// each registered popup is a trailing child (drawn last, hit-tested first).
#[composable]
pub fn PopupHost<F>(content: F)
where
    F: FnMut() + 'static,
{
    let registry = remember(PopupRegistry::hosted).with(PopupRegistry::clone);
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            let registry = registry.clone();
            CompositionLocalProvider([local_popup_registry().provides(registry.clone())], || {
                // App content: `Popup` calls inside here register into `registry`.
                content();
                // React to add/remove/move, then render the overlay entries.
                registry.subscribe();
                for entry in registry.snapshot() {
                    let content = entry.content;
                    Box(
                        Modifier::empty().absolute_offset(entry.position.x, entry.position.y),
                        BoxSpec::default(),
                        move || content(),
                    );
                }
            });
        },
    );
}

/// Composes `content` in the top-level overlay layer, positioned at
/// `anchor` shifted by `offset` (logical px, window coordinates).
///
/// The content is not clipped by the ancestor bounds of the `Popup` call site
/// and draws above all normal content. Requires an enclosing [`PopupHost`]
/// (installed at the app root); without one the call is inert.
///
/// `anchor` is supplied by the caller (there is no automatic
/// `onGloballyPositioned` yet) — derive it from a pointer position, a tracked
/// layout rect, or a text-field caret/selection geometry.
#[composable]
pub fn Popup<F>(anchor: Rect, offset: Point, content: F)
where
    F: Fn() + 'static,
{
    let registry = local_popup_registry().current();
    let id = remember(|| registry.allocate_id()).with(|id| *id);
    let content: Rc<dyn Fn()> = remember(move || {
        let content: Rc<dyn Fn()> = Rc::new(content);
        content
    })
    .with(Rc::clone);
    let position = Point {
        x: anchor.x + offset.x,
        y: anchor.y + offset.y,
    };

    let sync_registry = registry.clone();
    let sync_content = content.clone();
    SideEffect(move || sync_registry.upsert(id, position, sync_content.clone()));

    let dispose_registry = registry;
    cranpose_core::DisposableEffect!((), move |scope| {
        let dispose_registry = dispose_registry.clone();
        scope.on_dispose(move || dispose_registry.remove(id))
    });
}
