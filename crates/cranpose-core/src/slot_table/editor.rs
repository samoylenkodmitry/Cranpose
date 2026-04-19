use super::*;

pub(crate) struct PassEditor<'a> {
    table: &'a mut SlotTable,
    lifecycle: &'a mut SlotLifecycleCoordinator,
    state: &'a mut SlotWriteSessionState,
    mode: SlotPassMode,
}

impl<'a> PassEditor<'a> {
    pub(crate) fn new(
        table: &'a mut SlotTable,
        lifecycle: &'a mut SlotLifecycleCoordinator,
        state: &'a mut SlotWriteSessionState,
        mode: SlotPassMode,
    ) -> Self {
        Self {
            table,
            lifecycle,
            state,
            mode,
        }
    }

    pub(crate) fn start_recranpose_at_anchor(
        &mut self,
        anchor: AnchorId,
        owner: ScopeId,
    ) -> Option<GroupId> {
        let index = self.table.storage.resolve_anchor(anchor)?;
        if self.table.group_scope_owner(index) == Some(owner) {
            self.table.start_recompose_entry(self.state, index);
            Some(GroupId(index))
        } else {
            None
        }
    }

    pub(crate) fn begin_scoped_group(
        &mut self,
        key: Key,
        init_scope: impl FnOnce() -> RecomposeScope,
    ) -> StartScopedGroup<GroupId> {
        let started = self
            .table
            .start_group_entry(self.lifecycle, self.state, key);
        let scope =
            if let Some(existing_scope) = self.table.group_scope_value(started.index).cloned() {
                existing_scope
            } else {
                let scope = init_scope();
                self.table
                    .storage
                    .set_group_scope(started.index, Some(scope.clone()));
                scope
            };
        StartScopedGroup {
            group: GroupId(started.index),
            anchor: self.table.storage.entry_anchor(started.index),
            scope,
            restored_from_gap: started.restored_from_hidden,
        }
    }

    pub(crate) fn end_group(&mut self) {
        self.table.end_group_entry(self.state);
    }

    pub(crate) fn end_recompose(&mut self) {
        self.table.end_recompose_entry(self.lifecycle, self.state);
    }

    pub(crate) fn use_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> usize {
        let cursor = self.state.cursor;
        let disallow_live_reuse = self.table.current_disallow_live_slot_reuse(self.state);
        let owner_index = self.state.group_stack.last().map(|frame| frame.start);

        match self.table.storage.entry_kind(cursor) {
            Some(kind)
                if kind.matches(EntryClass::Value, EntryVisibility::Live)
                    && !disallow_live_reuse
                    && self.table.storage.value_matches_type::<T>(cursor) =>
            {
                self.table.finish_slot_write_at(self.state, cursor)
            }
            Some(kind)
                if kind.matches(EntryClass::Value, EntryVisibility::Hidden)
                    && !disallow_live_reuse
                    && self.table.storage.value_matches_type::<T>(cursor) =>
            {
                self.table.restore_hidden_entry(owner_index, cursor);
                self.table.finish_slot_write_at(self.state, cursor)
            }
            Some(EntryKind::Occupied {
                class: EntryClass::Value,
                ..
            }) if !disallow_live_reuse => {
                self.table.overwrite_value_entry(
                    self.lifecycle,
                    owner_index,
                    cursor,
                    init(),
                    false,
                );
                self.table.finish_slot_write_at(self.state, cursor)
            }
            _ => {
                self.table.insert_value_entry(self.state, cursor, init());
                self.table.finish_slot_write_at(self.state, cursor)
            }
        }
    }

    pub(crate) fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        let index = self.use_value_slot(|| Owned::new(init()));
        self.table.read_value::<Owned<T>>(index).clone()
    }

    pub(crate) fn record_node(&mut self, id: NodeId, generation: u32) {
        let cursor = self.state.cursor;
        let owner_index = self.state.group_stack.last().map(|frame| frame.start);
        match self.table.storage.node_at(cursor) {
            Some((kind, existing, existing_generation))
                if kind.matches(EntryClass::Node, EntryVisibility::Live)
                    && existing == id
                    && existing_generation == generation =>
            {
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
            Some((kind, existing, existing_generation))
                if kind.matches(EntryClass::Node, EntryVisibility::Hidden)
                    && existing == id
                    && existing_generation == generation =>
            {
                if self
                    .table
                    .current_parent_allows_exact_hidden_node_reuse(self.state)
                {
                    self.table.restore_hidden_entry(owner_index, cursor);
                    let _ = self.table.finish_slot_write_at(self.state, cursor);
                } else if !self.table.current_disallow_live_slot_reuse(self.state) {
                    self.table.overwrite_node_entry(
                        self.lifecycle,
                        owner_index,
                        cursor,
                        id,
                        generation,
                        false,
                    );
                    let _ = self.table.finish_slot_write_at(self.state, cursor);
                } else {
                    self.table
                        .insert_node_entry(self.state, cursor, id, generation);
                    let _ = self.table.finish_slot_write_at(self.state, cursor);
                }
            }
            Some((
                EntryKind::Occupied {
                    class: EntryClass::Node,
                    ..
                },
                _,
                _,
            )) if !self.table.current_disallow_live_slot_reuse(self.state) => {
                self.table.overwrite_node_entry(
                    self.lifecycle,
                    owner_index,
                    cursor,
                    id,
                    generation,
                    false,
                );
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
            _ => {
                self.table
                    .insert_node_entry(self.state, cursor, id, generation);
                let _ = self.table.finish_slot_write_at(self.state, cursor);
            }
        }
    }

    pub(crate) fn advance_after_node_read(&mut self) {
        if self
            .table
            .preserved_hidden_node_at_cursor(self.state, self.state.cursor)
            .is_some()
        {
            let owner_index = self.state.group_stack.last().map(|frame| frame.start);
            self.table
                .restore_hidden_entry(owner_index, self.state.cursor);
        }
        self.state.cursor += 1;
        self.table.update_group_bounds(self.state);
    }

    pub(crate) fn skip_current_group(&mut self) {
        self.table.skip_current(self.state);
    }

    pub(crate) fn finalize_current_group(&mut self) -> bool {
        let mut marked = false;
        if let Some((owner_start, group_end)) = self
            .state
            .group_stack
            .last()
            .map(|frame| (frame.start, frame.end.min(self.table.storage.len())))
        {
            if self.state.cursor < group_end
                && self.table.mark_range_as_hidden(
                    self.lifecycle,
                    self.state.cursor,
                    group_end,
                    Some(owner_start),
                )
            {
                marked = true;
            }
        } else if self.state.cursor < self.table.storage.len()
            && self.table.mark_range_as_hidden(
                self.lifecycle,
                self.state.cursor,
                self.table.storage.len(),
                None,
            )
        {
            marked = true;
        }

        marked
    }

    pub(crate) fn finalize_pass(&mut self) -> bool {
        let mut marked_hidden = false;
        while !self.state.group_stack.is_empty() {
            marked_hidden |= self.finalize_current_group();
            match self.mode {
                SlotPassMode::Compose => self.table.end_group_entry(self.state),
                SlotPassMode::Recompose => {
                    self.table.end_recompose_entry(self.lifecycle, self.state)
                }
            }
        }
        match self.mode {
            SlotPassMode::Compose => marked_hidden | self.finalize_current_group(),
            SlotPassMode::Recompose => marked_hidden,
        }
    }
}
