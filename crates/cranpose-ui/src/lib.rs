#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

extern crate self as cranpose_ui;

use std::{
    ops::{Deref, DerefMut},
    rc::Rc,
};

pub use cranpose_core::Composition;
use cranpose_core::{ApplierGuard, MemoryApplier, NodeError, NodeId, RuntimeHandle, location_key};
pub use cranpose_macros::composable;
#[cfg(feature = "localization")]
pub use cranpose_macros::{message, tr, translation_messages, translations};
#[cfg(feature = "localization")]
pub use localization::{
    Catalog, Language, LanguagePreference, Locale, LocalizedText, PreviewMode, ProvideLocalization,
    ProvideTranslator,
};
pub use text::SharedText;
#[cfg(feature = "localization")]
mod ui_text;
#[cfg(feature = "localization")]
pub use ui_text::UiText;
#[cfg(feature = "localization")]
mod language_preferences;
mod memory_headroom;
#[cfg(feature = "localization")]
pub use language_preferences::{
    LocalizationController, ProvideLanguagePreferences, local_localization,
};
pub use ui_strings::UiString;

/// Screen reader announcements for events that have no control on screen.
pub mod announce;
mod arrow_focus;
pub mod bring_into_view;
mod bring_into_view_spec;
pub mod clipboard_session;
mod configuration;
mod cursor_animation;
mod debug;
pub mod density;
mod dirty_node_queue;
pub mod draggable;
mod draw;
mod environment_locals;
mod eye;
pub mod fling_animation;
mod focus_dispatch;
/// Moving focus between focus targets, as `LocalFocusManager` does in Compose.
pub mod focus_manager;
mod focus_navigation;
/// The ordered list of focus targets that layout publishes for keyboard traversal.
pub mod focus_order;
mod focus_within;
pub mod font_scale;
mod interaction;
mod key_event;
mod keyboard_options;
/// Layout tree, measurement and placement of nodes.
pub mod layout;
pub mod layout_direction;
pub mod lazy_item;
mod lazy_prefetch;
#[cfg(feature = "localization")]
pub mod localization;
pub mod modal;
mod modal_nodes;
#[expect(clippy::multiple_inherent_impl, reason = "one file per modifier")]
mod modifier;
mod modifier_nodes;
pub mod mouse_input;
mod nested_scroll;
pub mod nine_patch;
mod pointer_dispatch;
pub mod pointer_icon_session;
mod primitives;
mod render_state;
mod renderer;
pub mod round_scaling_list;
pub mod round_scroll_indicator;
pub mod safe_area;
pub mod scroll;
mod scroll_semantics;
pub mod scrollbar;
pub mod selection_container;
mod semantics_dispatch;
mod semantics_layout_log;
mod subcompose_layout;
/// Text styling, measurement and layout.
pub mod text;
mod text_field_decorator_node;
pub mod text_field_focus;
mod text_field_handler;
mod text_field_input;
mod text_field_modifier_node;
mod text_field_presentation;
pub mod text_input_session;
pub mod text_layout_result;
mod text_modifier_node;
pub mod text_selection;
mod thread_state;
mod two_dimensional_focus_search;
mod ui_strings;
mod unhandled_keys;
pub mod widgets;
mod word_boundaries;
pub mod zoom;

pub use announce::{Announcement, Announcer, announce, drain_announcements, local_announcer};
#[doc(hidden)]
pub use arrow_focus::arrow_focus_target;
pub use bring_into_view::{
    BringIntoViewResponder, local_bring_into_view_responder, scroll_delta_to_reveal,
};
pub use bring_into_view_spec::local_bring_into_view_spec;
pub use configuration::{Configuration, UiModeType, local_configuration};
pub use cranpose_foundation::{
    BringIntoViewSpec, CanvasSemanticsNode, CollectionInfo, DelegatableNode, LiveRegionMode,
    ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState, PointerId,
    ProgressBarRangeInfo, ScrollAxisRange, SemanticsConfiguration, SemanticsCustomAction,
    SemanticsDismiss, SemanticsExpand, SemanticsLongClick, SemanticsMagicTap, SemanticsScrollBy,
    SemanticsScrollToIndex, SemanticsSetProgress, SemanticsSetSelection, SemanticsSetText,
    SemanticsSpec, SemanticsWidgetRole, ToggleableState,
    lazy::{
        LazyItems, LazyListItemInfo, LazyListLayoutInfo, LazyListScope, LazyListState,
        rememberLazyListState, rememberLazyListStateWithPosition,
    },
    text::{TextFieldBuffer, TextFieldLineLimits, TextFieldState, TextFieldValue, TextRange},
};
/// Composables in 3D: meshes, cameras and scenes for
/// [`Modifier::scene3d`] and [`Modifier::mesh_surface`].
pub use cranpose_ui_graphics::space;
pub use cranpose_ui_graphics::{
    BlurredEdgeTreatment, ColorFilter, CursorIcon, CustomPointerIcon, DashPathEffect, Dp,
    DrawScope, DrawSpecialization, DrawStyle, DrawTextStyle, DrawTransform, ExternalTexture,
    ImageBitmap, ImageBitmapError, ImagePixelFormat, ImageSampling, Path, PlaceholderShape,
    PointerIcon, PointerIconError, RuntimeShaderUniformError, ShaderPlaceholder, Stroke, StrokeCap,
    StrokeJoin, SubstrateSpec, TextMeasurement, TextVerticalAlign, TileMode, WindowCoordinates,
    glass_material_folds_enabled, set_glass_material_folds,
};
pub use cranpose_ui_layout::{AlignmentLines, Axis, IntrinsicSize, Measurement};
pub use cursor_animation::{
    is_cursor_visible, next_cursor_blink_time, reset_cursor_blink, start_cursor_blink,
    stop_cursor_blink, tick_cursor_blink,
};
pub use debug::{
    ModifierChainTraceGuard, format_layout_tree, format_modifier_chain, format_render_scene,
    format_screen_summary, install_modifier_chain_trace, log_layout_tree, log_modifier_chain,
    log_render_scene, log_screen_summary,
};
pub use density::{Density, DensityMeasureScope, density, local_density};
pub use draggable::{
    DragDeltaHandler, DraggableState, ScrollableState, rememberDraggableState,
    rememberScrollableState,
};
pub use draw::{
    DrawCacheBuilder, DrawCommand, DrawCommandFn, command_draw_scope, execute_draw_commands,
};
pub use eye::{Eye, local_eye, rememberEye};
pub use focus_dispatch::{
    active_focus_target, clear_focus_invalidations, has_pending_focus_invalidations,
    keyboard_focus_visible, process_focus_invalidations, schedule_focus_invalidation,
    set_active_focus_target, set_keyboard_focus_visible,
};
pub use focus_manager::{
    FocusManager, local_focus_manager, request_focus_from_platform, request_focus_without_keyboard,
};
pub use focus_navigation::{KeyboardFocusTarget, keyboard_focus_target};
pub use focus_order::{
    FocusEntry, collect_focus_order, collect_focus_order_under, focus_order_len, set_focus_order,
    take_requested_focus_move,
};
pub use focus_within::FocusWithin;
pub use font_scale::{FontScaleCurve, MAX_FONT_SCALE_KNOTS};
pub use interaction::{
    HoverInteraction, HoverInteractionEnter, HoverInteractionExit, Interaction,
    MutableInteractionSource, PressInteraction, PressInteractionCancel, PressInteractionPress,
    PressInteractionRelease, collect_is_hovered_as_state, collect_is_pressed_as_state,
    rememberMutableInteractionSource,
};
pub use key_event::{Key, KeyEvent, KeyEventType, Modifiers, NativeKeyEvent};
pub use keyboard_options::{
    ContentType, ImeAction, KeyboardActionHandler, KeyboardActionScope, KeyboardCapitalization,
    KeyboardOptions, KeyboardType,
};
pub use layout::{
    LayoutAllocationDebugStats, LayoutBox, LayoutEngine, LayoutMeasurements, LayoutNodeData,
    LayoutNodeKind, LayoutTree, MeasureLayoutOptions, SemanticsAction, SemanticsCallback,
    SemanticsDetails, SemanticsNode, SemanticsPlacement, SemanticsRole, SemanticsText,
    SemanticsTree, build_layout_tree_from_applier, build_semantics_tree_from_applier,
    build_semantics_tree_from_layout_tree,
    core::{
        Alignment, Arrangement, HorizontalAlignment, LayoutData, LinearArrangement, Measurable,
        ParentData, Placeable, VerticalAlignment,
    },
    has_placed_content, measure_layout, measure_layout_with_options, placed_content_extent,
    top_modal_from_applier, tree_needs_layout, tree_needs_semantics,
    update_semantics_tree_from_applier,
};
pub use layout_direction::{
    LayoutDirection, ProvideLayoutDirection, layout_direction, local_layout_direction,
};
pub use lazy_item::{ProvideLazyItemKey, lazy_item_key, local_lazy_item_key};
pub use lazy_prefetch::{
    drain_lazy_prefetch_requests, has_lazy_prefetch_requests, lazy_prefetch_pass_cost,
    set_idle_prefetch_passes, warm_prefetched_slices, with_lazy_prefetch_pass,
};
pub use memory_headroom::{MemoryHeadroom, current_memory_headroom, set_memory_headroom};
pub use modal::{
    ModalRegistration, clear_modals, dispatch_modal_back, local_modal_depth, modal_depth,
};
pub use modifier::{
    BlendMode, Brush, Color, CompositingStrategy, CornerRadii, DpOffset, DragAndDropEvent,
    DragAndDropOutcome, DragAndDropPayload, DragAndDropPoint, DragAndDropSource,
    DragAndDropSourceElement, DragAndDropSourceNode, DragAndDropState, DragAndDropTarget,
    DragAndDropTargetElement, DragAndDropTargetNode, EdgeInsets, FocusDirection, FocusRequestError,
    FocusRequester, GlassMaterial, GraphicsLayer, KeyEventRoute, LayerShape, Modifier,
    ModifierLocalKey, ModifierLocalReadScope, ModifierNodeSlices, ModifierNodeSlicesDebugStats,
    Point, PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerInputScope,
    PointerSource, Rect, RenderEffect, ResolvedBackground, ResolvedModifiers,
    RotaryInputModifierNode, RotaryScrollEvent, RoundedCornerShape, RuntimeShader,
    SemanticsRequester, Shadow, ShadowScope, Size, SliceTextRef, TransformOrigin,
    WindowRootDescriptor, WindowRootElement, WindowRootEntry, WindowRootNode, WindowRootRegistry,
    WindowRootRoutingScratch, collect_modifier_slices, collect_semantics_from_modifier,
    collect_slices_from_modifier, is_window_root, nearest_window_root, nearest_window_roots,
    nearest_window_roots_into, window_roots, window_roots_revision, z_index_in_use,
};
#[cfg(feature = "test-helpers")]
pub use modifier::{last_fling_velocity, reset_last_fling_velocity};
pub use modifier_nodes::{
    AlphaElement, AlphaNode, BackgroundElement, BackgroundNode, ClickableElement, ClickableNode,
    CornerShapeElement, CornerShapeNode, FillDirection, FillElement, FillNode,
    FractionalOffsetElement, FractionalOffsetNode, OffsetElement, OffsetNode, PaddingElement,
    PaddingNode, SizeElement, SizeNode,
};
pub use nested_scroll::{NestedScroll, NestedScrollConnection, NestedScrollSource};
pub use nine_patch::{
    NinePatchInsets, PatchFill, PatchQuad, nine_patch_quads, tile_count, tile_quads,
};
#[doc(hidden)]
pub use pointer_dispatch::{
    clear_pointer_repasses, has_pending_pointer_repasses, process_pointer_repasses,
    schedule_pointer_repass,
};
pub use primitives::{
    AnimateScroll, AnimatedContent, AnimatedContentWithKey, AnimatedVisibility,
    BasicSecureTextField, BasicText, BasicTextField, BasicTextFieldDecorated,
    BasicTextFieldDecorationScope, BasicTextFieldOptions, BasicTextFieldWithOptions,
    BasicTextWithOptions, BitmapPainter, BitmapRegionPainter, Box, BoxScope, BoxSpec,
    BoxWithConstraints, BoxWithConstraintsScope, BoxWithConstraintsScopeImpl, Button, ButtonSpec,
    Canvas, Column, ColumnSpec, ContentScale, ContentSeek, ContentTransform, Crossfade,
    DEFAULT_ALPHA, EnterTransition, ExitTransition, ForEach, HorizontalPager, Image, Layout,
    LayoutNode, PageSize, PagerFling, PagerSpec, PagerState, Painter, Row, RowSpec,
    SecureTextFieldOptions, SeekableAnimatedContent, SeekableCrossfade, SharedBounds,
    SharedElement, SharedTransitionLayout, SizeTransform, SlideDirection, SnapPosition, Spacer,
    SubcomposeLayout, SvgPainter, SvgPainterError, Text, TextObfuscationMode, TextWithOptions,
    VerticalPager, fade_in, fade_out, rememberPagerState, rememberSvg, scale_in, scale_out,
    slide_in_horizontally, slide_in_vertically, slide_into_container, slide_out_horizontally,
    slide_out_of_container, slide_out_vertically,
};
#[cfg(any(test, feature = "test-helpers"))]
#[doc(hidden)]
pub use render_state::reset_render_state_for_tests;
pub use render_state::{
    AppContext, AppContextScope, MAX_FONT_SCALE, MIN_FONT_SCALE, current_density,
    current_font_scale, current_font_scale_curve, scale_sp, set_density, set_font_scale,
    set_font_scale_curve,
};
#[doc(hidden)]
pub use render_state::{
    GeometrySceneNodes, MovedSceneNode, append_z_order_changes,
    clear_transient_scroll_motion_contexts, debug_last_fling_velocity,
    debug_reset_last_fling_velocity, has_current_app_context, has_pending_draw_repasses,
    has_pending_layer_property_repasses, has_pending_layout_repasses, has_pending_measure_repasses,
    peek_focus_invalidation, peek_layout_invalidation, peek_pointer_invalidation,
    peek_render_invalidation, pending_layout_repass_nodes_snapshot,
    pending_measure_repass_nodes_snapshot, prune_draw_observations_to_nodes,
    request_current_draw_redraw, request_focus_invalidation, request_layout_invalidation,
    request_pointer_invalidation, request_render_invalidation, schedule_draw_repass,
    schedule_layout_repass, schedule_measure_repass, schedule_z_order_change,
    take_draw_repass_nodes, take_draw_repass_nodes_into, take_focus_invalidation,
    take_geometry_scene_nodes, take_geometry_scene_nodes_into,
    take_layer_property_repass_nodes_into, take_layout_invalidation, take_layout_repass_nodes,
    take_measure_repass_nodes, take_pointer_invalidation, take_render_invalidation,
};
pub use renderer::{HeadlessRenderer, PaintLayer, RecordedRenderScene, RenderOp};
pub use safe_area::{WindowInsets, local_ime_insets, local_safe_area_insets, window_insets};
pub use scroll::{ScrollElement, ScrollMetrics, ScrollNode, ScrollSettlePolicy, ScrollState};
pub use scrollbar::{ThumbBounds, ThumbGeometry, content_delta_for_thumb_drag, thumb_geometry};
#[doc(hidden)]
pub use semantics_dispatch::{
    clear_semantics_invalidations, has_pending_semantics_invalidations,
    process_semantics_invalidations, schedule_semantics_invalidation,
};
pub use subcompose_layout::{
    Constraints, MeasureResult, Placement, SubcomposeLayoutNode, SubcomposeLayoutScope,
    SubcomposeMeasureScope, SubcomposeMeasureScopeImpl, clean_slot_skip_count,
    retained_measure_count,
};
pub use text::{
    LinkAnnotation, ParagraphStyle, PlatformParagraphStyle, PlatformSpanStyle, PlatformTextStyle,
    PreparedTextLayout, SpanStyle, StringAnnotation, TextDrawStyle, TextLayoutOptions,
    TextLayoutResult, TextLinePrefixWidths, TextMeasurer, TextMetrics, TextOptions, TextOverflow,
    TextShaping, TextStyle, get_cursor_x_for_offset, get_offset_for_position, layout_text,
    measure_text, measure_text_for_node, measure_text_with_options,
    measure_text_with_options_for_node, prepare_text_layout, prepare_text_layout_for_node,
    set_text_measurer,
};
pub use text_field_focus::{ImeEditorState, has_focused_field};
pub use text_field_modifier_node::{TextFieldElement, TextFieldModifierNode, TextPanResolver};
pub use text_input_session::PlatformTextInputHandler;
pub use text_modifier_node::{TextModifierElement, TextModifierNode};
pub use unhandled_keys::{UnhandledKeyEvents, dispatch_unhandled_key_event};
pub use widgets::{
    clickable_text::ClickableText,
    lazy_list::{
        GridCells, LazyColumn, LazyColumnSpec, LazyGridState, LazyHorizontalGrid,
        LazyHorizontalGridSpec, LazyHorizontalStaggeredGrid, LazyHorizontalStaggeredGridSpec,
        LazyRow, LazyRowSpec, LazyStaggeredGridState, LazyVerticalGrid, LazyVerticalGridSpec,
        LazyVerticalStaggeredGrid, LazyVerticalStaggeredGridSpec, rememberLazyGridState,
        rememberLazyStaggeredGridState,
    },
    linked_text::LinkedText,
    padding_values::PaddingValues,
    popup::{dismiss_top_popup, dismissable_popup_open},
    slider::{Slider, SliderOrientation, SliderScope, SliderSpec},
    swipe_to_dismiss::{
        SwipeDismissDirection, SwipeDismissSide, SwipeDismissState, SwipeToDismiss,
        SwipeToDismissBox, SwipeToDismissSpec, rememberSwipeDismissState,
    },
    text_selection_menu::local_on_light_surface,
};
pub use zoom::ZoomState;

/// In-memory composition helper used by tests.
pub struct TestComposition {
    _scope: AppContextScope,
    app_context: Rc<AppContext>,
    composition: Composition<MemoryApplier>,
    /// The root content and its key, which a root render composes again, as
    /// a host's reconcile does.
    root_content: Option<(cranpose_core::Key, Box<dyn FnMut()>)>,
}

impl TestComposition {
    /// Returns the id of the root node, or `None` when the composition has no root node.
    pub fn root(&self) -> Option<NodeId> {
        self.app_context.enter(|| self.composition.root())
    }

    /// Returns the runtime handle of the composition.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.app_context.enter(|| self.composition.runtime_handle())
    }

    /// Returns true when state changes or animation callbacks have work pending.
    pub fn should_render(&self) -> bool {
        self.app_context.enter(|| self.composition.should_render())
    }

    /// Returns true once if a root render was requested since the last call, then resets the
    /// request.
    pub fn take_root_render_request(&mut self) -> bool {
        self.app_context
            .enter(|| self.composition.take_root_render_request())
    }

    /// Applies the node updates that state changes queued.
    pub fn flush_pending_node_updates(&mut self) -> Result<(), NodeError> {
        self.app_context
            .enter(|| self.composition.flush_pending_node_updates())
    }

    /// Recomposes the scopes that state changes invalidated, and composes the root content again
    /// when a read with no composable around it asked for that, as a host does. Returns true
    /// when anything recomposed.
    pub fn process_invalid_scopes(&mut self) -> Result<bool, NodeError> {
        self.app_context.enter(|| match &mut self.root_content {
            Some((key, content)) => self.composition.reconcile(*key, content),
            None => self.composition.process_invalid_scopes(),
        })
    }

    /// Renders `content` as the root composition under `root_key`, then recomposes invalidated
    /// scopes. Later recompositions compose `content` again when the root needs it.
    pub fn render(
        &mut self,
        root_key: cranpose_core::Key,
        content: impl FnMut() + 'static,
    ) -> Result<(), NodeError> {
        let (key, content) = self.root_content.insert((root_key, Box::new(content)));
        self.app_context
            .enter(|| self.composition.render(*key, content))
    }

    /// Measures and places the tree under `root` within `max_size`, as a
    /// host's layout pass does.
    pub fn measure_layout(
        &mut self,
        root: NodeId,
        max_size: Size,
    ) -> Result<LayoutMeasurements, NodeError> {
        self.with_runtime_applier(|applier| measure_layout(applier, root, max_size))
    }

    /// Runs `with` on the applier while it holds the composition's runtime
    /// handle, as a host's layout pass does, and takes the handle back after.
    pub fn with_runtime_applier<R>(&mut self, with: impl FnOnce(&mut MemoryApplier) -> R) -> R {
        let handle = self.runtime_handle();
        let mut applier = self.applier_mut();
        applier.set_runtime_handle(handle);
        let result = with(&mut applier);
        applier.clear_runtime_handle();
        result
    }

    /// Borrows the applier that holds the node tree. The guard keeps this composition's app context
    /// active.
    pub fn applier_mut(&mut self) -> TestApplierGuard<'_> {
        let scope = self.app_context.enter_scope();
        let applier = self.composition.applier_mut();
        TestApplierGuard {
            _scope: scope,
            applier,
        }
    }

    /// Runs `block` with this composition's app context active.
    pub fn with_app_context<R>(&self, block: impl FnOnce() -> R) -> R {
        self.app_context.enter(block)
    }
}

/// Mutable access to the applier of a [`TestComposition`], with the app context of that composition
/// active while the guard lives.
pub struct TestApplierGuard<'a> {
    _scope: AppContextScope,
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
pub fn run_test_composition(build: impl FnMut() + 'static) -> TestComposition {
    let app_context = AppContext::new();
    app_context.enter(|| {
        #[cfg(test)]
        reset_render_state_for_tests();
    });
    let mut test_composition = TestComposition {
        _scope: app_context.enter_scope(),
        app_context,
        composition: Composition::new(MemoryApplier::new()),
        root_content: None,
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
#[path = "tests/lazy_recycle_effect_tests.rs"]
mod lazy_recycle_effect_tests;

#[cfg(test)]
#[path = "tests/measure_time_ui_task_tests.rs"]
mod measure_time_ui_task_tests;

#[cfg(test)]
#[path = "tests/animation_frame_pump_tests.rs"]
mod animation_frame_pump_tests;

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
#[path = "tests/layout_keep_tests.rs"]
mod layout_keep_tests;

#[cfg(test)]
#[path = "tests/lazy_list_viewport_tests.rs"]
mod lazy_list_viewport_tests;

#[cfg(test)]
#[path = "tests/swipe_to_dismiss_lazy_tests.rs"]
mod swipe_to_dismiss_lazy_tests;

#[cfg(test)]
#[path = "tests/top_modal_tests.rs"]
mod top_modal_tests;

#[cfg(test)]
#[path = "tests/swipe_to_dismiss_render_tests.rs"]
mod swipe_to_dismiss_render_tests;

#[cfg(test)]
#[path = "tests/lazy_list_recompose_tests.rs"]
mod lazy_list_recompose_tests;

#[cfg(test)]
#[path = "tests/lazy_row_tests.rs"]
mod lazy_row_tests;

#[cfg(test)]
#[path = "tests/wear_widget_tests.rs"]
mod wear_widget_tests;

#[cfg(test)]
#[path = "tests/ios_fling_measurement.rs"]
mod ios_fling_measurement;
