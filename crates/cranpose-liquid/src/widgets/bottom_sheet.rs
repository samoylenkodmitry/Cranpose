//! A modal sheet that slides up from the bottom of the window, as Compose's
//! `ModalBottomSheet`.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationSpec, AnimationType, Easing};
use cranpose_core::{
    DisposableEffect, MutableState, RuntimeHandle, internal::FrameCallbackRegistration, remember,
    with_current_composer,
};
use cranpose_foundation::SemanticsExpand;
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, GraphicsLayer, HorizontalAlignment, Modifier, NestedScroll, NestedScrollConnection,
    NestedScrollSource, Rect, ScrollAxis, ScrollableState, Size,
    widgets::{
        Box, BoxSpec, Column, ColumnSpec, DEFAULT_SCRIM, DialogSpec, DialogWithFadingScrim,
        local_popup_viewport,
    },
    window_insets,
};
use cranpose_ui_graphics::{Brush, CornerRadii};
use cranpose_ui_layout::Axis;

use crate::theme::liquid_colors;

/// How long the sheet takes to open or close, as Compose's sheet does.
const MOTION_MS: u64 = 300;
/// A release faster than this, in dp per second, opens or closes the sheet
/// wherever it is.
const FLING_VELOCITY: f32 = 800.0;
/// How close to a rest the sheet counts as there.
const GONE: f32 = 0.01;
/// The space a tall sheet leaves above it.
const TOP_GAP: f32 = 48.0;
const MAX_WIDTH: f32 = 640.0;
const CORNER_RADIUS: f32 = 16.0;
const HANDLE_SIZE: Size = Size {
    width: 32.0,
    height: 4.0,
};
const HANDLE_PADDING: f32 = 12.0;

fn sheet_motion() -> AnimationType {
    AnimationType::Tween(AnimationSpec::tween(MOTION_MS, Easing::FastOutSlowInEasing))
}

/// Opens, moves and closes one [`LiquidModalBottomSheet`].
///
/// Compose's `SheetState`, from [`rememberLiquidSheetState`].
#[derive(Clone)]
pub struct LiquidSheetState(Rc<Sheet>);

impl PartialEq for LiquidSheetState {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl LiquidSheetState {
    /// Slides the sheet out of the window, then calls its `on_dismiss`.
    /// A Done button in the sheet closes it this way.
    pub fn hide(&self) {
        self.0.hide();
    }

    /// Opens the sheet to its full height.
    pub fn expand(&self) {
        self.0.settle_to(1.0);
    }

    /// Lowers the sheet to half the window, when its content is taller than
    /// that and the state does not skip the partly open sheet. Answers
    /// whether it could.
    pub fn partial_expand(&self) -> bool {
        self.0
            .partial()
            .map(|partial| self.0.settle_to(partial))
            .is_some()
    }

    /// Whether the sheet is at, or on its way to, its full height. A
    /// composable that reads it recomposes when that changes.
    pub fn is_expanded(&self) -> bool {
        self.0.expanded.get()
    }
}

/// Remembers the state of one sheet. A sheet whose content is taller than
/// half the window opens to half the window, and a drag opens it fully,
/// unless `skip_partially_expanded` is set. Compose's
/// `rememberModalBottomSheetState(skipPartiallyExpanded)`.
#[composable]
pub fn rememberLiquidSheetState(skip_partially_expanded: bool) -> LiquidSheetState {
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || {
                LiquidSheetState(Rc::new(Sheet::new(runtime, skip_partially_expanded)))
            })
            .with(Clone::clone)
    })
}

struct Sheet {
    runtime: RuntimeHandle,
    /// How far the sheet is in the window: `0.0` below it, `1.0` at its full
    /// height.
    shown: RefCell<Animatable<f32>>,
    /// The sheet's bounds from its last layout.
    bounds: Rc<Cell<Rect>>,
    /// The window's height from the last composition.
    window_height: Cell<f32>,
    skip_partial: bool,
    /// Whether the sheet rests, or goes, at its full height.
    expanded: MutableState<bool>,
    /// The current drag moved the sheet.
    dragged: Cell<bool>,
    hiding: Cell<bool>,
    on_dismiss: RefCell<Rc<dyn Fn()>>,
    /// Waits for the first layout, or for the end of the slide out.
    watcher: RefCell<Option<FrameCallbackRegistration>>,
}

impl Sheet {
    fn new(runtime: RuntimeHandle, skip_partial: bool) -> Self {
        Self {
            shown: RefCell::new(Animatable::new(0.0, runtime.clone())),
            expanded: MutableState::with_runtime(false, runtime.clone()),
            runtime,
            bounds: Rc::new(Cell::new(Rect::EMPTY)),
            window_height: Cell::new(0.0),
            skip_partial,
            dragged: Cell::new(false),
            hiding: Cell::new(false),
            on_dismiss: RefCell::new(Rc::new(|| {})),
            watcher: RefCell::new(None),
        }
    }

    fn shown(&self) -> f32 {
        self.shown.borrow().state().value()
    }

    /// Where the partly open sheet rests: half the window, when the sheet is
    /// taller than that.
    fn partial(&self) -> Option<f32> {
        let height = self.bounds.get().height;
        let half = self.window_height.get() * 0.5;
        (!self.skip_partial && height > half && half > 0.0).then(|| half / height)
    }

    /// Slides the sheet to `shown` of its height.
    fn settle_to(&self, shown: f32) {
        self.hiding.set(false);
        self.expanded.set(shown >= 1.0);
        self.shown.borrow_mut().animateTo(shown, sheet_motion());
    }

    /// Opens the sheet partly, or fully when it is short, once its first
    /// layout gives its height.
    fn open(self: &Rc<Self>) {
        self.hiding.set(false);
        if self.bounds.get().height > 0.0 {
            self.settle_to(self.partial().unwrap_or(1.0));
            return;
        }
        let weak = Rc::downgrade(self);
        let registration = self.runtime.frame_clock().with_frame_nanos(move |_| {
            if let Some(sheet) = weak.upgrade() {
                sheet.watcher.borrow_mut().take();
                sheet.open();
            }
        });
        *self.watcher.borrow_mut() = Some(registration);
    }

    /// Puts the sheet back below the window, for the next time it opens.
    fn reset(&self) {
        self.watcher.borrow_mut().take();
        self.hiding.set(false);
        self.dragged.set(false);
        self.expanded.set(false);
        self.shown.borrow_mut().snapTo(0.0);
    }

    /// Moves the sheet down by `delta`, up when it is negative, within the
    /// window; returns how far it moved.
    fn drag_by(&self, delta: f32) -> f32 {
        let height = self.bounds.get().height;
        if height <= 0.0 || self.hiding.get() {
            return 0.0;
        }
        let lowered = (1.0 - self.shown()) * height;
        let moved = (lowered + delta).clamp(0.0, height) - lowered;
        if moved != 0.0 {
            self.dragged.set(true);
            self.shown
                .borrow_mut()
                .snapTo(1.0 - (lowered + moved) / height);
        }
        moved
    }

    /// Brings a released sheet to rest: a fast release goes to the next
    /// rest the way it went, a slow one to the nearest, and a rest below the
    /// lowest open one closes the sheet. Returns whether the drag moved the
    /// sheet.
    fn release(self: &Rc<Self>, velocity: f32) -> bool {
        if !self.dragged.replace(false) {
            return false;
        }
        let shown = self.shown();
        let partial = self.partial();
        let rest = if velocity > FLING_VELOCITY {
            partial.filter(|partial| shown > partial + GONE)
        } else if velocity < -FLING_VELOCITY {
            Some(1.0)
        } else {
            [Some(0.0), partial, Some(1.0)]
                .into_iter()
                .flatten()
                .min_by(|a, b| (a - shown).abs().total_cmp(&(b - shown).abs()))
                .filter(|rest| *rest > 0.0)
        };
        match rest {
            Some(rest) => self.settle_to(rest),
            None => self.hide(),
        }
        true
    }

    fn hide(self: &Rc<Self>) {
        if self.hiding.replace(true) {
            return;
        }
        self.expanded.set(false);
        self.shown.borrow_mut().animateTo(0.0, sheet_motion());
        watch_hidden(self);
    }
}

/// Calls `on_dismiss` on the frame the sheet has left the window.
fn watch_hidden(sheet: &Rc<Sheet>) {
    let weak = Rc::downgrade(sheet);
    let registration = sheet.runtime.frame_clock().with_frame_nanos(move |_| {
        let Some(sheet) = weak.upgrade() else {
            return;
        };
        sheet.watcher.borrow_mut().take();
        if !sheet.hiding.get() {
            return;
        }
        if sheet.shown.borrow().is_running() {
            watch_hidden(&sheet);
            return;
        }
        let on_dismiss = Rc::clone(&sheet.on_dismiss.borrow());
        on_dismiss();
    });
    *sheet.watcher.borrow_mut() = Some(registration);
}

/// A list in the sheet scrolls as long as it can; what it leaves of a
/// downward drag at its top lowers the sheet, and an upward drag first
/// raises a lowered sheet.
struct SheetConnection(Rc<Sheet>);

impl NestedScrollConnection for SheetConnection {
    fn on_pre_scroll(&self, axis: ScrollAxis, available: f32, source: NestedScrollSource) -> f32 {
        if axis != ScrollAxis::Vertical || source != NestedScrollSource::Drag || available >= 0.0 {
            return 0.0;
        }
        self.0.drag_by(available)
    }

    fn on_post_scroll(
        &self,
        axis: ScrollAxis,
        _consumed: f32,
        available: f32,
        source: NestedScrollSource,
    ) -> f32 {
        if axis != ScrollAxis::Vertical || source != NestedScrollSource::Drag || available <= 0.0 {
            return 0.0;
        }
        self.0.drag_by(available)
    }

    fn on_pre_fling(&self, axis: ScrollAxis, velocity: f32) -> f32 {
        if axis == ScrollAxis::Vertical && self.0.release(velocity) {
            velocity
        } else {
            0.0
        }
    }
}

/// A reader opens a partly open sheet and lowers an open one.
fn sheet_actions(
    sheet: Rc<Sheet>,
    expanded: bool,
    can_lower: bool,
) -> impl Fn(&mut cranpose_ui::SemanticsConfiguration) {
    move |config| {
        let (opens, lowers) = (Rc::clone(&sheet), Rc::clone(&sheet));
        config.expand = (!expanded).then(|| {
            SemanticsExpand::new(move || {
                opens.settle_to(1.0);
                true
            })
        });
        config.collapse = can_lower.then(|| {
            SemanticsExpand::new(move || {
                lowers
                    .partial()
                    .map(|partial| lowers.settle_to(partial))
                    .is_some()
            })
        });
    }
}

/// A modal sheet holding `content` at the bottom of the window.
///
/// Show it while the app's flag is set, and clear the flag in `on_dismiss`.
/// The sheet slides up when it is composed, to half the window when its
/// content is taller than that (see [`rememberLiquidSheetState`]); a drag
/// moves it between that and its full height, and readers get expand and
/// collapse actions. A drag down, a tap on the scrim around it, the back
/// gesture and `state.hide()` slide it out and then call `on_dismiss`. A
/// scrolling list in the sheet hands the sheet the drag it leaves at its
/// top.
///
/// Compose's `ModalBottomSheet(onDismissRequest, sheetState)`.
#[composable]
pub fn LiquidModalBottomSheet(
    state: LiquidSheetState,
    on_dismiss: impl Fn() + 'static,
    content: impl Fn() + 'static,
) {
    let sheet = state.0;
    *sheet.on_dismiss.borrow_mut() = Rc::new(on_dismiss);
    DisposableEffect((), {
        let sheet = Rc::clone(&sheet);
        move |scope| {
            sheet.open();
            scope.on_dispose(move || sheet.reset())
        }
    });
    let connection = remember({
        let sheet = Rc::clone(&sheet);
        move || Rc::new(SheetConnection(sheet)) as Rc<dyn NestedScrollConnection>
    })
    .with(Rc::clone);
    // The sheet's own surface moves it through the connection, as a list does.
    let surface_drag = remember(|| ScrollableState::new(|_| 0.0)).with(Clone::clone);
    let viewport = local_popup_viewport().current().get();
    sheet.window_height.set(viewport.height);
    let expanded = sheet.expanded.get();
    let can_lower = expanded && !sheet.skip_partial;
    let insets = window_insets().combined();
    let max_height = (viewport.height - insets.top - TOP_GAP).max(0.0);
    let surface = liquid_colors().surface;
    let handle = liquid_colors().tertiary_label;
    let content = Rc::new(content);
    let scrim_sheet = Rc::clone(&sheet);
    let dismissing = Rc::clone(&sheet);
    DialogWithFadingScrim(
        DialogSpec::default()
            .with_window_insets(false)
            .with_alignment(Alignment::BOTTOM_CENTER),
        DEFAULT_SCRIM,
        move || scrim_sheet.shown(),
        move |_reason| dismissing.hide(),
        move || {
            let sheet = Rc::clone(&sheet);
            let content = Rc::clone(&content);
            let surface_drag = surface_drag.clone();
            NestedScroll(Rc::clone(&connection), move || {
                let layer_sheet = Rc::clone(&sheet);
                let modifier = Modifier::empty()
                    .width_in(0.0, MAX_WIDTH)
                    .fill_max_width()
                    .height_in(0.0, max_height)
                    .report_window_rect(Rc::clone(&sheet.bounds))
                    .graphics_layer(move || {
                        // Below the window until the first layout gives a height.
                        let height = layer_sheet.bounds.get().height;
                        let travel = if height > 0.0 {
                            height
                        } else {
                            viewport.height
                        };
                        GraphicsLayer {
                            translation_y: (1.0 - layer_sheet.shown()) * travel,
                            ..GraphicsLayer::default()
                        }
                    })
                    .scrollable(Axis::Vertical, surface_drag)
                    .semantics(sheet_actions(Rc::clone(&sheet), expanded, can_lower))
                    .draw_behind(move |scope| {
                        scope.draw_round_rect(
                            Brush::solid(surface),
                            CornerRadii {
                                top_left: CORNER_RADIUS,
                                top_right: CORNER_RADIUS,
                                bottom_right: 0.0,
                                bottom_left: 0.0,
                            },
                        );
                    })
                    .padding_each(0.0, 0.0, 0.0, insets.bottom);
                let content = Rc::clone(&content);
                Column(
                    modifier,
                    ColumnSpec::default()
                        .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
                    move || {
                        Box(
                            Modifier::empty()
                                .padding_vertical(HANDLE_PADDING)
                                .size(HANDLE_SIZE)
                                .draw_behind(move |scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(handle),
                                        CornerRadii::uniform(HANDLE_SIZE.height * 0.5),
                                    );
                                }),
                            BoxSpec::default(),
                            || {},
                        );
                        content();
                    },
                );
            });
        },
    );
}
