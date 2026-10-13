use std::{
    cell::{Cell, OnceCell, RefCell},
    num::NonZeroU64,
    rc::{Rc, Weak},
    sync::Arc,
};

use cranpose_core::{
    NodeId,
    collections::{FixedHashMap, fixed_hash_map},
};
use cranpose_ui_graphics::DrawTextMeasurer;
use cranpose_ui_layout::MeasurePolicy;

use crate::{
    announce::Announcement,
    environment_locals::EnvironmentLocals,
    modifier::{Modifier, ModifierNodeSlices},
    render_state::{AppContext, AppContextId},
    selection_container::RegistrarInner,
    text::{TextStyle, annotated_string::SharedPlainText},
    unhandled_keys::KeyHandler,
    widgets::nodes::layout_node::PlacementPass,
};

pub(crate) struct ThreadCells {
    pub(crate) current_draw_node: Cell<Option<NodeId>>,
    pub(crate) next_app_context_id: Cell<NonZeroU64>,
    pub(crate) keyboard_focus_visible: Cell<bool>,
    pub(crate) clean_slot_skips: Cell<u64>,
    pub(crate) last_modifier_revision: Cell<u64>,
    pub(crate) armed_semantics_layout_logs: Cell<usize>,
    pub(crate) modal_counting_contexts: Cell<usize>,
    pub(crate) placement_pass: PlacementPass,
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
            last_modifier_revision: Cell::new(0),
            armed_semantics_layout_logs: Cell::new(0),
            modal_counting_contexts: Cell::new(0),
            placement_pass: PlacementPass::new(),
            last_unhandled_key_handler_id: Cell::new(0),
            idle_prefetch_passes: Cell::new(false),
        }
    }
}

pub(crate) struct UiThreadState {
    pub(crate) environment: EnvironmentLocals,
    pub(crate) app_context_stack: RefCell<Vec<Weak<AppContext>>>,
    pub(crate) app_contexts: RefCell<FixedHashMap<AppContextId, Weak<AppContext>>>,
    pub(crate) active_selection: RefCell<Weak<RegistrarInner>>,
    pub(crate) recent_text_styles: RefCell<Vec<Arc<TextStyle>>>,
    pub(crate) shared_plain_texts: RefCell<FixedHashMap<u64, SharedPlainText>>,
    pub(crate) pending_announcements: RefCell<Vec<Announcement>>,
    pub(crate) unhandled_key_handlers: RefCell<Vec<(u64, KeyHandler)>>,
    pub(crate) fill_width: OnceCell<Modifier>,
    pub(crate) fill_height: OnceCell<Modifier>,
    pub(crate) fill_size: OnceCell<Modifier>,
    pub(crate) clip_to_bounds: OnceCell<Modifier>,
    pub(crate) empty_slices: OnceCell<Rc<ModifierNodeSlices>>,
    pub(crate) empty_measure_policy: OnceCell<Rc<dyn MeasurePolicy>>,
    pub(crate) draw_text_measurer: OnceCell<Rc<dyn DrawTextMeasurer>>,
}

impl UiThreadState {
    const fn new() -> Self {
        Self {
            environment: EnvironmentLocals::new(),
            app_context_stack: RefCell::new(Vec::new()),
            app_contexts: RefCell::new(fixed_hash_map()),
            active_selection: RefCell::new(Weak::new()),
            recent_text_styles: RefCell::new(Vec::new()),
            shared_plain_texts: RefCell::new(fixed_hash_map()),
            pending_announcements: RefCell::new(Vec::new()),
            unhandled_key_handlers: RefCell::new(Vec::new()),
            fill_width: OnceCell::new(),
            fill_height: OnceCell::new(),
            fill_size: OnceCell::new(),
            clip_to_bounds: OnceCell::new(),
            empty_slices: OnceCell::new(),
            empty_measure_policy: OnceCell::new(),
            draw_text_measurer: OnceCell::new(),
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
    STATE.with(f)
}

pub(crate) fn try_with<R>(f: impl FnOnce(&UiThreadState) -> R) -> Option<R> {
    STATE.try_with(f).ok()
}

pub(crate) fn with_environment<R>(f: impl FnOnce(&EnvironmentLocals) -> R) -> R {
    STATE.with(|state| f(&state.environment))
}
