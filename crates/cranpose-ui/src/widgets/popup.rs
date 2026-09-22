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
//! unclipped, viewport-filling `Box` and renders every registered popup as a
//! trailing child, positioned absolutely at its anchor.
//!
//! [`Popup`] itself emits **no node at its call site**. Instead it registers
//! its `(position, content)` into a [`PopupRegistry`] carried down the tree by
//! a `CompositionLocal`; the enclosing [`PopupHost`] reads that registry and
//! composes the content at the root. Registration/teardown is reactive:
//! adding or removing a popup recomposes the list. Moving or refreshing one
//! popup recomposes its own layer, keeping nested popups from invalidating
//! their parents as their content changes.

#![allow(non_snake_case)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    CompositionLocalProvider, OwnedMutableState, SideEffect, StaticCompositionLocal,
    ownedMutableStateOf, remember, staticCompositionLocalOf,
};
use cranpose_foundation::PointerEventKind;
use cranpose_ui_graphics::{Point, Rect};

use super::box_widget::{Box, BoxSpec};
use crate::{PointerInputScope, composable, modifier::Modifier};

/// One registered popup: a stable id, its absolute top-left position (logical
/// px, in [`PopupHost`] space, i.e. window coordinates) and its content.
#[derive(Clone)]
struct PopupEntry {
    id: u64,
    data: OwnedMutableState<PopupContent>,
}

impl PartialEq for PopupEntry {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.data.handle() == other.data.handle()
    }
}

#[derive(Clone)]
struct PopupContent {
    position: Point,
    content: Rc<dyn Fn()>,
    on_dismiss: Option<Rc<dyn Fn()>>,
}

impl PartialEq for PopupContent {
    fn eq(&self, other: &Self) -> bool {
        self.position == other.position
            && Rc::ptr_eq(&self.content, &other.content)
            && match (&self.on_dismiss, &other.on_dismiss) {
                (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

struct PopupRegistryState {
    entries: RefCell<Vec<PopupEntry>>,
    next_id: Cell<u64>,
    revision: Option<OwnedMutableState<u64>>,
}

/// Shared, cheaply-cloneable handle to the popup registry provided by the
/// nearest [`PopupHost`].
#[derive(Clone)]
pub struct PopupRegistry {
    inner: Rc<PopupRegistryState>,
}

impl PartialEq for PopupRegistry {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

#[derive(Default)]
pub(crate) struct HostedPopupRegistries {
    registries: RefCell<Vec<std::rc::Weak<PopupRegistryState>>>,
}

/// Closes the dismissable popup on top of the most recent host, the way an
/// outside tap would, for a screen reader's escape gesture or an Escape key
/// with no dialog open. Answers whether a popup was there to close.
pub fn dismiss_top_popup() -> bool {
    let on_dismiss = crate::render_state::with_hosted_popup_registries(|hosted| {
        let mut hosted = hosted.registries.borrow_mut();
        hosted.retain(|registry| registry.strong_count() > 0);
        hosted.iter().rev().find_map(|registry| {
            let registry = registry.upgrade()?;
            let entries = registry.entries.borrow();
            entries
                .iter()
                .rev()
                .find_map(|entry| entry.data.get_non_reactive().on_dismiss)
        })
    });
    match on_dismiss {
        Some(on_dismiss) => {
            on_dismiss();
            true
        }
        None => false,
    }
}

/// Whether a dismissable popup is open on any host. Reading this in a
/// composable subscribes to every host's revision, so the reader recomposes
/// when a popup opens or closes.
pub fn dismissable_popup_open() -> bool {
    crate::render_state::with_hosted_popup_registries(|hosted| {
        hosted.registries.borrow().iter().any(|registry| {
            let Some(registry) = registry.upgrade() else {
                return false;
            };
            if let Some(revision) = registry.revision.as_ref() {
                let _ = revision.value();
            }
            registry
                .entries
                .borrow()
                .iter()
                .any(|entry| entry.data.get_non_reactive().on_dismiss.is_some())
        })
    })
}

impl PopupRegistry {
    fn hosted() -> Self {
        let inner = Rc::new(PopupRegistryState {
            entries: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
            revision: Some(ownedMutableStateOf(0u64)),
        });
        crate::render_state::with_hosted_popup_registries(|hosted| {
            hosted.registries.borrow_mut().push(Rc::downgrade(&inner));
        });
        Self { inner }
    }

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

    fn bump(&self) {
        if let Some(revision) = self.inner.revision.as_ref() {
            revision.update(|value| *value = value.wrapping_add(1));
        }
    }

    fn upsert(
        &self,
        id: u64,
        position: Point,
        content: Rc<dyn Fn()>,
        on_dismiss: Option<Rc<dyn Fn()>>,
    ) {
        let next = PopupContent {
            position,
            content,
            on_dismiss,
        };
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(existing) = entries.iter_mut().find(|entry| entry.id == id) {
            let state = existing.data.clone();
            let dismiss_changed =
                state.get_non_reactive().on_dismiss.is_some() != next.on_dismiss.is_some();
            drop(entries);
            state.set(next);
            if dismiss_changed {
                self.bump();
            }
        } else {
            entries.push(PopupEntry {
                id,
                data: ownedMutableStateOf(next),
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

/// The [`PopupHost`]'s live measured viewport size (logical px), published on
/// every measure pass through observable state. Overlay content recomposes
/// when the host is first measured or resized, keeping its bounds in the window;
/// `Size::ZERO` means "not measured yet" (or no host) — treat as unclamped.
pub fn local_popup_viewport()
-> StaticCompositionLocal<OwnedMutableState<cranpose_ui_graphics::Size>> {
    type ViewportState = OwnedMutableState<cranpose_ui_graphics::Size>;
    thread_local! {
        static LOCAL: RefCell<Option<StaticCompositionLocal<ViewportState>>> =
            const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| {
                staticCompositionLocalOf(|| ownedMutableStateOf(cranpose_ui_graphics::Size::ZERO))
            })
            .clone()
    })
}

/// Installs the top-level overlay layer and composes `content` beneath it.
///
/// Wrap an application's root content in a single `PopupHost` so that any
/// [`Popup`] composed anywhere inside `content` renders in the overlay, above
/// everything and clipped only by the viewport. The host itself is an
/// unclipped, viewport-filling `Box`; the app content is its first child and
/// each registered popup is a trailing child (drawn last, hit-tested first).
#[composable]
pub fn PopupHost<F>(content: F)
where
    F: FnMut() + 'static,
{
    let registry = remember(PopupRegistry::hosted).with(PopupRegistry::clone);
    let viewport =
        remember(|| ownedMutableStateOf(cranpose_ui_graphics::Size::ZERO)).with(Clone::clone);
    let report_sink = viewport.handle();
    Box(
        Modifier::empty()
            .fill_max_size()
            .report_size_state(report_sink),
        BoxSpec::default(),
        move || {
            let registry = registry.clone();
            let viewport = viewport.clone();
            CompositionLocalProvider(
                [
                    local_popup_registry().provides(registry.clone()),
                    local_popup_viewport().provides(viewport),
                ],
                || {
                    content();
                    PopupOverlay(registry.clone());
                },
            );
        },
    );
}

#[composable]
fn PopupOverlay(registry: PopupRegistry) {
    registry.subscribe();
    for entry in registry.snapshot() {
        cranpose_core::key(entry.id, || PopupLayer(entry));
    }
}

#[composable]
fn PopupLayer(entry: PopupEntry) {
    let data = entry.data.get();
    let dismiss = data.on_dismiss.clone();
    if let Some(on_dismiss) = data.on_dismiss {
        Box(
            Modifier::empty()
                .fill_max_size()
                .then(popup_scrim_pointer_input(entry.id, on_dismiss)),
            BoxSpec::default(),
            || {},
        );
    }
    let content = data.content;
    Box(
        Modifier::empty()
            .absolute_offset(data.position.x, data.position.y)
            .semantics(move |config| {
                config.is_modal = dismiss.is_some();
                config.dismiss = dismiss.clone().map(|dismiss| {
                    cranpose_foundation::SemanticsDismiss::new(move || {
                        dismiss();
                        true
                    })
                });
            }),
        BoxSpec::default(),
        move || content(),
    );
}

/// Modal outside-tap handling for dismissable popups. Consuming Down prevents
/// lower z-order siblings from joining the shell's captured hit path; consuming
/// every follow-up keeps the entire gesture inside the overlay even when the
/// dismiss callback removes the popup on release.
fn popup_scrim_pointer_input(id: u64, on_dismiss: Rc<dyn Fn()>) -> Modifier {
    Modifier::empty().pointer_input(id, move |scope: PointerInputScope| {
        let on_dismiss = Rc::clone(&on_dismiss);
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    let mut pressed = false;
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        match event.kind {
                            PointerEventKind::Down => {
                                pressed = true;
                                event.consume();
                            }
                            PointerEventKind::Move => event.consume(),
                            PointerEventKind::Up => {
                                let should_dismiss = pressed && !event.is_consumed();
                                pressed = false;
                                event.consume();
                                if should_dismiss {
                                    on_dismiss();
                                }
                            }
                            PointerEventKind::Cancel => {
                                pressed = false;
                                event.consume();
                            }
                            _ => {}
                        }
                    }
                })
                .await;
        }
    })
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
    popup_impl(anchor, offset, None, Rc::new(content));
}

/// Renders `anchor_content` normally and, while `expanded`, places
/// `popup_content` relative to the anchor's measured window rectangle.
#[composable]
pub fn PopupAnchored<A, P>(
    modifier: Modifier,
    expanded: bool,
    offset: Point,
    anchor_content: A,
    popup_content: P,
) -> cranpose_core::NodeId
where
    A: Fn() + 'static,
    P: Fn() + 'static,
{
    let anchor = cranpose_core::rememberMutableStateOf(|| {
        Rect::from_origin_size(
            Point { x: 0.0, y: 0.0 },
            cranpose_ui_graphics::Size {
                width: 0.0,
                height: 0.0,
            },
        )
    });
    let measured = modifier.report_window_rect_state(anchor);
    let anchor_content = Rc::new(anchor_content);
    let popup_content = Rc::new(popup_content);
    Box(measured, BoxSpec::default(), move || {
        anchor_content();
        if expanded {
            let popup_content = Rc::clone(&popup_content);
            Popup(anchor.get(), offset, move || popup_content());
        }
    })
}

/// A [`Popup`] with an outside-tap dismissal: the host renders a
/// viewport-filling scrim beneath the content that calls `on_dismiss` — the
/// analogue of Compose's `Popup(onDismissRequest = …)`. Menus and pickers use
/// this; anchored chrome like selection handles uses plain [`Popup`].
#[composable]
pub fn PopupDismissable<F>(anchor: Rect, offset: Point, on_dismiss: impl Fn() + 'static, content: F)
where
    F: Fn() + 'static,
{
    PopupDismissableWhen(true, anchor, offset, on_dismiss, content);
}

/// A dismissable popup whose modal scrim can be disabled without unmounting
/// its visual content. Controls with an exit animation use this to stop
/// intercepting the rest of the UI as soon as dismissal begins while their
/// popup surface finishes animating out.
#[composable]
pub fn PopupDismissableWhen<F>(
    dismissable: bool,
    anchor: Rect,
    offset: Point,
    on_dismiss: impl Fn() + 'static,
    content: F,
) where
    F: Fn() + 'static,
{
    let on_dismiss = popup_dismiss_callback(dismissable, Rc::new(on_dismiss));
    popup_impl(anchor, offset, on_dismiss, Rc::new(content));
}

fn popup_dismiss_callback(dismissable: bool, on_dismiss: Rc<dyn Fn()>) -> Option<Rc<dyn Fn()>> {
    dismissable.then_some(on_dismiss)
}

fn popup_impl(
    anchor: Rect,
    offset: Point,
    on_dismiss: Option<Rc<dyn Fn()>>,
    content: Rc<dyn Fn()>,
) {
    let registry = local_popup_registry().current();
    let id = remember(|| registry.allocate_id()).with(|id| *id);
    let position = Point {
        x: anchor.x + offset.x,
        y: anchor.y + offset.y,
    };

    let sync_registry = registry.clone();
    let sync_content = content.clone();
    SideEffect(move || {
        sync_registry.upsert(id, position, sync_content.clone(), on_dismiss.clone())
    });

    let dispose_registry = registry;
    cranpose_core::DisposableEffect((), move |scope| {
        let dispose_registry = dispose_registry.clone();
        scope.on_dispose(move || dispose_registry.remove(id))
    });
}

#[cfg(test)]
#[path = "tests/popup_tests.rs"]
mod tests;
