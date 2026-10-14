//! A short label above a control, as Compose's `TooltipBox` with a
//! `PlainTooltip`.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{
    MutableState, RuntimeHandle, State, internal::FrameCallbackRegistration, remember,
    rememberMutableStateOf, with_current_composer,
};
use cranpose_foundation::LiveRegionMode;
use cranpose_macros::composable;
use cranpose_ui::{
    FocusWithin, Layout, Modifier, Rect, Size, Text, TextStyle, rememberMutableInteractionSource,
    widgets::{Box, BoxSpec, Popup, local_popup_viewport},
};
use cranpose_ui_graphics::{Brush, CornerRadii, Point};
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::theme::{liquid_colors, liquid_typography};

/// How long a tooltip that a long press showed stays, as Compose's.
const LONG_PRESS_SHOWN_NANOS: u64 = 1_500_000_000;
/// The space between the tooltip and its control.
const GAP: f32 = 4.0;
/// The space a tooltip keeps from the window's edges.
const EDGE: f32 = 8.0;
const CORNER_RADIUS: f32 = 6.0;

/// Shows `tooltip` above `content` while the pointer rests on it, while a
/// control in it has keyboard focus, and for a moment after a long press on
/// it. A screen reader asks for the tooltip with the long press action named
/// `reader_label` ("Show tooltip", in the app's language) and hears it read
/// out.
///
/// Compose's `TooltipBox(tooltip = { PlainTooltip { Text(…) } })`.
#[composable]
pub fn LiquidTooltipBox(
    tooltip: impl Into<String>,
    reader_label: impl Into<String>,
    modifier: Modifier,
    content: impl FnMut() + 'static,
) {
    let tooltip = tooltip.into();
    let hover = rememberMutableInteractionSource();
    let shown = Shown {
        hovered: hover.collectIsHoveredAsState(),
        focused: rememberMutableStateOf(|| false),
        long_pressed: rememberMutableStateOf(|| false),
    };
    let anchor = rememberMutableStateOf(|| Rect::EMPTY);
    let timer = remember(|| Rc::new(RefCell::new(None))).with(Rc::clone);
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let long_pressed = shown.long_pressed;
    let wrapper = modifier
        .report_window_rect_state(anchor)
        .hoverable(hover, true)
        .on_long_press(reader_label, move || {
            long_pressed.set(true);
            hide_later(&timer, &runtime, long_pressed);
        });
    let focused = shown.focused;
    let mut content = content;
    Box(wrapper, BoxSpec::default(), move || {
        FocusWithin(move |inside| focused.set(inside), &mut content);
        Tooltip(tooltip.clone(), shown, anchor);
    });
}

/// Why the tooltip shows.
#[derive(Clone, Copy, PartialEq)]
struct Shown {
    hovered: State<bool>,
    focused: MutableState<bool>,
    long_pressed: MutableState<bool>,
}

/// Clears `long_pressed` once the tooltip has stayed its time in frames.
fn hide_later(
    timer: &Rc<RefCell<Option<FrameCallbackRegistration>>>,
    runtime: &RuntimeHandle,
    long_pressed: MutableState<bool>,
) {
    fn watch(
        timer: Rc<RefCell<Option<FrameCallbackRegistration>>>,
        runtime: RuntimeHandle,
        long_pressed: MutableState<bool>,
        started: Option<u64>,
    ) {
        let next = runtime.clone();
        let slot = Rc::clone(&timer);
        let registration = runtime.frame_clock().with_frame_nanos(move |frame_nanos| {
            slot.borrow_mut().take();
            let started = started.unwrap_or(frame_nanos);
            if frame_nanos.saturating_sub(started) < LONG_PRESS_SHOWN_NANOS {
                watch(slot, next, long_pressed, Some(started));
            } else {
                long_pressed.set(false);
            }
        });
        *timer.borrow_mut() = Some(registration);
    }
    watch(Rc::clone(timer), runtime.clone(), long_pressed, None);
}

#[composable]
fn Tooltip(tooltip: String, shown: Shown, anchor: MutableState<Rect>) {
    if !(shown.hovered.value() || shown.focused.get() || shown.long_pressed.get()) {
        return;
    }
    let placement = TooltipPlacement {
        anchor: anchor.get(),
        viewport: local_popup_viewport().current().get(),
    };
    let colors = liquid_colors();
    // Inverse colors, as a tooltip stands apart from the screen under it.
    let (background, foreground) = (colors.label, colors.background);
    let caption = liquid_typography().caption1;
    let style = TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(foreground),
            ..caption.span_style
        },
        ..caption
    };
    Popup(Rect::EMPTY, Point::ZERO, move || {
        let tooltip = tooltip.clone();
        let style = style.clone();
        Layout(
            Modifier::empty().size(placement.viewport),
            placement,
            move || {
                Text(
                    tooltip.clone(),
                    Modifier::empty()
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(background),
                                CornerRadii::uniform(CORNER_RADIUS),
                            );
                        })
                        .padding_horizontal(8.0)
                        .padding_vertical(4.0)
                        .live_region(LiveRegionMode::Polite),
                    style.clone(),
                );
            },
        );
    });
}

/// Puts the tooltip above its control, centred on it and inside the
/// window, or below the control when there is no room above it.
#[derive(Clone, Copy, PartialEq)]
struct TooltipPlacement {
    anchor: Rect,
    viewport: Size,
}

impl MeasurePolicy for TooltipPlacement {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        _constraints: Constraints,
    ) -> MeasureResult {
        let Self { anchor, viewport } = *self;
        let loose = Constraints {
            min_width: 0.0,
            max_width: (viewport.width - EDGE * 2.0).max(0.0),
            min_height: 0.0,
            max_height: viewport.height,
        };
        let placements = measurables
            .iter()
            .map(|measurable| {
                let placeable = measurable.measure(loose);
                let (width, height) = (placeable.width(), placeable.height());
                let x = (anchor.x + (anchor.width - width) * 0.5)
                    .min(viewport.width - EDGE - width)
                    .max(EDGE);
                let above = anchor.y - GAP - height;
                let y = if above >= EDGE {
                    above
                } else {
                    anchor.y + anchor.height + GAP
                };
                placeable.place(x, y);
                Placement::new(placeable.node_id(), x, y, 0)
            })
            .collect();
        MeasureResult::new(viewport, placements)
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.viewport.width
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.viewport.width
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.viewport.height
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.viewport.height
    }
}
