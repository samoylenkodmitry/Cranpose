//! The keys of the open branch guards, folded into the identity of the
//! groups, values and nodes composed inside them.

use std::cell::{Cell, RefCell};

use super::BRANCH_PATH_ROOT;
use crate::Key;

/// Folds `key` into `fold` (one FNV-1a step).
#[inline]
pub(crate) fn mix_fold(fold: Key, key: Key) -> Key {
    (fold ^ key).wrapping_mul(0x0000_0100_0000_01b3)
}

struct FoldEntry {
    key: Key,
    live: bool,
}

/// What [`BranchFolds::push`] returns for closing the branch: the place of
/// its entry and the cached fold before it opened, which closing restores.
#[derive(Clone, Copy)]
pub(crate) struct FoldToken {
    index: usize,
    prev_fold: Option<Key>,
}

/// The branch fold stack of the composers that share one runtime state.
///
/// A guard pushes its key while its branch composes. A group folds the live
/// entries opened since it began: each group frame and each slot host pass
/// records the stack length it began at as its watermark. Composers nest on
/// one thread, so their guards open and close in stack order; a guard closed
/// out of order leaves a dead entry until the entries above it close.
#[derive(Default)]
pub(crate) struct BranchFolds {
    entries: RefCell<Vec<FoldEntry>>,
    /// The fold of the live entries above the current watermark, or `None`
    /// until it is read after the watermark or a dead entry changed it.
    fold: Cell<Option<Key>>,
    /// Entries closed out of order that still sit under a live entry.
    dead: Cell<usize>,
}

impl BranchFolds {
    /// Opens a branch for `key` and returns the token that closes it.
    #[inline]
    pub(crate) fn push(&self, key: Key) -> FoldToken {
        let prev_fold = self.fold.get();
        if let Some(fold) = prev_fold {
            self.fold.set(Some(mix_fold(fold, key)));
        }
        let mut entries = self.entries.borrow_mut();
        entries.push(FoldEntry { key, live: true });
        FoldToken {
            index: entries.len() - 1,
            prev_fold,
        }
    }

    /// Closes the branch `push` returned `token` for.
    #[inline]
    pub(crate) fn close(&self, token: FoldToken) {
        let mut entries = self.entries.borrow_mut();
        if token.index + 1 == entries.len() && self.dead.get() == 0 {
            entries.pop();
            self.fold.set(token.prev_fold);
            return;
        }
        self.close_out_of_order(&mut entries, token.index);
    }

    /// Closes the branch `token` is for and opens one for `key` in its
    /// place, as `close` and then `push` do. The branch on top of the stack
    /// only changes its key.
    #[inline]
    pub(crate) fn advance(&self, token: &mut FoldToken, key: Key) {
        let mut entries = self.entries.borrow_mut();
        if token.index + 1 == entries.len()
            && self.dead.get() == 0
            && let Some(entry) = entries.last_mut()
        {
            entry.key = key;
            self.fold
                .set(token.prev_fold.map(|fold| mix_fold(fold, key)));
            return;
        }
        drop(entries);
        self.advance_out_of_order(token, key);
    }

    /// [`Self::advance`] for a branch under another, out of the inlined path.
    #[cold]
    fn advance_out_of_order(&self, token: &mut FoldToken, key: Key) {
        self.close_out_of_order(&mut self.entries.borrow_mut(), token.index);
        *token = self.push(key);
    }

    fn close_out_of_order(&self, entries: &mut Vec<FoldEntry>, token: usize) {
        self.fold.set(None);
        if token + 1 == entries.len() {
            entries.pop();
            while entries.last().is_some_and(|entry| !entry.live) {
                entries.pop();
                self.dead.set(self.dead.get() - 1);
            }
            return;
        }
        let Some(entry) = entries.get_mut(token) else {
            log::error!("branch fold {token} closed past depth {}", entries.len());
            return;
        };
        if entry.live {
            entry.live = false;
            self.dead.set(self.dead.get() + 1);
        }
    }

    /// The number of entries, the watermark of a group or pass beginning now.
    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    /// The fold of the live entries above the current watermark, while it
    /// is known without a walk.
    #[inline]
    pub(crate) fn cached_fold(&self) -> Option<Key> {
        self.fold.get()
    }

    /// The fold of the live entries from `watermark`, the current watermark.
    pub(crate) fn fold(&self, watermark: usize) -> Key {
        if let Some(fold) = self.fold.get() {
            return fold;
        }
        let entries = self.entries.borrow();
        let fold = entries[watermark.min(entries.len())..]
            .iter()
            .filter(|entry| entry.live)
            .fold(BRANCH_PATH_ROOT, |fold, entry| mix_fold(fold, entry.key));
        self.fold.set(Some(fold));
        fold
    }

    /// A group begins: returns its watermark, above which no entry sits yet.
    #[inline]
    pub(crate) fn begin_group(&self) -> usize {
        self.fold.set(Some(BRANCH_PATH_ROOT));
        self.len()
    }

    /// The current watermark moved down to an enclosing group or pass.
    #[inline]
    pub(crate) fn watermark_moved(&self) {
        self.fold.set(None);
    }

    /// The current watermark moved down to `watermark`, an enclosing
    /// group's or pass's. With no entry above it, as when no branch guard
    /// is open, its fold is the root's, known without a walk.
    #[inline]
    pub(crate) fn watermark_moved_to(&self, watermark: usize) {
        let none_above = self.entries.borrow().len() <= watermark;
        self.fold.set(none_above.then_some(BRANCH_PATH_ROOT));
    }

    /// A slot host pass that began at `base` ends: drops the entries its
    /// guards left open, which only a guard that never dropped leaves.
    pub(crate) fn end_pass(&self, base: usize) {
        self.fold.set(None);
        let mut entries = self.entries.borrow_mut();
        if entries.len() <= base {
            return;
        }
        log::error!(
            "a slot host pass ended with {} branch folds whose guards never closed",
            entries.len() - base
        );
        let dead = entries[base..].iter().filter(|entry| !entry.live).count();
        entries.truncate(base);
        self.dead.set(self.dead.get() - dead);
    }
}
