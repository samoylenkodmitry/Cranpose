//! Helpers shared by the crate's tests.

use cranpose_core::{Composition, MemoryApplier, NodeId};

use crate::{
    LayoutEngine,
    modifier::Size,
    renderer::{HeadlessRenderer, RenderOp},
};

/// Lays out `root` in `size` and returns the texts the headless renderer
/// draws, in draw order.
pub(crate) fn rendered_texts(
    composition: &mut Composition<MemoryApplier>,
    root: NodeId,
    size: Size,
) -> Vec<String> {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier.compute_layout(root, size).expect("layout");
    applier.clear_runtime_handle();
    HeadlessRenderer::new()
        .render(&layout)
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}
