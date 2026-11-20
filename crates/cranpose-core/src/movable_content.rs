use std::{
    cell::Cell,
    hash::Hash,
    rc::{Rc, Weak},
};

use crate::{
    Composer, Key,
    composer::{ComposerRuntimeState, MovableLookup},
    composer_context, current_runtime_handle, hash_key,
    owned::WeakOwned,
    with_current_composer,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MovableOwnerState {
    NeverShown,
    ShownInOwnerHost,
    ShownOutsideOwnerHost,
    DisposedWithSubtree,
}

struct RememberedMovable {
    id: Key,
    shared_state: Weak<ComposerRuntimeState>,
    owner_host_key: usize,
    state: Cell<MovableOwnerState>,
}

impl Drop for RememberedMovable {
    fn drop(&mut self) {
        let owner_host = match self.state.get() {
            MovableOwnerState::NeverShown | MovableOwnerState::DisposedWithSubtree => return,
            MovableOwnerState::ShownInOwnerHost => Some(self.owner_host_key),
            MovableOwnerState::ShownOutsideOwnerHost => None,
        };
        if let Some(state) = self.shared_state.upgrade() {
            state.on_movable_owner_dropped(self.id, owner_host);
        }
    }
}

// Returns the movable id when `payload` is the sole owner of a remembered
// movable with no live `MovableContent` handles anywhere outside the slot.
pub(crate) fn sole_owned_movable_id(payload: &dyn std::any::Any) -> Option<Key> {
    let owned = payload.downcast_ref::<crate::Owned<RememberedMovable>>()?;
    (owned.strong_count() == 1 && owned.weak_count() == 0).then(|| owned.with(|owner| owner.id))
}

// Marks a remembered movable whose content is already being disposed inline
// with its owning subtree, so dropping the owner performs no redundant lookup.
pub(crate) fn disarm_remembered_movable(payload: &dyn std::any::Any) {
    if let Some(owned) = payload.downcast_ref::<crate::Owned<RememberedMovable>>() {
        owned.with(|owner| owner.state.set(MovableOwnerState::DisposedWithSubtree));
    }
}

/// Composes `content` under an identity that is `key` alone, the same at
/// every call site, so the subtree can be emitted from any parent and keep
/// its remembered values, its running effects and its nodes.
///
/// Move content by emitting it under a different parent, in the same pass
/// or a later one: when the old parent stops emitting it, the subtree is
/// retained under `key`; when a parent emits it, the retained subtree is
/// taken back. A site that asks for content still attached elsewhere
/// composes nothing and is recomposed once the content is retained, so the
/// order in which the two parents recompose does not matter.
///
/// Content is shown at most once. Two live sites with the same key leave
/// the later one empty. Retained content is kept
/// until a parent takes it back or [`forget_movable`] releases it, so an
/// item the app closes for good must be forgotten or its state stays in
/// memory. A subtree may cross into and out of a `SubcomposeLayout` slot,
/// whose content lives in a slot table of its own: the table that held it
/// gives up its anchors and the one taking it over issues new ones.
#[track_caller]
pub fn movable<K: Hash>(key: K, content: impl FnOnce()) {
    let id = hash_key(&key);
    with_current_composer(|composer| {
        composer.with_movable_group(id, MovableLookup::AnyHost, |_| content());
    });
}

/// Content written once that keeps its identity wherever it is shown.
///
/// [`movable`] names content by a key, which means writing the content out
/// at every parent that might show it and trusting the keys to match. When
/// the parents are alternatives -- a pane docked in a stack or pulled out
/// into a window of its own, a tab in a strip or torn into its own window --
/// that is the same body written twice, and two bodies that must be kept in
/// step by hand are two bodies that drift.
///
/// This hands back the content as a value instead. Write the body once,
/// remember it, and call [`MovableContent::show`] from whichever parent is
/// showing it this pass:
///
/// ```ignore
/// let pane = rememberMovableContentOf(move || PaneBody(state));
/// if docked {
///     Box(Modifier::empty().offset(0.0, y), BoxSpec::default(), || pane.show());
/// } else {
///     Box(Modifier::empty().window(config), BoxSpec::default(), || pane.show());
/// }
/// ```
///
/// The identity comes from the `remember`, so two calls at one site produce
/// two distinct pieces of content. The same identity survives recomposition.
/// Leaving the composition releases the content with the call site. Calling
/// [`MovableContent::forget`] releases retained content earlier while the call
/// site stays alive.
///
/// Content that must be recognised from another composable that this value
/// cannot reach uses [`movableContentOf`] instead, which takes a key as the
/// identity.
///
/// Mirrors Jetpack Compose's `movableContentOf`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberMovableContentOf(content: impl Fn() + 'static) -> MovableContent {
    let source = crate::caller_location_key();
    let (id, owner) = with_current_composer(|composer| {
        let owned = composer.remember_at(source, || RememberedMovable {
            id: composer.runtime_handle().next_movable_content_id(),
            shared_state: composer.shared_state_weak(),
            owner_host_key: composer.active_slots_host_storage_key(),
            state: Cell::new(MovableOwnerState::NeverShown),
        });
        let id = owned.with(|owner| owner.id);
        (id, owned.downgrade())
    });
    MovableContent {
        id,
        content: Rc::new(content),
        owner: Some(owner),
    }
}

/// Movable content whose identity is `key`, the same at every call site.
///
/// The keyed twin of [`rememberMovableContentOf`], and the same identity
/// [`movable`] uses: content shown by this value and content shown by
/// `movable` under the same key is one piece of content. Reach for this when
/// the parents that can show it are in different composables, so no single
/// value can be handed to all of them.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn movableContentOf<K: Hash>(key: K, content: impl Fn() + 'static) -> MovableContent {
    MovableContent {
        id: hash_key(&key),
        content: Rc::new(content),
        owner: None,
    }
}

/// Movable content held as a value. See [`rememberMovableContentOf`].
#[derive(Clone)]
pub struct MovableContent {
    id: Key,
    content: Rc<dyn Fn()>,
    owner: Option<WeakOwned<RememberedMovable>>,
}

impl MovableContent {
    /// Shows the content here. Calling this from a different parent than
    /// last pass moves the content, with its remembered values, its running
    /// effects and its nodes.
    pub fn show(&self) {
        let content = Rc::clone(&self.content);
        let id = self.id;
        with_current_composer(|composer| {
            let lookup = self
                .owner
                .as_ref()
                .and_then(|owner| {
                    owner.with(|owner| {
                        let outside =
                            composer.active_slots_host_storage_key() != owner.owner_host_key;
                        let (next, lookup) = match owner.state.get() {
                            MovableOwnerState::NeverShown => (
                                if outside {
                                    MovableOwnerState::ShownOutsideOwnerHost
                                } else {
                                    MovableOwnerState::ShownInOwnerHost
                                },
                                MovableLookup::FreshUnshown,
                            ),
                            MovableOwnerState::ShownInOwnerHost => {
                                if outside {
                                    (
                                        MovableOwnerState::ShownOutsideOwnerHost,
                                        MovableLookup::AnyHost,
                                    )
                                } else {
                                    (
                                        MovableOwnerState::ShownInOwnerHost,
                                        MovableLookup::OwnerHostOnly,
                                    )
                                }
                            }
                            MovableOwnerState::ShownOutsideOwnerHost => (
                                MovableOwnerState::ShownOutsideOwnerHost,
                                MovableLookup::AnyHost,
                            ),
                            MovableOwnerState::DisposedWithSubtree => (
                                if outside {
                                    MovableOwnerState::ShownOutsideOwnerHost
                                } else {
                                    MovableOwnerState::ShownInOwnerHost
                                },
                                MovableLookup::AnyHost,
                            ),
                        };
                        owner.state.set(next);
                        lookup
                    })
                })
                .unwrap_or(MovableLookup::AnyHost);
            composer.with_movable_group(id, lookup, |_| content());
        });
    }

    /// Releases the content's state once no parent is showing it, for
    /// content the application has closed for good. See [`forget_movable`].
    pub fn forget(&self) {
        forget_movable_id(self.id);
    }
}

impl std::fmt::Debug for MovableContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MovableContent")
            .field("id", &self.id)
            .finish()
    }
}

impl PartialEq for MovableContent {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Rc::ptr_eq(&self.content, &other.content)
    }
}

/// Releases the state of [`movable`] content with identity `key` that no
/// parent is showing. Call it when the item is closed for good, from an
/// event handler or inside composition. Content a parent is still showing
/// is unaffected. Needs a runtime: an active composition or one created on
/// this thread.
pub fn forget_movable<K: Hash>(key: K) {
    let id = hash_key(&key);
    forget_movable_id(id);
}

fn forget_movable_id(id: Key) {
    let runtime = composer_context::try_with_composer(Composer::runtime_handle)
        .or_else(current_runtime_handle);
    match runtime {
        Some(runtime) => runtime.forget_movable(id),
        None => log::error!("forget_movable called without an active runtime"),
    }
}
