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
    /// The cached fold before this entry opened, restored when it closes.
    prev_fold: Option<Key>,
    live: bool,
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
    pub(crate) fn push(&self, key: Key) -> usize {
        let prev_fold = self.fold.get();
        if let Some(fold) = prev_fold {
            self.fold.set(Some(mix_fold(fold, key)));
        }
        let mut entries = self.entries.borrow_mut();
        entries.push(FoldEntry {
            key,
            prev_fold,
            live: true,
        });
        entries.len() - 1
    }

    /// Closes the branch `push` returned `token` for.
    #[inline]
    pub(crate) fn close(&self, token: usize) {
        let mut entries = self.entries.borrow_mut();
        if token + 1 == entries.len()
            && self.dead.get() == 0
            && let Some(entry) = entries.pop()
        {
            self.fold.set(entry.prev_fold);
            return;
        }
        self.close_out_of_order(&mut entries, token);
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

    /// The fold of the live entries from `watermark`, the current watermark.
    #[inline]
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
