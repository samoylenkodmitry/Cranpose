use crate::DrawGroup;

// The draw groups of a recording, `None` until a draw scope opens one: most
// recordings open none.
#[derive(Clone, Debug, Default)]
pub(super) struct DrawGroups(Option<Box<GroupLists>>);

#[derive(Clone, Debug, Default)]
struct GroupLists {
    // The groups that `RecordLane::GroupStart` segments open.
    list: Vec<DrawGroup>,
    // While recording, the groups open now, outermost first, as indices
    // into `list`. Empty once the recording is finished.
    open: Vec<u32>,
}

impl DrawGroups {
    // The groups, in recorded order.
    pub(super) fn list(&self) -> &[DrawGroup] {
        self.0.as_deref().map_or(&[], |lists| &lists.list)
    }

    // The groups open now, outermost first, as indices into `list`.
    pub(super) fn open(&self) -> &[u32] {
        self.0.as_deref().map_or(&[], |lists| &lists.open)
    }

    // Opens `group` and returns its index.
    pub(super) fn start(&mut self, group: DrawGroup) -> u32 {
        let lists = self.0.get_or_insert_default();
        let index = lists.list.len() as u32;
        lists.list.push(group);
        lists.open.push(index);
        index
    }

    // Ends the group opened last; false when none is open.
    pub(super) fn end(&mut self) -> bool {
        self.0
            .as_deref_mut()
            .and_then(|lists| lists.open.pop())
            .is_some()
    }

    pub(super) fn clear(&mut self) {
        if let Some(lists) = self.0.as_deref_mut() {
            lists.list.clear();
            lists.open.clear();
        }
    }
}
