//! Which nodes of the web accessibility mirror move when the order of a
//! parent's children changes. The nodes in the longest run that keeps its
//! old order stay; every other node moves once.

/// The plan that brings a parent's children from the order the mirror wrote
/// last to a new one. The buffers stay between plans, so a plan allocates
/// nothing once they have grown.
#[derive(Default)]
pub(crate) struct ChildMoves {
    /// For each length of an increasing run found so far, the child that
    /// ends such a run with the smallest old position, and that position.
    tails: Vec<(usize, usize)>,
    /// For each child, the child before it in the run it ends.
    previous: Vec<Option<usize>>,
    stays: Vec<bool>,
}

impl ChildMoves {
    /// Takes the children of the new order by their old positions, nothing
    /// for a child that was not among the parent's children, and keeps in
    /// place the longest run whose old positions increase. No smaller set of
    /// moves gives the new order.
    pub(crate) fn plan(&mut self, positions: impl IntoIterator<Item = Option<usize>>) {
        self.tails.clear();
        self.previous.clear();
        self.stays.clear();
        for (child, position) in positions.into_iter().enumerate() {
            self.stays.push(false);
            let Some(position) = position else {
                self.previous.push(None);
                continue;
            };
            let length = self.tails.partition_point(|&(_, end)| end < position);
            self.previous.push(
                length
                    .checked_sub(1)
                    .and_then(|shorter| self.tails.get(shorter))
                    .map(|&(before, _)| before),
            );
            match self.tails.get_mut(length) {
                Some(tail) => *tail = (child, position),
                None => self.tails.push((child, position)),
            }
        }
        let mut child = self.tails.last().map(|&(last, _)| last);
        while let Some(index) = child {
            if let Some(stays) = self.stays.get_mut(index) {
                *stays = true;
            }
            child = self.previous.get(index).copied().flatten();
        }
    }

    /// The moves that bring the old order to the new one, from the last
    /// child to the first: a child and the child it goes right before, or
    /// nothing for the end of the parent.
    pub(crate) fn moves(&self) -> impl Iterator<Item = (usize, Option<usize>)> + '_ {
        let mut next = None;
        self.stays
            .iter()
            .enumerate()
            .rev()
            .filter_map(move |(child, stays)| {
                let before = next.replace(child);
                (!stays).then_some((child, before))
            })
    }
}

#[cfg(test)]
#[path = "tests/web_accessibility_order_tests.rs"]
mod tests;
