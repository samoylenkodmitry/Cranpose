use smallvec::SmallVec;

use crate::{
    Applier, Command, CommandCallback, DeferredChildCleanupQueue, DirtyBubble, NodeError, NodeId,
    sync_or_settle_children,
};

pub(crate) const COMMAND_FLUSH_THRESHOLD: usize = 4096;
pub(crate) type ChildList = SmallVec<[NodeId; 4]>;
pub(crate) const SMALL_CHILD_SYNC_LINEAR_THRESHOLD: usize = 8;

#[derive(Copy, Clone)]
pub(crate) enum CommandTag {
    BubbleDirty,
    RemoveNode,
    MountNode,
    AttachChild,
    InsertChild,
    MoveChild,
    RemoveChild,
    DetachChild,
    SyncChildren,
    Callback,
}

impl CommandTag {
    fn label(self) -> &'static str {
        match self {
            Self::BubbleDirty => "BubbleDirty",
            Self::RemoveNode => "RemoveNode",
            Self::MountNode => "MountNode",
            Self::AttachChild => "AttachChild",
            Self::InsertChild => "InsertChild",
            Self::MoveChild => "MoveChild",
            Self::RemoveChild => "RemoveChild",
            Self::DetachChild => "DetachChild",
            Self::SyncChildren => "SyncChildren",
            Self::Callback => "Callback",
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) struct BubbleDirtyCommand {
    pub(crate) node_id: NodeId,
    pub(crate) bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
pub(crate) struct AttachChildCommand {
    pub(crate) parent_id: NodeId,
    pub(crate) child_id: NodeId,
    pub(crate) insert_index: Option<usize>,
    pub(crate) bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
pub(crate) struct InsertChildCommand {
    pub(crate) parent_id: NodeId,
    pub(crate) child_id: NodeId,
    pub(crate) appended_index: usize,
    pub(crate) insert_index: usize,
    pub(crate) bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
pub(crate) struct MoveChildCommand {
    pub(crate) parent_id: NodeId,
    pub(crate) from_index: usize,
    pub(crate) to_index: usize,
    pub(crate) bubble: DirtyBubble,
}

#[derive(Copy, Clone)]
pub(crate) struct RemoveChildCommand {
    pub(crate) parent_id: NodeId,
    pub(crate) child_id: NodeId,
}

#[derive(Copy, Clone)]
pub(crate) struct DetachChildCommand {
    pub(crate) parent_id: NodeId,
    pub(crate) child_id: NodeId,
}

pub(crate) struct SyncChildrenCommand {
    pub(crate) parent_id: NodeId,
    /// Where the expected children start in the queue's child ids, and how
    /// many there are; `None` for a parent that kept its children.
    pub(crate) children: Option<(usize, usize)>,
}

/// The commands a composition pass queued for the applier, in order. Kept
/// across passes once applied, so a frame's commands reuse the storage the
/// last frame's grew.
#[derive(Default)]
pub(crate) struct CommandQueue {
    pub(crate) tags: Vec<CommandTag>,
    pub(crate) bubble_dirty: Vec<BubbleDirtyCommand>,
    pub(crate) remove_nodes: Vec<NodeId>,
    pub(crate) mount_nodes: Vec<NodeId>,
    pub(crate) attach_children: Vec<AttachChildCommand>,
    pub(crate) insert_children: Vec<InsertChildCommand>,
    pub(crate) move_children: Vec<MoveChildCommand>,
    pub(crate) remove_children: Vec<RemoveChildCommand>,
    pub(crate) detach_children: Vec<DetachChildCommand>,
    pub(crate) sync_children: Vec<SyncChildrenCommand>,
    pub(crate) sync_child_ids: Vec<NodeId>,
    pub(crate) callbacks: Vec<CommandCallback>,
    pub(crate) cleanup: DeferredChildCleanupQueue,
}

impl CommandQueue {
    pub(crate) fn push_tag(&mut self, tag: CommandTag) {
        self.tags.push(tag);
    }

    pub(crate) fn push(&mut self, command: Command) {
        match command {
            Command::BubbleDirty { node_id, bubble } => {
                self.bubble_dirty
                    .push(BubbleDirtyCommand { node_id, bubble });
                self.push_tag(CommandTag::BubbleDirty);
            }
            Command::RemoveNode { id } => {
                self.remove_nodes.push(id);
                self.push_tag(CommandTag::RemoveNode);
            }
            Command::MountNode { id } => {
                self.mount_nodes.push(id);
                self.push_tag(CommandTag::MountNode);
            }
            Command::AttachChild {
                parent_id,
                child_id,
                insert_index,
                bubble,
            } => {
                self.attach_children.push(AttachChildCommand {
                    parent_id,
                    child_id,
                    insert_index,
                    bubble,
                });
                self.push_tag(CommandTag::AttachChild);
            }
            Command::InsertChild {
                parent_id,
                child_id,
                appended_index,
                insert_index,
                bubble,
            } => {
                self.insert_children.push(InsertChildCommand {
                    parent_id,
                    child_id,
                    appended_index,
                    insert_index,
                    bubble,
                });
                self.push_tag(CommandTag::InsertChild);
            }
            Command::MoveChild {
                parent_id,
                from_index,
                to_index,
                bubble,
            } => {
                self.move_children.push(MoveChildCommand {
                    parent_id,
                    from_index,
                    to_index,
                    bubble,
                });
                self.push_tag(CommandTag::MoveChild);
            }
            Command::RemoveChild {
                parent_id,
                child_id,
            } => {
                self.remove_children.push(RemoveChildCommand {
                    parent_id,
                    child_id,
                });
                self.push_tag(CommandTag::RemoveChild);
            }
            Command::DetachChild {
                parent_id,
                child_id,
            } => {
                self.detach_children.push(DetachChildCommand {
                    parent_id,
                    child_id,
                });
                self.push_tag(CommandTag::DetachChild);
            }
            Command::SyncChildren {
                parent_id,
                expected_children,
            } => {
                let children = expected_children.map(|expected| {
                    let child_start = self.sync_child_ids.len();
                    self.sync_child_ids.extend_from_slice(&expected);
                    (child_start, expected.len())
                });
                self.sync_children.push(SyncChildrenCommand {
                    parent_id,
                    children,
                });
                self.push_tag(CommandTag::SyncChildren);
            }
            Command::Callback(callback) => {
                self.callbacks.push(callback);
                self.push_tag(CommandTag::Callback);
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.tags.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.tags.capacity()
    }

    pub(crate) fn payload_len_bytes(&self) -> usize {
        self.bubble_dirty
            .len()
            .saturating_mul(size_of::<BubbleDirtyCommand>())
            .saturating_add(self.remove_nodes.len().saturating_mul(size_of::<NodeId>()))
            .saturating_add(self.mount_nodes.len().saturating_mul(size_of::<NodeId>()))
            .saturating_add(
                self.attach_children
                    .len()
                    .saturating_mul(size_of::<AttachChildCommand>()),
            )
            .saturating_add(
                self.insert_children
                    .len()
                    .saturating_mul(size_of::<InsertChildCommand>()),
            )
            .saturating_add(
                self.move_children
                    .len()
                    .saturating_mul(size_of::<MoveChildCommand>()),
            )
            .saturating_add(
                self.remove_children
                    .len()
                    .saturating_mul(size_of::<RemoveChildCommand>()),
            )
            .saturating_add(
                self.detach_children
                    .len()
                    .saturating_mul(size_of::<DetachChildCommand>()),
            )
            .saturating_add(
                self.sync_children
                    .len()
                    .saturating_mul(size_of::<SyncChildrenCommand>()),
            )
            .saturating_add(
                self.sync_child_ids
                    .len()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.callbacks
                    .len()
                    .saturating_mul(size_of::<CommandCallback>()),
            )
    }

    pub(crate) fn payload_capacity_bytes(&self) -> usize {
        self.bubble_dirty
            .capacity()
            .saturating_mul(size_of::<BubbleDirtyCommand>())
            .saturating_add(
                self.remove_nodes
                    .capacity()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.mount_nodes
                    .capacity()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.attach_children
                    .capacity()
                    .saturating_mul(size_of::<AttachChildCommand>()),
            )
            .saturating_add(
                self.insert_children
                    .capacity()
                    .saturating_mul(size_of::<InsertChildCommand>()),
            )
            .saturating_add(
                self.move_children
                    .capacity()
                    .saturating_mul(size_of::<MoveChildCommand>()),
            )
            .saturating_add(
                self.remove_children
                    .capacity()
                    .saturating_mul(size_of::<RemoveChildCommand>()),
            )
            .saturating_add(
                self.detach_children
                    .capacity()
                    .saturating_mul(size_of::<DetachChildCommand>()),
            )
            .saturating_add(
                self.sync_children
                    .capacity()
                    .saturating_mul(size_of::<SyncChildrenCommand>()),
            )
            .saturating_add(
                self.sync_child_ids
                    .capacity()
                    .saturating_mul(size_of::<NodeId>()),
            )
            .saturating_add(
                self.callbacks
                    .capacity()
                    .saturating_mul(size_of::<CommandCallback>()),
            )
    }

    /// Applies the queued commands in order and empties the queue, keeping
    /// its storage. A failed command leaves the queue empty too.
    pub(crate) fn apply(&mut self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        applier.begin_bubble_batch();
        let result = self.apply_in_order(applier);
        applier.end_bubble_batch();
        self.clear();
        result
    }

    fn clear(&mut self) {
        let Self {
            tags,
            bubble_dirty,
            remove_nodes,
            mount_nodes,
            attach_children,
            insert_children,
            move_children,
            remove_children,
            detach_children,
            sync_children,
            sync_child_ids,
            callbacks,
            cleanup,
        } = self;
        tags.clear();
        bubble_dirty.clear();
        remove_nodes.clear();
        mount_nodes.clear();
        attach_children.clear();
        insert_children.clear();
        move_children.clear();
        remove_children.clear();
        detach_children.clear();
        sync_children.clear();
        sync_child_ids.clear();
        callbacks.clear();
        cleanup.clear();
    }

    fn apply_in_order(&mut self, applier: &mut dyn Applier) -> Result<(), NodeError> {
        let mut payloads = CommandPayloads {
            bubble_dirty: self.bubble_dirty.drain(..),
            remove_nodes: self.remove_nodes.drain(..),
            mount_nodes: self.mount_nodes.drain(..),
            attach_children: self.attach_children.drain(..),
            insert_children: self.insert_children.drain(..),
            move_children: self.move_children.drain(..),
            remove_children: self.remove_children.drain(..),
            detach_children: self.detach_children.drain(..),
            sync_children: self.sync_children.drain(..),
            sync_child_ids: &self.sync_child_ids,
            callbacks: self.callbacks.drain(..),
        };
        for &tag in &self.tags {
            payloads.apply(tag, applier, &mut self.cleanup)?;
        }
        payloads.assert_all_taken();
        self.cleanup.flush(applier)
    }
}

/// The payloads of a queue being applied, taken in the order its tags name
/// them.
struct CommandPayloads<'a> {
    bubble_dirty: std::vec::Drain<'a, BubbleDirtyCommand>,
    remove_nodes: std::vec::Drain<'a, NodeId>,
    mount_nodes: std::vec::Drain<'a, NodeId>,
    attach_children: std::vec::Drain<'a, AttachChildCommand>,
    insert_children: std::vec::Drain<'a, InsertChildCommand>,
    move_children: std::vec::Drain<'a, MoveChildCommand>,
    remove_children: std::vec::Drain<'a, RemoveChildCommand>,
    detach_children: std::vec::Drain<'a, DetachChildCommand>,
    sync_children: std::vec::Drain<'a, SyncChildrenCommand>,
    sync_child_ids: &'a [NodeId],
    callbacks: std::vec::Drain<'a, CommandCallback>,
}

impl CommandPayloads<'_> {
    fn apply(
        &mut self,
        tag: CommandTag,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match tag {
            CommandTag::BubbleDirty
            | CommandTag::RemoveNode
            | CommandTag::MountNode
            | CommandTag::Callback => self.apply_node_command(tag, applier, cleanup),
            CommandTag::AttachChild
            | CommandTag::InsertChild
            | CommandTag::MoveChild
            | CommandTag::RemoveChild
            | CommandTag::DetachChild
            | CommandTag::SyncChildren => self.apply_child_command(tag, applier, cleanup),
        }
    }

    fn apply_node_command(
        &mut self,
        tag: CommandTag,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match tag {
            CommandTag::BubbleDirty => {
                let BubbleDirtyCommand { node_id, bubble } =
                    next_command_payload(&mut self.bubble_dirty, tag)?;
                Command::BubbleDirty { node_id, bubble }.apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::RemoveNode => {
                let id = next_command_payload(&mut self.remove_nodes, tag)?;
                Command::RemoveNode { id }.apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::MountNode => {
                let id = next_command_payload(&mut self.mount_nodes, tag)?;
                Command::MountNode { id }.apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::Callback => {
                let callback = next_command_payload(&mut self.callbacks, tag)?;
                Command::Callback(callback).apply_with_cleanup(applier, cleanup)?;
                applier.forget_bubble_passes();
            }
            CommandTag::AttachChild
            | CommandTag::InsertChild
            | CommandTag::MoveChild
            | CommandTag::RemoveChild
            | CommandTag::DetachChild
            | CommandTag::SyncChildren => return Err(command_payload_error(tag)),
        }
        Ok(())
    }

    fn apply_child_command(
        &mut self,
        tag: CommandTag,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        match tag {
            CommandTag::AttachChild => {
                let AttachChildCommand {
                    parent_id,
                    child_id,
                    insert_index,
                    bubble,
                } = next_command_payload(&mut self.attach_children, tag)?;
                Command::AttachChild {
                    parent_id,
                    child_id,
                    insert_index,
                    bubble,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::InsertChild => {
                let InsertChildCommand {
                    parent_id,
                    child_id,
                    appended_index,
                    insert_index,
                    bubble,
                } = next_command_payload(&mut self.insert_children, tag)?;
                Command::InsertChild {
                    parent_id,
                    child_id,
                    appended_index,
                    insert_index,
                    bubble,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::MoveChild => {
                let MoveChildCommand {
                    parent_id,
                    from_index,
                    to_index,
                    bubble,
                } = next_command_payload(&mut self.move_children, tag)?;
                Command::MoveChild {
                    parent_id,
                    from_index,
                    to_index,
                    bubble,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::RemoveChild => {
                let RemoveChildCommand {
                    parent_id,
                    child_id,
                } = next_command_payload(&mut self.remove_children, tag)?;
                Command::RemoveChild {
                    parent_id,
                    child_id,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::DetachChild => {
                let DetachChildCommand {
                    parent_id,
                    child_id,
                } = next_command_payload(&mut self.detach_children, tag)?;
                Command::DetachChild {
                    parent_id,
                    child_id,
                }
                .apply_with_cleanup(applier, cleanup)?;
            }
            CommandTag::SyncChildren => self.sync_children(applier, cleanup)?,
            CommandTag::BubbleDirty
            | CommandTag::RemoveNode
            | CommandTag::MountNode
            | CommandTag::Callback => return Err(command_payload_error(tag)),
        }
        Ok(())
    }

    fn sync_children(
        &mut self,
        applier: &mut dyn Applier,
        cleanup: &mut DeferredChildCleanupQueue,
    ) -> Result<(), NodeError> {
        let tag = CommandTag::SyncChildren;
        let SyncChildrenCommand {
            parent_id,
            children,
        } = next_command_payload(&mut self.sync_children, tag)?;
        let expected_children = children
            .map(|(child_start, child_len)| {
                child_start
                    .checked_add(child_len)
                    .and_then(|child_end| self.sync_child_ids.get(child_start..child_end))
                    .ok_or_else(|| command_payload_error(tag))
            })
            .transpose()?;
        sync_or_settle_children(applier, parent_id, expected_children, cleanup)
    }

    fn assert_all_taken(&mut self) {
        debug_assert!(self.bubble_dirty.next().is_none());
        debug_assert!(self.remove_nodes.next().is_none());
        debug_assert!(self.mount_nodes.next().is_none());
        debug_assert!(self.attach_children.next().is_none());
        debug_assert!(self.insert_children.next().is_none());
        debug_assert!(self.move_children.next().is_none());
        debug_assert!(self.remove_children.next().is_none());
        debug_assert!(self.detach_children.next().is_none());
        debug_assert!(self.sync_children.next().is_none());
        debug_assert!(self.callbacks.next().is_none());
    }
}

fn command_payload_error(tag: CommandTag) -> NodeError {
    NodeError::MalformedCommandPayload { tag: tag.label() }
}

fn next_command_payload<T>(
    payloads: &mut impl Iterator<Item = T>,
    tag: CommandTag,
) -> Result<T, NodeError> {
    payloads.next().ok_or_else(|| command_payload_error(tag))
}
