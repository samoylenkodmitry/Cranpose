use std::{
    collections::HashMap,
    hash::{BuildHasher, Hash},
};

const MIN_TRIMMED_SLACK: usize = 16;

fn slack_target(len: usize, capacity: usize) -> Option<usize> {
    let slack = capacity.saturating_sub(len);
    (slack > (len / 4).max(MIN_TRIMMED_SLACK)).then_some(len + len / 8)
}

pub(in crate::slot) trait GrowthSlack {
    fn trim_growth_slack(&mut self);
}

impl<T> GrowthSlack for Vec<T> {
    fn trim_growth_slack(&mut self) {
        if let Some(target) = slack_target(self.len(), self.capacity()) {
            self.shrink_to(target);
        }
    }
}

impl<K: Eq + Hash, V, S: BuildHasher> GrowthSlack for HashMap<K, V, S> {
    fn trim_growth_slack(&mut self) {
        if let Some(target) = slack_target(self.len(), self.capacity()) {
            self.shrink_to(target);
        }
    }
}
