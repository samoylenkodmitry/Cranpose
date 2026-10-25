use std::{hash::Hash, rc::Rc};

use crate::{
    Composer, Key, composer_context, current_runtime_handle, hash_key, remember,
    with_current_composer,
};
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
    with_current_composer(|composer| composer.with_movable_group(id, |_| content()));
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
/// The identity comes from the `remember`, so two of these at one call site
/// -- one per row of a list -- are two pieces of content, and the same one
/// survives recomposition. Everything [`movable`] says about ordering,
/// showing content at most once, and releasing it still holds; release this
/// one with [`MovableContent::forget`].
///
/// Content that has to be recognised from somewhere this value cannot reach
/// -- another composable that shows the same pane in another arrangement --
/// wants [`movableContentOf`] instead, which takes the identity as a key.
///
/// Mirrors Jetpack Compose's `movableContentOf`.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberMovableContentOf(content: impl Fn() + 'static) -> MovableContent {
    let runtime = with_current_composer(Composer::runtime_handle);
    let id = remember(|| runtime.next_movable_content_id()).with(|id| *id);
    MovableContent {
        id,
        content: Rc::new(content),
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
    }
}

/// Movable content held as a value. See [`rememberMovableContentOf`].
#[derive(Clone)]
pub struct MovableContent {
    id: Key,
    content: Rc<dyn Fn()>,
}

impl MovableContent {
    /// Shows the content here. Calling this from a different parent than
    /// last pass moves the content, with its remembered values, its running
    /// effects and its nodes.
    pub fn show(&self) {
        let content = Rc::clone(&self.content);
        let id = self.id;
        with_current_composer(|composer| composer.with_movable_group(id, |_| content()));
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
