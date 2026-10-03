use std::{
    cell::{BorrowMutError, Cell, RefCell, RefMut},
    ops::{Deref, DerefMut},
    rc::Rc,
};

use crate::{
    Applier, NodeError, NodeId, SlotsHost,
    slot::{DetachedSubtree, SlotLifecycleCoordinator, dispose_detached_node_now},
};

/// Detached node identities and the resources that must outlive their unmount.
#[derive(Default)]
pub struct NodeDisposal {
    nodes: Vec<(NodeId, u32)>,
    lifecycle: SlotLifecycleCoordinator,
    slot_hosts: Vec<Rc<SlotsHost>>,
}

impl NodeDisposal {
    /// Creates a disposal batch from node IDs and their current generations.
    ///
    /// ```
    /// use cranpose_core::{Applier, MemoryApplier, Node, NodeDisposal};
    /// struct Leaf;
    /// impl Node for Leaf {}
    /// let mut applier = MemoryApplier::new();
    /// let id = applier.create(Box::new(Leaf));
    /// let disposal = NodeDisposal::new(vec![(id, applier.node_generation(id))]);
    /// ```
    pub fn new(nodes: Vec<(NodeId, u32)>) -> Self {
        Self {
            nodes,
            lifecycle: SlotLifecycleCoordinator::default(),
            slot_hosts: Vec::new(),
        }
    }

    pub(crate) fn from_subcomposed(
        disposal: crate::subcompose::SubcomposeDisposal,
        applier: &dyn Applier,
    ) -> Self {
        Self {
            nodes: disposal
                .nodes
                .into_iter()
                .map(|id| (id, applier.node_generation(id)))
                .collect(),
            lifecycle: SlotLifecycleCoordinator::default(),
            slot_hosts: disposal.slot_hosts,
        }
    }

    pub(crate) fn retain_subtree(&mut self, subtree: DetachedSubtree) {
        subtree.append_disposal_roots(&mut self.nodes);
        self.lifecycle.queue_subtree_disposal(subtree);
    }
}

#[derive(Default)]
struct NodeDisposalQueue {
    batches: RefCell<Vec<NodeDisposal>>,
    pending: Cell<bool>,
}

impl NodeDisposalQueue {
    fn enqueue(&self, disposal: NodeDisposal) {
        if !disposal.nodes.is_empty()
            || !disposal.slot_hosts.is_empty()
            || disposal.lifecycle.pending_drops_len() != 0
        {
            self.batches.borrow_mut().push(disposal);
            self.pending.set(true);
        }
    }

    fn flush<A: Applier + ?Sized>(
        &self,
        applier: &mut A,
        dispose: fn(&mut A, NodeId) -> Result<(), NodeError>,
    ) -> Result<(), NodeError> {
        while self.pending.get() {
            let next = self.batches.borrow_mut().pop();
            let Some(mut batch) = next else {
                self.pending.set(false);
                break;
            };
            if let Some((node, generation)) = batch.nodes.pop() {
                let resume_at = self.batches.borrow().len();
                self.batches.borrow_mut().push(batch);
                if applier.node_generation(node) == generation
                    && let Err(error) = dispose(applier, node)
                {
                    self.batches.borrow_mut()[resume_at]
                        .nodes
                        .push((node, generation));
                    return Err(error);
                }
            } else if let Some(host) = batch.slot_hosts.pop() {
                self.batches.borrow_mut().push(batch);
                drop(host);
            } else if let Some(payload) = batch.lifecycle.pop_pending_drop() {
                self.batches.borrow_mut().push(batch);
                drop(payload);
            }
        }
        Ok(())
    }
}

/// Owns an applier and completes node disposal when an outstanding borrow ends.
pub trait ApplierHost {
    fn borrow_dyn(&self) -> ApplierGuard<'_, dyn Applier>;

    /// Disposes detached roots, deferring recursive cleanup to the current borrow's guard.
    ///
    /// ```
    /// use cranpose_core::{
    ///     Applier, ApplierHost, ConcreteApplierHost, MemoryApplier, Node, NodeDisposal,
    /// };
    /// # fn example() -> Result<(), cranpose_core::NodeError> {
    /// struct Leaf;
    /// impl Node for Leaf {}
    /// let host = ConcreteApplierHost::new(MemoryApplier::new());
    /// let mut applier = host.borrow_typed();
    /// let root = applier.create(Box::new(Leaf));
    /// host.dispose_nodes(NodeDisposal::new(vec![(
    ///     root,
    ///     applier.node_generation(root),
    /// )]))?;
    /// drop(applier);
    /// assert!(host.borrow_typed().get_mut(root).is_err());
    /// # Ok(())
    /// # }
    /// ```
    fn dispose_nodes(&self, disposal: NodeDisposal) -> Result<(), NodeError>;

    /// Compacts internal storage after commands have been applied.
    fn compact(&self) {}
}

pub struct ConcreteApplierHost<A: Applier + 'static> {
    inner: RefCell<A>,
    disposals: NodeDisposalQueue,
}

impl<A: Applier + 'static> ConcreteApplierHost<A> {
    pub fn new(applier: A) -> Self {
        Self {
            inner: RefCell::new(applier),
            disposals: NodeDisposalQueue::default(),
        }
    }

    pub fn borrow_typed(&self) -> ApplierGuard<'_, A> {
        self.guard(self.inner.borrow_mut())
    }

    pub fn try_borrow_typed(&self) -> Result<ApplierGuard<'_, A>, BorrowMutError> {
        self.inner.try_borrow_mut().map(|inner| self.guard(inner))
    }

    fn guard<'a>(&'a self, inner: RefMut<'a, A>) -> ApplierGuard<'a, A> {
        ApplierGuard {
            inner,
            disposals: &self.disposals,
            dispose: |applier, node| dispose_detached_node_now(applier, node),
        }
    }

    pub fn into_inner(self) -> A {
        drop(self.borrow_typed());
        self.inner.into_inner()
    }
}

impl<A: Applier + 'static> ApplierHost for ConcreteApplierHost<A> {
    fn borrow_dyn(&self) -> ApplierGuard<'_, dyn Applier> {
        ApplierGuard {
            inner: RefMut::map(self.inner.borrow_mut(), |applier| {
                applier as &mut dyn Applier
            }),
            disposals: &self.disposals,
            dispose: dispose_detached_node_now,
        }
    }

    fn dispose_nodes(&self, disposal: NodeDisposal) -> Result<(), NodeError> {
        self.disposals.enqueue(disposal);
        if let Ok(mut applier) = self.inner.try_borrow_mut() {
            return self.disposals.flush(&mut *applier, |applier, node| {
                dispose_detached_node_now(applier, node)
            });
        }
        Ok(())
    }

    fn compact(&self) {
        self.borrow_typed().compact();
    }
}

/// A mutable applier borrow that drains recursive node disposal before returning.
pub struct ApplierGuard<'a, A: Applier + ?Sized + 'static> {
    inner: RefMut<'a, A>,
    disposals: &'a NodeDisposalQueue,
    dispose: fn(&mut A, NodeId) -> Result<(), NodeError>,
}

impl<A: Applier + ?Sized + 'static> ApplierGuard<'_, A> {
    fn flush_disposals(&mut self) -> Result<(), NodeError> {
        self.disposals.flush(&mut *self.inner, self.dispose)
    }
}

impl<A: Applier + ?Sized + 'static> Deref for ApplierGuard<'_, A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<A: Applier + ?Sized + 'static> DerefMut for ApplierGuard<'_, A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<A: Applier + ?Sized + 'static> Drop for ApplierGuard<'_, A> {
    fn drop(&mut self) {
        if self.disposals.pending.get()
            && let Err(error) = self.flush_disposals()
        {
            log::error!("deferred node disposal failed: {error}");
        }
    }
}
