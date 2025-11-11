//! High level UI primitives built on top of the Compose core runtime.

use compose_core::{location_key, MemoryApplier};
pub use compose_core::{Composition, Key};
pub use compose_macros::composable;

mod debug;
mod draw;
mod layout;
mod modifier;
mod modifier_nodes;
mod primitives;
mod render_state;
mod renderer;
mod subcompose_layout;
mod text;
pub mod widgets;

pub use compose_ui_graphics::Dp;
pub use compose_ui_layout::IntrinsicSize;
pub use draw::{execute_draw_commands, DrawCacheBuilder, DrawCommand};
pub use layout::{
    core::{
        Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, Placeable,
        VerticalAlignment,
    },
    measure_layout, tree_needs_layout, LayoutBox, LayoutEngine, LayoutMeasurements, LayoutNodeData,
    LayoutNodeKind, LayoutTree, SemanticsAction, SemanticsCallback, SemanticsNode, SemanticsRole,
    SemanticsTree,
};
pub use modifier::{
    collect_modifier_slices, collect_slices_from_modifier, Brush, Color, CornerRadii, EdgeInsets,
    GraphicsLayer, Modifier, ModifierNodeSlices, Point, PointerEvent, PointerEventKind,
    PointerInputScope, Rect, ResolvedBackground, ResolvedModifiers, RoundedCornerShape, Size,
};
pub use modifier_nodes::{
    AlphaElement, AlphaNode, BackgroundElement, BackgroundNode, ClickableElement, ClickableNode,
    CornerShapeElement, CornerShapeNode, FillDirection, FillElement, FillNode, OffsetElement,
    OffsetNode, PaddingElement, PaddingNode, SizeElement, SizeNode,
};
pub use primitives::{
    Box, BoxScope, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope,
    BoxWithConstraintsScopeImpl, Button, ButtonNode, Column, ColumnSpec, ForEach, Layout,
    LayoutNode, Row, RowSpec, Spacer, SpacerNode, SubcomposeLayout, Text, TextNode,
};
pub use render_state::{
    peek_focus_invalidation, peek_pointer_invalidation, peek_render_invalidation,
    request_focus_invalidation, request_pointer_invalidation, request_render_invalidation,
    take_focus_invalidation, take_pointer_invalidation, take_render_invalidation,
};
pub use renderer::{HeadlessRenderer, PaintLayer, RecordedRenderScene, RenderOp};
pub use subcompose_layout::{
    Constraints, MeasureResult, Placement, SubcomposeLayoutNode, SubcomposeLayoutScope,
    SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};
pub use text::{measure_text, set_text_measurer, TextMeasurer, TextMetrics};

// Debug utilities
pub use debug::{
    format_layout_tree, format_modifier_chain, format_render_scene, install_modifier_chain_trace,
    log_layout_tree, log_modifier_chain, log_render_scene, log_screen_summary,
    ModifierChainTraceGuard,
};

/// Convenience alias used in examples and tests.
pub type TestComposition = Composition<MemoryApplier>;

/// Build a composition with a simple in-memory applier and run the provided closure once.
pub fn run_test_composition(mut build: impl FnMut()) -> TestComposition {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), || build())
        .expect("initial render succeeds");
    composition
}

pub use compose_core::MutableState as SnapshotState;

#[cfg(test)]
#[path = "tests/anchor_async_tests.rs"]
mod anchor_async_tests;

#[cfg(test)]
#[path = "tests/async_runtime_full_layout_test.rs"]
mod async_runtime_full_layout_test;

#[cfg(test)]
#[path = "tests/tab_switching_tests.rs"]
mod tab_switching_tests;
