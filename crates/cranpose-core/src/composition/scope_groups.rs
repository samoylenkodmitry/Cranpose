use std::rc::Rc;

use crate::{RecomposeScope, SlotsHost, collections::map::HashMap, scope_order::ScopeOrder};

// The scopes of a pass grouped by the slots host each recomposes in, in the
// order each host first appears. The scopes stay in one list, each group's
// together and in the order the pass gave them; the storage stays from pass
// to pass.
#[derive(Default)]
pub(super) struct ScopeGroups {
    // Each group's host and the end of its scopes in the list.
    hosts: Vec<(Rc<SlotsHost>, usize)>,
    // Each group by its host's storage key.
    by_key: HashMap<usize, usize>,
    // Places the scopes in group order.
    order: ScopeOrder,
}

impl ScopeGroups {
    // Groups `scopes` in place by the host `host_of` finds for each.
    // Consecutive scopes of one slots identity share a host, as do all
    // scopes of the usual pass, which stay as they are.
    pub(super) fn group(
        &mut self,
        scopes: &mut Vec<RecomposeScope>,
        mut host_of: impl FnMut(&RecomposeScope) -> Rc<SlotsHost>,
    ) {
        self.hosts.clear();
        let Some(first) = scopes.first() else {
            return;
        };
        let first_identity = first.slots_host_identity();
        if scopes
            .iter()
            .all(|scope| scope.slots_host_identity() == first_identity)
        {
            self.hosts.push((host_of(first), scopes.len()));
            return;
        }
        let (hosts, by_key) = (&mut self.hosts, &mut self.by_key);
        let mut previous = None;
        self.order.order_by(scopes, |scope| {
            let identity = scope.slots_host_identity();
            match previous {
                Some((previous_identity, group)) if previous_identity == identity => group,
                _ => {
                    let host = host_of(scope);
                    let next = hosts.len();
                    let group = *by_key.entry(host.storage_key()).or_insert(next);
                    if group == next {
                        hosts.push((host, 0));
                    }
                    previous = Some((identity, group));
                    group
                }
            }
        });
        self.by_key.clear();
        for ((_, end), &placed_end) in self.hosts.iter_mut().zip(self.order.ends()) {
            *end = placed_end;
        }
    }

    // Each group's host and the range of its scopes in the grouped list.
    pub(super) fn iter(&self) -> impl Iterator<Item = (&Rc<SlotsHost>, std::ops::Range<usize>)> {
        let mut start = 0;
        self.hosts.iter().map(move |(host, end)| {
            let range = start..*end;
            start = *end;
            (host, range)
        })
    }

    // Lets go of the hosts, keeping the storage for the next pass.
    pub(super) fn clear(&mut self) {
        self.hosts.clear();
    }
}
