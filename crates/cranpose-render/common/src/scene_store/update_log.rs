// The items a store logged in its latest updates, read by update number: a
// reader keeps the update it read last and reads the items of the updates
// after it. A reader further behind than the log keeps finds them lost.

use cranpose_core::{SceneUpdate, collections::growth_slack::GrowthSlack};

#[derive(Clone, Debug)]
pub(super) struct UpdateLog<T> {
    items: Vec<T>,
    // Each kept update, oldest first, with where its items start: `None`
    // for one that logged more items than an update keeps.
    updates: Vec<(SceneUpdate, Option<usize>)>,
}

impl<T> Default for UpdateLog<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            updates: Vec::new(),
        }
    }
}

// How many updates a log keeps: once it holds `most`, it forgets all but
// the latest `kept`, so an update seldom pays for the forgetting.
#[derive(Clone, Copy)]
pub(super) struct UpdatesKept {
    pub(super) most: usize,
    pub(super) kept: usize,
}

impl<T> UpdateLog<T> {
    // Starts the items of `update`, forgetting the oldest updates as
    // `updates` says and those whose items take the log past `most_items`.
    #[inline]
    pub(super) fn begin(&mut self, update: SceneUpdate, updates: UpdatesKept, most_items: usize) {
        if self.updates.len() >= updates.most || self.items.len() - self.start(0) > most_items {
            self.forget_oldest(updates.kept, most_items);
        }
        // The log holds `most` updates at most: it takes their room once.
        if self.updates.capacity() < updates.most {
            self.updates
                .reserve_exact(updates.most - self.updates.len());
        }
        self.updates.push((update, Some(self.items.len())));
    }

    // Forgets all but the latest `kept` updates, and more of them while
    // their items take the log past `most_items`. Out of line: a log forgets
    // once in many updates.
    #[inline(never)]
    fn forget_oldest(&mut self, kept: usize, most_items: usize) {
        let mut gone = self.updates.len().saturating_sub(kept);
        while gone < self.updates.len() && self.items.len() - self.start(gone) > most_items {
            gone += 1;
        }
        if gone > 0 {
            self.forget(gone);
        }
    }

    // Where the items of the kept update at `index` and those after it
    // start.
    fn start(&self, index: usize) -> usize {
        self.updates[index..]
            .iter()
            .find_map(|&(_, start)| start)
            .unwrap_or(self.items.len())
    }

    // Forgets the oldest `gone` updates and gives back the room the log
    // grew past what it keeps.
    fn forget(&mut self, gone: usize) {
        let cut = if gone < self.updates.len() {
            self.start(gone)
        } else {
            self.items.len()
        };
        self.items.drain(..cut);
        self.items.trim_growth_slack();
        self.updates.drain(..gone);
        for (_, start) in &mut self.updates {
            if let Some(start) = start {
                *start -= cut;
            }
        }
    }

    // Logs `item` in the update in progress, unless the update logged
    // `most_in_update` items already: its items are then lost.
    #[inline]
    pub(super) fn push(&mut self, item: T, most_in_update: usize) {
        let Some((_, Some(start))) = self.updates.last().copied() else {
            return;
        };
        if self.items.len() - start < most_in_update {
            self.items.push(item);
        } else {
            self.lose_last(start);
        }
    }

    #[cold]
    fn lose_last(&mut self, start: usize) {
        self.items.truncate(start);
        if let Some(last) = self.updates.last_mut() {
            last.1 = None;
        }
    }

    pub(super) fn clear(&mut self) {
        self.items.clear();
        self.updates.clear();
    }

    // Gives back at rest the items of the kept updates: a reader that read
    // the latest update reads nothing more, and one further behind finds
    // them lost.
    pub(super) fn trim(&mut self) {
        self.items = Vec::new();
        for (_, start) in &mut self.updates {
            *start = None;
        }
        self.updates.trim_growth_slack();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.items.capacity() * size_of::<T>()
            + self.updates.capacity() * size_of::<(SceneUpdate, Option<usize>)>()
    }

    // The items of the updates after `read`, when the store's update is
    // `now`; `None` when the log lost some of them.
    pub(super) fn since(&self, read: SceneUpdate, now: SceneUpdate) -> Option<&[T]> {
        if read == now {
            return Some(&[]);
        }
        // A reader mostly lags by one update, the last kept.
        let after = read.next();
        let first = self
            .updates
            .iter()
            .rposition(|&(update, _)| update == after)?;
        let mut kept = self.updates[first..].iter().map(|&(_, start)| start);
        let start = kept.next().flatten()?;
        kept.all(|start| start.is_some())
            .then(|| &self.items[start..])
    }
}
