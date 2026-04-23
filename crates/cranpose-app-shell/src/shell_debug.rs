use super::*;

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    pub fn debug_info_report(&mut self) -> String {
        let mut report = String::new();
        writeln!(report, "=== DEBUG: CURRENT SCREEN STATE ===").ok();
        if let Some(layout_tree) = self.layout_tree() {
            let renderer = HeadlessRenderer::new();
            let render_scene = renderer.render(layout_tree);
            writeln!(report, "{}", format_layout_tree(layout_tree)).ok();
            writeln!(report, "{}", format_render_scene(&render_scene)).ok();
            writeln!(
                report,
                "{}",
                format_screen_summary(layout_tree, &render_scene)
            )
            .ok();
        } else {
            writeln!(report, "No layout available").ok();
        }
        report
    }

    pub fn log_debug_info(&mut self) -> String {
        let report = self.debug_info_report();
        log::info!(target: "cranpose::debug::screen", "\n{report}");
        report
    }

    /// Get the current layout tree (for robot/testing)
    pub fn layout_tree(&mut self) -> Option<&LayoutTree> {
        self.layout_tree.as_ref()
    }

    /// Get the current semantics tree (for robot/testing)
    pub fn semantics_tree(&self) -> Option<&SemanticsTree> {
        self.semantics_tree.as_ref()
    }

    pub fn root_layout_size(&mut self) -> Option<(f32, f32)> {
        self.layout_tree().map(|tree| {
            let root = tree.root();
            (root.rect.width, root.rect.height)
        })
    }

    pub fn node_layout_bounds(&mut self, target: NodeId) -> Option<(f32, f32, f32, f32)> {
        self.layout_tree()
            .and_then(|tree| find_layout_box(tree.root(), target))
            .map(layout_box_bounds)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_runtime_leak_stats(&mut self) -> RuntimeLeakDebugStats {
        let runtime = self.composition.runtime_handle();
        let (applier_stats, live_node_heap_bytes, recycled_node_heap_bytes) = {
            let applier = self.composition.applier_mut();
            (
                applier.debug_stats(),
                applier.debug_live_node_heap_bytes(),
                applier.debug_recycled_node_heap_bytes(),
            )
        };
        RuntimeLeakDebugStats {
            applier_stats,
            live_node_heap_bytes,
            recycled_node_heap_bytes,
            slot_table_heap_bytes: self.composition.slot_table_heap_bytes(),
            pass_stats: self.composition.debug_last_pass_stats(),
            slot_stats: self.composition.debug_slot_table_stats(),
            observer_stats: self.composition.debug_observer_stats(),
            runtime_stats: runtime.debug_stats(),
            state_arena_stats: runtime.state_arena_debug_stats(),
            recompose_scope_stats: debug_recompose_scope_registry_stats(),
            snapshot_v2_stats: debug_snapshot_v2_stats(),
            snapshot_pinning_stats: debug_snapshot_pinning_stats(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_slot_table_groups(&self) -> Vec<(usize, Key, Option<usize>, usize)> {
        self.composition.debug_dump_slot_table_groups()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_slot_entries(&self) -> Vec<cranpose_core::SlotDebugEntry> {
        self.composition.debug_dump_slot_entries()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn runtime_handle(&self) -> cranpose_core::RuntimeHandle {
        self.composition.runtime_handle()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_live_subcompose_scope_ids(&mut self) -> Vec<(NodeId, Vec<(u64, Vec<usize>)>)> {
        fn collect_node_ids(layout: &LayoutBox, out: &mut Vec<NodeId>) {
            out.push(layout.node_id);
            for child in &layout.children {
                collect_node_ids(child, out);
            }
        }

        let mut node_ids = Vec::new();
        if let Some(tree) = self.layout_tree() {
            collect_node_ids(tree.root(), &mut node_ids);
        }

        let mut applier = self.composition.applier_mut();
        let mut result = Vec::new();
        for node_id in node_ids {
            if let Ok(scope_ids) = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.debug_scope_ids_by_slot()
            }) {
                result.push((node_id, scope_ids));
            }
        }
        result
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_subcompose_slot_table(
        &mut self,
        node_id: NodeId,
        slot_id: u64,
    ) -> Option<Vec<cranpose_core::SlotDebugEntry>> {
        let mut applier = self.composition.applier_mut();
        applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.debug_slot_table_for_slot(SlotId::new(slot_id))
            })
            .ok()
            .flatten()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn debug_subcompose_slot_groups(
        &mut self,
        node_id: NodeId,
        slot_id: u64,
    ) -> Option<Vec<(usize, Key, Option<usize>, usize)>> {
        let mut applier = self.composition.applier_mut();
        applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.debug_slot_table_groups_for_slot(SlotId::new(slot_id))
            })
            .ok()
            .flatten()
    }
}

fn find_layout_box(layout_box: &LayoutBox, target: NodeId) -> Option<&LayoutBox> {
    if layout_box.node_id == target {
        return Some(layout_box);
    }

    layout_box
        .children
        .iter()
        .find_map(|child| find_layout_box(child, target))
}

fn layout_box_bounds(layout_box: &LayoutBox) -> (f32, f32, f32, f32) {
    (
        layout_box.rect.x,
        layout_box.rect.y,
        layout_box.rect.width,
        layout_box.rect.height,
    )
}
