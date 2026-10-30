// Values kept for reuse, at most `N`, most used first. A hit moves one step
// toward the front, so the values in use stay among the first compared
// without the whole list moving on every hit; a new value goes first and the
// last one goes.
pub(crate) struct RecentValues<T, const N: usize>(Vec<T>);

impl<T, const N: usize> RecentValues<T, N> {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    // The kept value that `matches` `key`, or the one `make` makes of it.
    pub(crate) fn find_or_keep<K>(
        &mut self,
        key: K,
        matches: impl Fn(&T, &K) -> bool,
        make: impl FnOnce(K) -> T,
    ) -> &T {
        match self.0.iter().position(|kept| matches(kept, &key)) {
            Some(0) => &self.0[0],
            Some(index) => {
                self.0.swap(index, index - 1);
                &self.0[index - 1]
            }
            None => {
                self.0.truncate(N - 1);
                self.0.insert(0, make(key));
                &self.0[0]
            }
        }
    }
}
