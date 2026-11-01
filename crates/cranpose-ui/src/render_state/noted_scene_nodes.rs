/// The nodes a layout pass names to the scene, each once a take: a node
/// keeps the take it was last noted in, so a second note in the same take
/// is dropped with one comparison instead of a hash. A node pushed without
/// that record may come twice, which the scene's routing marks absorb.
pub(super) struct NotedSceneNodes<T> {
    nodes: Vec<T>,
    /// The take the list fills: a number no other list on the thread uses.
    take: u64,
}

impl<T> NotedSceneNodes<T> {
    pub(super) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            take: next_moved_scene_node_take(),
        }
    }

    pub(super) fn note(&mut self, node: T, noted_in: &mut u64) {
        if *noted_in != self.take {
            *noted_in = self.take;
            self.nodes.push(node);
        }
    }

    pub(super) fn push(&mut self, node: T) {
        self.nodes.push(node);
    }

    /// Moves the nodes into `output` and keeps the storage `output` held
    /// for the next take.
    pub(super) fn take_into(&mut self, output: &mut Vec<T>) {
        output.clear();
        if !self.nodes.is_empty() {
            std::mem::swap(&mut self.nodes, output);
            self.take = next_moved_scene_node_take();
        }
    }
}

fn next_moved_scene_node_take() -> u64 {
    crate::thread_state::cells(|cells| {
        let take = cells.moved_scene_node_takes.get() + 1;
        cells.moved_scene_node_takes.set(take);
        take
    })
}
