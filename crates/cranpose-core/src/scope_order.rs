use crate::RecomposeScope;

// Puts a list of scopes in the order of a key that each one gets, keeping the
// list's order among the scopes of one key: a counting placement, linear in
// the scopes and the keys. The storage stays from one use to the next.
#[derive(Default)]
pub(crate) struct ScopeOrder {
    // The key of each scope, in the list's order.
    keys: Vec<usize>,
    // For each key, where its next scope goes, then where its scopes end.
    ends: Vec<usize>,
    // The scopes at their places in key order.
    placed: Vec<Option<RecomposeScope>>,
}

impl ScopeOrder {
    // Orders `scopes` by the key that `key_of` gives each, called once a
    // scope in the list's order. Small keys keep the storage small.
    pub(crate) fn order_by(
        &mut self,
        scopes: &mut Vec<RecomposeScope>,
        mut key_of: impl FnMut(&RecomposeScope) -> usize,
    ) {
        self.keys.clear();
        self.ends.clear();
        for scope in scopes.iter() {
            let key = key_of(scope);
            if key >= self.ends.len() {
                self.ends.resize(key + 1, 0);
            }
            self.ends[key] += 1;
            self.keys.push(key);
        }
        let mut start = 0;
        for place in &mut self.ends {
            let count = *place;
            *place = start;
            start += count;
        }
        self.placed.resize_with(scopes.len(), || None);
        for (scope, key) in scopes.drain(..).zip(self.keys.drain(..)) {
            let place = &mut self.ends[key];
            self.placed[*place] = Some(scope);
            *place += 1;
        }
        scopes.extend(self.placed.drain(..).flatten());
    }

    // Where the scopes of each key end in the list that the last `order_by`
    // ordered.
    pub(crate) fn ends(&self) -> &[usize] {
        &self.ends
    }
}
