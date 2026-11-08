//! What a layout box keeps of its node: the node's modifier, slices,
//! semantics and kind, read from the applier when the box is made.

use std::{cell::RefCell, fmt, rc::Rc};

use cranpose_core::{MemoryApplier, NodeError, NodeId};
use cranpose_foundation::SemanticsConfiguration;
use cranpose_ui_graphics::{ProjectiveTransform, layer_transform::layer_transform_to_window};

use super::{SemanticsRole, SemanticsText};
use crate::{
    modifier::{Modifier, ModifierNodeSlices, Point, ResolvedModifiers, Size},
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{LayoutNode, LayoutState},
};

/// Snapshot of the data required to render a layout node.
#[derive(Debug, Clone)]
pub struct LayoutNodeData {
    /// Composable origins retained by an inspection-enabled layout node.
    #[cfg(feature = "inspection")]
    pub source_trace: Rc<[cranpose_core::source_trace::SourceLocation]>,
    /// The modifier chain of the node.
    pub modifier: Modifier,
    /// Padding, layout properties and offset that the modifier chain resolved to.
    pub resolved_modifiers: ResolvedModifiers,
    /// Draw commands, pointer input handlers, text and graphics layer data collected from the
    /// modifier nodes.
    pub modifier_slices: Rc<ModifierNodeSlices>,
    /// Semantics that the modifier chain reported, if any.
    pub semantics: Option<Rc<SemanticsConfiguration>>,
    /// What kind of node this is.
    pub kind: LayoutNodeKind,
    /// Where the node draws among its siblings: its `Modifier::z_index`
    /// plus the z index its parent placed it with.
    pub z_index: f32,
}

impl LayoutNodeData {
    /// Creates node data from its parts.
    pub fn new(
        modifier: Modifier,
        resolved_modifiers: ResolvedModifiers,
        modifier_slices: Rc<ModifierNodeSlices>,
        semantics: Option<Rc<SemanticsConfiguration>>,
        kind: LayoutNodeKind,
    ) -> Self {
        Self {
            #[cfg(feature = "inspection")]
            source_trace: Rc::default(),
            modifier,
            resolved_modifiers,
            modifier_slices,
            semantics,
            kind,
            z_index: 0.0,
        }
    }

    /// Semantics the node's live modifier chain reported when the snapshot
    /// was taken.
    pub fn semantics(&self) -> Option<&SemanticsConfiguration> {
        self.semantics.as_deref()
    }

    /// Returns the padding, layout properties and offset that the modifier chain resolved to.
    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved_modifiers
    }

    /// Returns the draw, pointer input, text and graphics layer data collected from the modifier
    /// nodes.
    pub fn modifier_slices(&self) -> &ModifierNodeSlices {
        &self.modifier_slices
    }
}

/// Classification of the node captured inside a [`LayoutBox`](super::LayoutBox).
///
/// Note: Text content is no longer represented as a distinct LayoutNodeKind.
/// Text nodes now use `LayoutNodeKind::Layout` with their content stored in
/// `modifier_slices.text_content()` via TextModifierNode, following Jetpack
/// Compose's pattern where text is a modifier node capability.
#[derive(Clone)]
pub enum LayoutNodeKind {
    /// A node that measures and places its children.
    Layout,
    /// A subcomposition boundary, such as a lazy list or a `SubcomposeLayout`.
    Subcompose,
    /// A non-interactive spacer.
    Spacer,
    /// A button node.
    Button {
        /// Handler that runs when the button is clicked.
        on_click: Rc<RefCell<dyn FnMut()>>,
    },
    /// A node that fits no other kind.
    Unknown,
}

impl fmt::Debug for LayoutNodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout => f.write_str("Layout"),
            Self::Subcompose => f.write_str("Subcompose"),
            Self::Spacer => f.write_str("Spacer"),
            Self::Button { .. } => f.write_str("Button"),
            Self::Unknown => f.write_str("Unknown"),
        }
    }
}

pub(super) fn snapshot_node_data(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    top_left: Point,
    size: Size,
    parent_transform: ProjectiveTransform,
) -> Result<(LayoutNodeData, ProjectiveTransform), NodeError> {
    let info = runtime_metadata_for(applier, node_id)?;
    let kind = layout_kind_from_metadata(node_id, &info);
    let RuntimeNodeMetadata {
        modifier,
        resolved_modifiers,
        modifier_slices,
        semantics,
        z_index,
        ..
    } = info;

    let window_transform = modifier_slices
        .graphics_layer()
        .map_or(parent_transform, |layer| {
            layer_transform_to_window(
                parent_transform,
                top_left,
                modifier_slices.layer_bounds(size),
                &layer,
            )
        });
    modifier_slices.publish_window_geometry(top_left, window_transform, size);

    let data = LayoutNodeData {
        #[cfg(feature = "inspection")]
        source_trace: applier
            .with_node::<LayoutNode, _>(node_id, |node| Rc::clone(&node.source_trace))
            .unwrap_or_default(),
        modifier,
        resolved_modifiers,
        modifier_slices,
        semantics,
        kind,
        z_index,
    };
    Ok((data, window_transform))
}

#[derive(Clone)]
struct RuntimeNodeMetadata {
    modifier: Modifier,
    resolved_modifiers: ResolvedModifiers,
    modifier_slices: Rc<ModifierNodeSlices>,
    semantics: Option<Rc<SemanticsConfiguration>>,
    role: SemanticsRole,
    // The node's `Modifier::z_index` plus its placement's.
    z_index: f32,
    button_handler: Option<Rc<RefCell<dyn FnMut()>>>,
}

impl Default for RuntimeNodeMetadata {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            resolved_modifiers: ResolvedModifiers::default(),
            modifier_slices: Rc::default(),
            semantics: None,
            role: SemanticsRole::Unknown,
            button_handler: None,
            z_index: 0.0,
        }
    }
}

pub(super) fn role_from_modifier_slices(modifier_slices: &ModifierNodeSlices) -> SemanticsRole {
    modifier_slices
        .annotated_text()
        .map_or(SemanticsRole::Layout, |text| SemanticsRole::Text {
            value: SemanticsText(text.clone()),
        })
}

fn runtime_metadata_for(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<RuntimeNodeMetadata, NodeError> {
    if let Ok(meta) = applier.with_node::<LayoutNode, _>(node_id, |layout| {
        let modifier = layout.modifier.clone();
        let resolved_modifiers = layout.resolved_modifiers();
        let modifier_slices = layout.modifier_slices_snapshot();
        let role = role_from_modifier_slices(&modifier_slices);
        let z_index = modifier_slices.z_index() + layout.with_layout_state(LayoutState::z_index);

        RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            semantics: layout.semantics_configuration().map(Rc::new),
            role,
            button_handler: None,
            z_index,
        }
    }) {
        return Ok(meta);
    }

    if let Ok((modifier, resolved_modifiers, modifier_slices, semantics, z_index)) =
        applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            let modifier_slices = node.modifier_slices_snapshot();
            let placed = node.layout_parts().layout_state().borrow().z_index();
            let z_index = modifier_slices.z_index() + placed;
            (
                node.modifier(),
                node.resolved_modifiers(),
                modifier_slices,
                node.semantics_configuration().map(Rc::new),
                z_index,
            )
        })
    {
        return Ok(RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            semantics,
            role: SemanticsRole::Subcompose,
            button_handler: None,
            z_index,
        });
    }
    Ok(RuntimeNodeMetadata::default())
}

fn layout_kind_from_metadata(_node_id: NodeId, info: &RuntimeNodeMetadata) -> LayoutNodeKind {
    match &info.role {
        SemanticsRole::Layout => LayoutNodeKind::Layout,
        SemanticsRole::Subcompose => LayoutNodeKind::Subcompose,
        SemanticsRole::Text { .. } => LayoutNodeKind::Layout,
        SemanticsRole::Spacer => LayoutNodeKind::Spacer,
        SemanticsRole::Button => {
            let handler = info
                .button_handler
                .as_ref()
                .cloned()
                .unwrap_or_else(|| Rc::new(RefCell::new(|| {})));
            LayoutNodeKind::Button { on_click: handler }
        }
        SemanticsRole::Unknown => LayoutNodeKind::Unknown,
    }
}
