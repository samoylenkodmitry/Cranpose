use std::{rc::Rc, time::Duration};

use cranpose_animation::{
    AnimationSpec, AnimationType, Easing, Lerp, animate_float_as_state_with_initial, spring,
};
use cranpose_core::{
    LaunchedEffect, MovableContent, MutableState, delay, mutableStateOf, remember,
    rememberMovableContentOf, rememberMutableStateOf,
};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Rect, Size,
    widgets::{Box, BoxSpec, Popup},
};
use cranpose_ui_graphics::{Brush, Color, Dp, GraphicsLayer, Point, Shadow};

use super::{
    LiquidMenu, LiquidMenuPlacement, LiquidMenuScope, LiquidMenuSpec, alert_dialog::modal_dim,
    menu_placement::MenuArea, rememberLiquidMenuGesture,
};
use crate::material::LiquidShape;

// The iOS 27 context menu, measured on the simulator at 3x: a long press
// lifts the content in place at 1.15 times its size, under a soft shadow,
// and dims the rest of the screen as a modal dialog does, with no blur. The
// preview keeps 16 points inside the safe area. The menu sits 16 points
// below it, or above it when no room is left below. A custom preview keeps
// its own size and rounds its corners by 32 points.
const LIFT_SCALE: f32 = 1.15;
const MENU_GAP: f32 = 16.0;
const PREVIEW_RADIUS: f32 = 32.0;
// The light backdrop reads 30 levels darker at the preview's edge and still
// 7 levels darker 27 points out.
const SHADOW_RADIUS: f32 = 36.0;
const SHADOW_ALPHA: f32 = 0.29;
const LIFT_STIFFNESS: f32 = 300.0;
// The preview settles back within the menu's own close, which takes 205 ms.
const CLOSE_MS: u64 = 260;

/// A region whose long press lifts `content` above the dimmed screen at
/// 1.15 times its size and opens a menu of `actions` beside it. A tap on an
/// action runs it and closes the menu; a tap anywhere else closes it.
/// `shape` is the outline of the content, which the shadow of the lifted
/// content follows. Readers open the menu through the action named
/// `reader_label`. The lifted content is the content itself, moved, so it
/// keeps its state.
///
/// SwiftUI's `contextMenu(menuItems:)` and UIKit's
/// `UIContextMenuInteraction`.
#[composable]
pub fn LiquidContextMenu(
    modifier: Modifier,
    shape: LiquidShape,
    reader_label: impl Into<String>,
    actions: impl Fn(&LiquidMenuScope) + 'static,
    content: impl Fn() + 'static,
) {
    // Only this shell is generic, so every call site shares the body below.
    let content = rememberMovableContentOf(content);
    ContextMenuBody(
        modifier,
        reader_label.into(),
        Rc::new(actions),
        Lift::Content(shape),
        content,
    );
}

/// [`LiquidContextMenu`] that lifts `preview` in place of the content: at
/// its own size, centred on the content, its corners rounded by 32 points.
/// The content stays in place, hidden while the menu is open.
///
/// SwiftUI's `contextMenu(menuItems:preview:)`.
#[composable]
pub fn LiquidContextMenuWithPreview(
    modifier: Modifier,
    reader_label: impl Into<String>,
    actions: impl Fn(&LiquidMenuScope) + 'static,
    preview: impl Fn() + 'static,
    content: impl Fn() + 'static,
) {
    let content = rememberMovableContentOf(content);
    ContextMenuBody(
        modifier,
        reader_label.into(),
        Rc::new(actions),
        Lift::Preview(Rc::new(preview)),
        content,
    );
}

// What a long press lifts above the screen.
#[derive(Clone)]
enum Lift {
    // The content itself, with the outline its shadow follows.
    Content(LiquidShape),
    // A preview of its own.
    Preview(Rc<dyn Fn()>),
}

impl Lift {
    fn scale(&self) -> f32 {
        match self {
            Self::Content(_) => LIFT_SCALE,
            Self::Preview(_) => 1.0,
        }
    }

    fn shape(&self) -> LiquidShape {
        match self {
            Self::Content(shape) => *shape,
            Self::Preview(_) => LiquidShape::RoundedRect(PREVIEW_RADIUS),
        }
    }
}

// The states of one context menu: whether it is open, whether the overlay
// shows, which outlasts the menu by its closing motion, and where the
// content stood when the long press opened it.
#[derive(Clone, Copy)]
struct MenuStates {
    open: MutableState<bool>,
    shown: MutableState<bool>,
    held: MutableState<Rect>,
}

#[composable(no_skip)]
fn ContextMenuBody(
    modifier: Modifier,
    reader_label: String,
    actions: Rc<dyn Fn(&LiquidMenuScope)>,
    lift: Lift,
    content: MovableContent,
) {
    let states = remember(|| MenuStates {
        open: mutableStateOf(false),
        shown: mutableStateOf(false),
        held: mutableStateOf(Rect::EMPTY),
    })
    .with(|states| *states);
    // Read only when the menu opens, so a scroll does not compose the region.
    let source = rememberMutableStateOf(|| Rect::EMPTY);
    let preview_size = rememberMutableStateOf(|| Size::ZERO);
    let shown = states.shown.get();
    let held = if shown {
        states.held.get()
    } else {
        Rect::EMPTY
    };
    let wrapper =
        modifier
            .report_window_rect_state(source)
            .on_long_press(reader_label, move || {
                states.held.set(source.get_non_reactive());
                states.open.set(true);
                states.shown.set(true);
            });
    // While the menu shows, the place the content leaves keeps its size, and
    // content that a preview stands in for hides.
    let (wrapper, in_place) = match (&lift, shown) {
        (_, false) => (wrapper, Some(content.clone())),
        (Lift::Content(_), true) => (wrapper.size(held.size()), None),
        (Lift::Preview(_), true) => (
            wrapper.graphics_layer_value(GraphicsLayer {
                alpha: 0.0,
                ..GraphicsLayer::default()
            }),
            Some(content.clone()),
        ),
    };
    Box(wrapper, BoxSpec::default(), move || {
        if let Some(in_place) = &in_place {
            in_place.show();
        }
    });
    if !shown {
        return;
    }
    let laid = match lift {
        Lift::Content(_) => held.size(),
        Lift::Preview(_) => preview_size.get(),
    };
    let (width, height) = (laid.width * lift.scale(), laid.height * lift.scale());
    let centred = Rect {
        x: held.x + (held.width - width) * 0.5,
        y: held.y + (held.height - height) * 0.5,
        width,
        height,
    };
    let target = MenuArea::current().map_or(centred, |area| area.fit(centred));
    LiftOverlay(states, lift, content, held, target, preview_size);
    let gesture = rememberLiquidMenuGesture();
    let spec =
        LiquidMenuSpec::default().placement(LiquidMenuPlacement::BesideAnchor { gap: MENU_GAP });
    let open = states.open;
    LiquidMenu(
        open.get(),
        target,
        spec,
        Vec::new(),
        gesture,
        move || open.set(false),
        move |scope| actions(scope),
    );
}

// The dimmed screen and the lifted preview, which rise from `source` to
// `target` as the menu opens and settle back as it closes.
#[composable(no_skip)]
fn LiftOverlay(
    states: MenuStates,
    lift: Lift,
    content: MovableContent,
    source: Rect,
    target: Rect,
    preview_size: MutableState<Size>,
) {
    let open = states.open.get();
    let progress = animate_float_as_state_with_initial(
        0.0,
        if open { 1.0 } else { 0.0 },
        if open {
            spring(1.0, LIFT_STIFFNESS)
        } else {
            AnimationType::Tween(AnimationSpec::tween(CLOSE_MS, Easing::EaseOut))
        },
        "context-menu-lift",
    );
    let shown = states.shown;
    LaunchedEffect(open, async move |_| {
        if !open {
            delay(Duration::from_millis(CLOSE_MS)).await;
            shown.set(false);
        }
    });
    let dim = Color::BLACK.with_alpha(modal_dim());
    Popup(Rect::EMPTY, Point::ZERO, move || {
        let (lift, content) = (lift.clone(), content.clone());
        Box(
            Modifier::empty().fill_max_size().draw_behind(move |scope| {
                scope.draw_rect(Brush::solid(dim.with_alpha(dim.a() * progress.get())));
            }),
            BoxSpec::default(),
            || {},
        );
        // The preview lays out at its own size, centred where it lands, and
        // the lift moves it there from its place and scales it about its
        // centre.
        let scale = lift.scale();
        let laid = Size::new(target.width / scale, target.height / scale);
        let centred = |rect: Rect| {
            Point::new(
                rect.x + (rect.width - laid.width) * 0.5,
                rect.y + (rect.height - laid.height) * 0.5,
            )
        };
        let (from, to) = (centred(source), centred(target));
        let outline = lift.shape().layer_shape();
        let preview = Modifier::empty()
            .absolute_offset_with(move || from.lerp(&to, progress.get()))
            .graphics_layer(move || {
                let grown = 1.0.lerp(&scale, progress.get());
                GraphicsLayer {
                    scale_x: grown,
                    scale_y: grown,
                    ..GraphicsLayer::default()
                }
            })
            .drop_shadow_value(
                outline,
                Shadow {
                    radius: Dp(SHADOW_RADIUS),
                    alpha: SHADOW_ALPHA,
                    ..Shadow::default()
                },
            );
        match lift {
            Lift::Content(_) => Box(preview.size(laid), BoxSpec::default(), move || {
                content.show();
            }),
            // The clip sits inside the shadow, which it would cut away.
            Lift::Preview(preview_content) => Box(
                preview.report_size_state(preview_size),
                BoxSpec::default(),
                move || {
                    let preview_content = Rc::clone(&preview_content);
                    Box(
                        Modifier::empty().graphics_layer_value(GraphicsLayer {
                            shape: outline,
                            clip: true,
                            ..GraphicsLayer::default()
                        }),
                        BoxSpec::default(),
                        move || preview_content(),
                    );
                },
            ),
        };
    });
}
