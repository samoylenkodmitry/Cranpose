// Values kept for reuse, at most `N`, most used first. A hit moves one step
// toward the front, so the values in use stay among the first compared
// without the whole list moving on every hit; a new value goes first and the
// last one goes.
pub(crate) struct RecentValues<T, const N: usize>(Vec<T>);

impl<T, const N: usize> RecentValues<T, N> {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    // The kept value `matches` accepts.
    pub(crate) fn find(&mut self, matches: impl Fn(&T) -> bool) -> Option<&T> {
        match self.0.iter().position(matches)? {
            0 => self.0.first(),
            index => {
                self.0.swap(index, index - 1);
                self.0.get(index - 1)
            }
        }
    }

    // Keeps `value` first, in place of the last kept one when the list is
    // full.
    pub(crate) fn keep(&mut self, value: T) -> &T {
        self.0.truncate(N - 1);
        self.0.insert(0, value);
        &self.0[0]
    }
}
