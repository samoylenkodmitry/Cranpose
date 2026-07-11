//! High level UI primitives built on top of the Compose core runtime.

#![deny(unsafe_code)]

use cranpose_core::{location_key, ApplierGuard, MemoryApplier, NodeError, NodeId, RuntimeHandle};
pub use cranpose_core::{Composition, Key};
pub use cranpose_macros::composable;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

pub mod bring_into_view;
pub mod clipboard_session;
mod cursor_animation;
mod debug;
mod draw;
pub mod fling_animation;
mod focus_dispatch;
mod interaction;
mod key_event;
pub mod layout;
mod modifier;
mod modifier_nodes;
mod pointer_dispatch;
mod primitives;
mod render_state;
mod renderer;
pub mod safe_area;
pub mod scroll;
mod subcompose_layout;
pub mod text;
pub mod text_field_focus;
mod text_field_handler;
mod text_field_input;
mod text_field_modifier_node;
pub mod text_input_session;
pub mod text_layout_result;
mod text_modifier_node;
pub mod text_selection;
pub mod widgets;
mod word_boundaries;
pub mod zoom;

// Export for cursor blink animation - AppShell checks this to continuously redraw
pub use text_field_focus::has_focused_field;
// Editable-state snapshot for platform IMEs (Android InputConnection, web
// composition) - platform runtimes read it through the shell
pub use text_field_focus::ImeEditorState;
// Platform soft-keyboard bridge - platform runtimes install a handler so text
// field focus changes can show/hide the on-screen keyboard
pub use text_input_session::PlatformTextInputHandler;
// Export cursor blink timing for WaitUntil scheduling
pub use cursor_animation::{
    is_cursor_visible, next_cursor_blink_time, reset_cursor_blink, start_cursor_blink,
    stop_cursor_blink, tick_cursor_blink,
};

pub use bring_into_view::{
    local_bring_into_view_responder, scroll_delta_to_reveal, BringIntoViewResponder,
};
pub use cranpose_ui_graphics::{BlurredEdgeTreatment, ColorFilter, Dp, ImageBitmap, ImageSampling};
pub use cranpose_ui_layout::IntrinsicSize;
pub use draw::{execute_draw_commands, DrawCacheBuilder, DrawCommand};
pub use focus_dispatch::{
    active_focus_target, clear_focus_invalidations, has_pending_focus_invalidations,
    process_focus_invalidations, schedule_focus_invalidation, set_active_focus_target,
};
pub use interaction::{
    collect_is_pressed_as_state, rememberMutableInteractionSource, Interaction,
    MutableInteractionSource, PressInteraction, PressInteractionCancel, PressInteractionPress,
    PressInteractionRelease,
};
pub use safe_area::{local_ime_insets, local_safe_area_insets};
// Re-export FocusManager from cranpose-foundation to avoid duplication
pub use cranpose_foundation::nodes::input::focus::FocusManager;
pub use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState,
};
pub use layout::{
    build_layout_tree_from_applier, build_semantics_tree_from_applier,
    build_semantics_tree_from_layout_tree,
    core::{
        Alignment, Arrangement, HorizontalAlignment, LinearArrangement, Measurable, Placeable,
        VerticalAlignment,
    },
    measure_layout, measure_layout_with_options, tree_needs_layout, tree_needs_semantics,
    LayoutAllocationDebugStats, LayoutBox, LayoutEngine, LayoutMeasurements, LayoutNodeData,
    LayoutNodeKind, LayoutTree, MeasureLayoutOptions, SemanticsAction, SemanticsCallback,
    SemanticsNode, SemanticsRole, SemanticsTree,
};
pub use modifier::{
    collect_modifier_slices, collect_semantics_from_modifier, collect_slices_from_modifier,
    BlendMode, Brush, Color, CompositingStrategy, CornerRadii, DpOffset, EdgeInsets,
    FocusDirection, FocusRequester, GlassMaterial, GraphicsLayer, LayerShape, Modifier,
    ModifierNodeSlices, ModifierNodeSlicesDebugStats, Point, PointerEvent, PointerEventKind,
    PointerInputScope, PointerSource, Rect, RenderEffect, ResolvedBackground, ResolvedModifiers,
    RoundedCornerShape, RuntimeShader, Shadow, ShadowScope, Size, TransformOrigin,
};
pub use modifier_nodes::{
    AlphaElement, AlphaNode, BackgroundElement, BackgroundNode, ClickableElement, ClickableNode,
    CornerShapeElement, CornerShapeNode, FillDirection, FillElement, FillNode,
    FractionalOffsetElement, FractionalOffsetNode, OffsetElement, OffsetNode, PaddingElement,
    PaddingNode, SizeElement, SizeNode,
};
pub use pointer_dispatch::{
    clear_pointer_repasses, has_pending_pointer_repasses, process_pointer_repasses,
    schedule_pointer_repass,
};
pub use primitives::{
    fade_in, fade_out, remember_svg, slide_in_vertically, slide_out_vertically, AnimatedVisibility,
    BasicText, BasicTextField, BasicTextFieldOptions, BasicTextWithOptions, BitmapPainter, Box,
    BoxScope, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, BoxWithConstraintsScopeImpl,
    Button, ButtonSpec, Canvas, Column, ColumnSpec, ContentScale, Crossfade, EnterTransition,
    ExitTransition, ForEach, Image, Layout, LayoutNode, Painter, Row, RowSpec, Spacer,
    SubcomposeLayout, SvgPainter, SvgPainterError, Text, TextWithOptions, DEFAULT_ALPHA,
};
// Lazy list exports - single source from cranpose-foundation
pub use cranpose_foundation::lazy::{LazyListItemInfo, LazyListLayoutInfo, LazyListState};
pub use key_event::{KeyCode, KeyEvent, KeyEventType, Modifiers};
#[cfg(any(test, feature = "test-helpers"))]
#[doc(hidden)]
pub use render_state::reset_render_state_for_tests;
pub use render_state::{
    clear_transient_scroll_motion_contexts, current_density, debug_last_fling_velocity,
    debug_reset_last_fling_velocity, has_current_app_context, has_pending_draw_repasses,
    has_pending_layout_repasses, has_pending_measure_repasses, peek_focus_invalidation,
    peek_layout_invalidation, peek_pointer_invalidation, peek_render_invalidation,
    pending_layout_repass_nodes_snapshot, request_focus_invalidation, request_layout_invalidation,
    request_pointer_invalidation, request_render_invalidation, schedule_draw_repass,
    schedule_layout_repass, schedule_measure_repass, set_density, take_draw_repass_nodes,
    take_focus_invalidation, take_layout_invalidation, take_layout_repass_nodes,
    take_measure_repass_nodes, take_pointer_invalidation, take_render_invalidation, AppContext,
    AppContextScope,
};
pub use renderer::{HeadlessRenderer, PaintLayer, RecordedRenderScene, RenderOp};
pub use scroll::{ScrollElement, ScrollNode, ScrollSettlePolicy, ScrollState};
pub use zoom::ZoomState;
// Test utilities for fling velocity verification (only with test-helpers feature)
#[cfg(feature = "test-helpers")]
pub use modifier::{last_fling_velocity, reset_last_fling_velocity};
pub use subcompose_layout::{
    Constraints, MeasureResult, Placement, SubcomposeLayoutNode, SubcomposeLayoutScope,
    SubcomposeMeasureScope, SubcomposeMeasureScopeImpl,
};
pub use text::{
    get_cursor_x_for_offset, get_offset_for_position, layout_text, measure_text,
    measure_text_for_node, measure_text_with_options, measure_text_with_options_for_node,
    prepare_text_layout, prepare_text_layout_for_node, set_text_measurer, LinkAnnotation,
    ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    PreparedTextLayout, SpanStyle, StringAnnotation, TextDrawStyle, TextLayoutOptions,
    TextLayoutResult, TextLinePrefixWidths, TextMeasurer, TextMetrics, TextOptions, TextOverflow,
    TextShaping, TextStyle,
};
pub use text_field_modifier_node::{TextFieldElement, TextFieldModifierNode, TextPanResolver};
pub use text_modifier_node::{TextModifierElement, TextModifierNode};
pub use widgets::clickable_text::ClickableText;
pub use widgets::lazy_list::{LazyColumn, LazyColumnSpec, LazyRow, LazyRowSpec};
pub use widgets::linked_text::LinkedText;
pub use widgets::swipe_to_dismiss::{SwipeDismissSide, SwipeToDismiss, SwipeToDismissSpec};

// Debug utilities
pub use debug::{
    format_layout_tree, format_modifier_chain, format_render_scene, format_screen_summary,
    install_modifier_chain_trace, log_layout_tree, log_modifier_chain, log_render_scene,
    log_screen_summary, ModifierChainTraceGuard,
};

/// In-memory composition helper used by tests.
pub struct TestComposition {
    _scope: render_state::AppContextScope,
    app_context: Rc<AppContext>,
    composition: Composition<MemoryApplier>,
}

impl TestComposition {
    pub fn root(&self) -> Option<NodeId> {
        self.app_context.enter(|| self.composition.root())
    }

    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.app_context.enter(|| self.composition.runtime_handle())
    }

    pub fn should_render(&self) -> bool {
        self.app_context.enter(|| self.composition.should_render())
    }

    pub fn take_root_render_request(&mut self) -> bool {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.composition.take_root_render_request())
    }

    pub fn flush_pending_node_updates(&mut self) -> Result<(), NodeError> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.composition.flush_pending_node_updates())
    }

    pub fn process_invalid_scopes(&mut self) -> Result<bool, NodeError> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.composition.process_invalid_scopes())
    }

    pub fn render(&mut self, root_key: Key, content: impl FnMut()) -> Result<(), NodeError> {
        let app_context = Rc::clone(&self.app_context);
        app_context.enter(|| self.composition.render(root_key, content))
    }

    pub fn applier_mut(&mut self) -> TestApplierGuard<'_> {
        let scope = self.app_context.enter_scope();
        let applier = self.composition.applier_mut();
        TestApplierGuard {
            _scope: scope,
            applier,
        }
    }

    pub fn with_app_context<R>(&self, block: impl FnOnce() -> R) -> R {
        self.app_context.enter(block)
    }
}

pub struct TestApplierGuard<'a> {
    _scope: render_state::AppContextScope,
    applier: ApplierGuard<'a, MemoryApplier>,
}

impl Deref for TestApplierGuard<'_> {
    type Target = MemoryApplier;

    fn deref(&self) -> &Self::Target {
        &self.applier
    }
}

impl DerefMut for TestApplierGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.applier
    }
}

/// Build a composition with a simple in-memory applier and run the provided closure once.
pub fn run_test_composition(build: impl FnMut()) -> TestComposition {
    let app_context = AppContext::new();
    app_context.enter(|| {
        #[cfg(test)]
        reset_render_state_for_tests();
    });
    let mut test_composition = TestComposition {
        _scope: app_context.enter_scope(),
        app_context,
        composition: Composition::new(MemoryApplier::new()),
    };
    test_composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
    test_composition
}

pub use cranpose_core::MutableState as SnapshotState;

#[cfg(test)]
#[path = "tests/anchor_async_tests.rs"]
mod anchor_async_tests;

#[cfg(test)]
#[path = "tests/animated_visibility_tests.rs"]
mod animated_visibility_tests;

#[cfg(test)]
#[path = "tests/crossfade_tests.rs"]
mod crossfade_tests;

#[cfg(test)]
#[path = "tests/async_runtime_full_layout_test.rs"]
mod async_runtime_full_layout_test;

#[cfg(test)]
#[path = "tests/cursor_position_tests.rs"]
mod cursor_position_tests;

#[cfg(test)]
#[path = "tests/popup_tests.rs"]
mod popup_tests;

#[cfg(test)]
#[path = "tests/selection_handle_tests.rs"]
mod selection_handle_tests;

#[cfg(test)]
#[path = "tests/tab_switching_tests.rs"]
mod tab_switching_tests;

#[cfg(test)]
#[path = "tests/lazy_list_viewport_tests.rs"]
mod lazy_list_viewport_tests;

#[cfg(test)]
#[path = "tests/swipe_to_dismiss_lazy_tests.rs"]
mod swipe_to_dismiss_lazy_tests;

#[cfg(test)]
#[path = "tests/swipe_to_dismiss_render_tests.rs"]
mod swipe_to_dismiss_render_tests;

#[cfg(test)]
#[path = "tests/lazy_list_recompose_tests.rs"]
mod lazy_list_recompose_tests;
