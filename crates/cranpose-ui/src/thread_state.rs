use std::{
    cell::{Cell, OnceCell, RefCell},
    num::NonZeroU64,
    rc::{Rc, Weak},
};

use cranpose_core::{
    NodeId,
    collections::{FixedHashMap, fixed_hash_map},
};
use cranpose_ui_graphics::{DrawTextMeasurer, VectorPath};
use cranpose_ui_layout::MeasurePolicy;

use crate::{
    announce::Announcement,
    environment_locals::EnvironmentLocals,
    layout::policies::SpareFlexChildren,
    modifier::{Modifier, ModifierNodeSlices},
    render_state::{AppContext, AppContextId},
    selection_container::RegistrarInner,
    text::{draw_scope_text::DrawnTextStyles, shared_plain_text::SharedPlainText},
    text_modifier_node::RecentTextStyles,
    unhandled_keys::KeyHandler,
};

// Each static path string with its parse, `None` when the data does not parse.
type IconPaths = Vec<(&'static str, Option<Rc<VectorPath>>)>;

pub(crate) struct ThreadCells {
    pub(crate) current_draw_node: Cell<Option<NodeId>>,
    pub(crate) next_app_context_id: Cell<NonZeroU64>,
    pub(crate) keyboard_focus_visible: Cell<bool>,
    pub(crate) clean_slot_skips: Cell<u64>,
    pub(crate) retained_measures: Cell<u64>,
    /// The last take of a moved-node list on this thread
    /// ([`crate::render_state`]'s `MovedSceneNodes`).
    pub(crate) moved_scene_node_takes: Cell<u64>,
    pub(crate) last_modifier_revision: Cell<u64>,
    pub(crate) modal_counting_contexts: Cell<usize>,
    /// How many `z_index` modifier nodes are attached on this thread.
    pub(crate) z_index_nodes: Cell<usize>,
    pub(crate) last_unhandled_key_handler_id: Cell<u64>,
    /// Whether the shell on this thread runs idle prefetch passes
    /// ([`crate::set_idle_prefetch_passes`]).
    pub(crate) idle_prefetch_passes: Cell<bool>,
}

const _: () = assert!(!std::mem::needs_drop::<ThreadCells>());

impl ThreadCells {
    const fn new() -> Self {
        Self {
            current_draw_node: Cell::new(None),
            next_app_context_id: Cell::new(NonZeroU64::MIN),
            keyboard_focus_visible: Cell::new(false),
            clean_slot_skips: Cell::new(0),
            retained_measures: Cell::new(0),
            moved_scene_node_takes: Cell::new(0),
            last_modifier_revision: Cell::new(0),
            modal_counting_contexts: Cell::new(0),
            z_index_nodes: Cell::new(0),
            last_unhandled_key_handler_id: Cell::new(0),
            idle_prefetch_passes: Cell::new(false),
        }
    }
}

pub(crate) struct UiThreadState {
    pub(crate) environment: EnvironmentLocals,
    /// While a focus move runs, the focus observers it left with no focused
    /// target; `None` outside a move.
    pub(crate) left_in_focus_move:
        RefCell<Option<Vec<Rc<crate::focus_within::FocusWithinObserver>>>>,
    pub(crate) app_context_stack: RefCell<Vec<Weak<AppContext>>>,
    pub(crate) app_contexts: RefCell<FixedHashMap<AppContextId, Weak<AppContext>>>,
    pub(crate) active_selection: RefCell<Weak<RegistrarInner>>,
    pub(crate) recent_text_styles: RefCell<RecentTextStyles>,
    pub(crate) drawn_text_styles: RefCell<DrawnTextStyles>,
    pub(crate) shared_plain_texts: RefCell<cranpose_ui_graphics::ContentPool<SharedPlainText>>,
    // Each icon's path data parsed once, for every icon that draws it.
    pub(crate) icon_paths: RefCell<IconPaths>,
    pub(crate) pending_announcements: RefCell<Vec<Announcement>>,
    pub(crate) unhandled_key_handlers: RefCell<Vec<(u64, KeyHandler)>>,
    pub(crate) fill_width: OnceCell<Modifier>,
    pub(crate) fill_height: OnceCell<Modifier>,
    pub(crate) fill_size: OnceCell<Modifier>,
    pub(crate) clip_to_bounds: OnceCell<Modifier>,
    pub(crate) empty_slices: OnceCell<Rc<ModifierNodeSlices>>,
    // Where a node with no slices of its own collects its chain, to learn
    // whether the chain gives any.
    pub(crate) spare_slices: Cell<Option<ModifierNodeSlices>>,
    pub(crate) empty_measure_policy: OnceCell<Rc<dyn MeasurePolicy>>,
    pub(crate) draw_text_measurer: OnceCell<Rc<dyn DrawTextMeasurer>>,
    pub(crate) spare_flex_children: SpareFlexChildren,
}

impl UiThreadState {
    const fn new() -> Self {
        Self {
            environment: EnvironmentLocals::new(),
            left_in_focus_move: RefCell::new(None),
            app_context_stack: RefCell::new(Vec::new()),
            app_contexts: RefCell::new(fixed_hash_map()),
            active_selection: RefCell::new(Weak::new()),
            recent_text_styles: RefCell::new(RecentTextStyles::new()),
            drawn_text_styles: RefCell::new(DrawnTextStyles::new()),
            shared_plain_texts: RefCell::new(cranpose_ui_graphics::ContentPool::new()),
            icon_paths: RefCell::new(Vec::new()),
            pending_announcements: RefCell::new(Vec::new()),
            unhandled_key_handlers: RefCell::new(Vec::new()),
            fill_width: OnceCell::new(),
            fill_height: OnceCell::new(),
            fill_size: OnceCell::new(),
            clip_to_bounds: OnceCell::new(),
            empty_slices: OnceCell::new(),
            spare_slices: Cell::new(None),
            empty_measure_policy: OnceCell::new(),
            draw_text_measurer: OnceCell::new(),
            spare_flex_children: SpareFlexChildren::new(),
        }
    }
}

thread_local! {
    static CELLS: ThreadCells = const { ThreadCells::new() };
    static STATE: UiThreadState = const { UiThreadState::new() };
}

pub(crate) fn cells<R>(f: impl FnOnce(&ThreadCells) -> R) -> R {
    CELLS.with(f)
}

pub(crate) fn with<R>(f: impl FnOnce(&UiThreadState) -> R) -> R {
    cranpose_core::with_or_fresh(&STATE, UiThreadState::new, f)
}

pub(crate) fn try_with<R>(f: impl FnOnce(&UiThreadState) -> R) -> Option<R> {
    STATE.try_with(f).ok()
}

pub(crate) fn with_environment<R>(f: impl FnOnce(&EnvironmentLocals) -> R) -> R {
    with(|state| f(&state.environment))
}
