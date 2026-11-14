//! The state reads of the work a pass does for each node, as a layout pass's
//! measures: a read is kept for the node whose work made it, the node's next
//! run replaces its reads, and a change of a state it read hands the node to
//! the owner. Compose observes a measure's reads the same way; work that
//! reads nothing pays for a cell write.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use smallvec::SmallVec;

use crate::{
    NodeId,
    collections::map::HashMap,
    snapshot_v2::{
        ObserverHandle, ReadObserver, TransparentObserverMutableSnapshot,
        current_snapshot_reads_into, register_apply_observer,
        take_transparent_observer_mutable_snapshot_reusing,
    },
    state::{ObjectId, StateObject},
};

type ReadSnapshot = Rc<TransparentObserverMutableSnapshot>;

// Takes the nodes whose reads a change of state reaches.
type OnChanged = Box<dyn Fn(&[NodeId])>;

/// Which states the work of each node read in the passes it ran in.
pub struct NodeReads {
    this: Weak<Self>,
    // The node whose work runs: `None` outside it and while it is paused.
    reader: Cell<Option<NodeId>>,
    // The reads of the running pass, with the node that made each.
    seen: RefCell<Vec<(NodeId, ObjectId)>>,
    // The nodes the pass ran again that hold reads of an earlier run.
    ran_again: RefCell<Vec<NodeId>>,
    // The states each node's last run read.
    by_node: RefCell<HashMap<NodeId, SmallVec<[ObjectId; 2]>>>,
    // The nodes whose last run read each state.
    by_state: RefCell<HashMap<ObjectId, SmallVec<[NodeId; 2]>>>,
    // How many nodes hold reads, which a run checks before it looks its
    // node up.
    holders: Cell<usize>,
    read_observer: ReadObserver,
    // The snapshot of the last pass, for the next one to enter again.
    read_snapshot: RefCell<Option<ReadSnapshot>>,
    // Registered when a pass first keeps a read.
    apply: RefCell<Option<ObserverHandle>>,
    on_changed: OnChanged,
}

/// Puts back the reader it replaced when dropped.
pub struct ReaderGuard<'a> {
    reads: &'a NodeReads,
    previous: Option<NodeId>,
}

impl Drop for ReaderGuard<'_> {
    fn drop(&mut self) {
        self.reads.reader.set(self.previous);
    }
}

impl NodeReads {
    /// Reads that call `on_changed` with the nodes whose reads a change of
    /// state reaches.
    pub fn new(on_changed: impl Fn(&[NodeId]) + 'static) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<Self>| {
            let observer = Weak::clone(this);
            Self {
                this: Weak::clone(this),
                reader: Cell::new(None),
                seen: RefCell::default(),
                ran_again: RefCell::default(),
                by_node: RefCell::default(),
                by_state: RefCell::default(),
                holders: Cell::new(0),
                read_observer: Rc::new(move |state: &dyn StateObject| {
                    if let Some(reads) = observer.upgrade() {
                        reads.note(state.object_id());
                    }
                }),
                read_snapshot: RefCell::new(None),
                apply: RefCell::new(None),
                on_changed: Box::new(on_changed),
            }
        })
    }

    /// Runs `pass`, keeping each read for the node whose work made it.
    pub fn observe<R>(&self, pass: impl FnOnce() -> R) -> R {
        if current_snapshot_reads_into(&self.read_observer) {
            return pass();
        }
        let mut snapshot = take_transparent_observer_mutable_snapshot_reusing(
            Some(Rc::clone(&self.read_observer)),
            None,
            self.read_snapshot.take(),
        );
        let result = snapshot.enter(pass);
        snapshot.dispose();
        if Rc::get_mut(&mut snapshot).is_some() && !snapshot.has_pending_changes() {
            self.read_snapshot.replace(Some(snapshot));
        }
        self.keep_seen();
        result
    }

    /// Makes `node` the reader until the guard drops: the reads of its work
    /// replace those of its earlier run.
    #[inline]
    pub fn reading(&self, node: NodeId) -> ReaderGuard<'_> {
        if self.holders.get() != 0 {
            self.note_run(node);
        }
        ReaderGuard {
            reads: self,
            previous: self.reader.replace(Some(node)),
        }
    }

    /// Keeps no reads until the guard drops, as while a node's work composes
    /// content of its own.
    #[inline]
    pub fn paused(&self) -> ReaderGuard<'_> {
        ReaderGuard {
            reads: self,
            previous: self.reader.replace(None),
        }
    }

    /// How many reads the running pass has kept, to tell whether some work
    /// read a state.
    #[inline]
    pub fn seen(&self) -> usize {
        self.seen.borrow().len()
    }

    /// Drops the reads `node` holds, as when it leaves.
    pub fn forget(&self, node: NodeId) {
        if self.holders.get() == 0 {
            return;
        }
        let Some(states) = self.by_node.borrow_mut().remove(&node) else {
            return;
        };
        self.holders.set(self.holders.get() - 1);
        let mut by_state = self.by_state.borrow_mut();
        for state in states {
            if let Some(nodes) = by_state.get_mut(&state) {
                nodes.retain(|reader| *reader != node);
                if nodes.is_empty() {
                    by_state.remove(&state);
                }
            }
        }
    }

    // Out of line: few passes run a node that holds reads.
    #[inline(never)]
    fn note_run(&self, node: NodeId) {
        if self.by_node.borrow().contains_key(&node) {
            self.ran_again.borrow_mut().push(node);
        }
    }

    fn note(&self, state: ObjectId) {
        if let Some(node) = self.reader.get() {
            self.seen.borrow_mut().push((node, state));
        }
    }

    // Replaces the reads of the nodes the pass ran again with those it saw.
    fn keep_seen(&self) {
        for node in self.ran_again.borrow_mut().drain(..) {
            self.forget(node);
        }
        let mut seen = self.seen.borrow_mut();
        if seen.is_empty() {
            return;
        }
        // A node that reads one state again and again notes it once.
        seen.dedup();
        {
            let mut by_node = self.by_node.borrow_mut();
            let mut by_state = self.by_state.borrow_mut();
            for &(node, state) in seen.iter() {
                let states = by_node.entry(node).or_insert_with(|| {
                    self.holders.set(self.holders.get() + 1);
                    SmallVec::new()
                });
                if !states.contains(&state) {
                    states.push(state);
                    by_state.entry(state).or_default().push(node);
                }
            }
        }
        seen.clear();
        drop(seen);
        self.watch_changes();
    }

    fn watch_changes(&self) {
        if self.apply.borrow().is_some() {
            return;
        }
        let this = Weak::clone(&self.this);
        let handle = register_apply_observer(Rc::new(move |modified, _| {
            if let Some(reads) = this.upgrade() {
                reads.changed(modified);
            }
        }));
        *self.apply.borrow_mut() = Some(handle);
    }

    fn changed(&self, modified: &[Rc<dyn StateObject>]) {
        let mut reached: SmallVec<[NodeId; 4]> = SmallVec::new();
        {
            let by_state = self.by_state.borrow();
            for state in modified {
                if let Some(nodes) = by_state.get(&state.object_id()) {
                    reached.extend(nodes.iter().copied());
                }
            }
        }
        if !reached.is_empty() {
            (self.on_changed)(&reached);
        }
    }
}
