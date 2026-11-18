use std::rc::Rc;

use cranpose_animation::{animate_float_as_state_with_initial, spring};
use cranpose_core::{
    MovableContent, MutableState, SideEffect, mutableStateOf, remember, rememberMovableContentOf,
    rememberMutableStateOf,
};
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Rect, Size, local_safe_area_insets,
    widgets::{Box, BoxSpec, Popup, local_popup_viewport},
};
use cranpose_ui_graphics::{Color, Dp, EdgeInsets, GraphicsLayer, LayerShape, Point, Shadow};

use super::{
    LiquidMenu, LiquidMenuPlacement, LiquidMenuScope, LiquidMenuSpec, rememberLiquidMenuGesture,
};
use crate::{material::LiquidShape, theme::liquid_colors};

// The iOS 27 context menu, measured on the simulator at 3x: a long press
// lifts the content in place at 1.15 times its size, under a soft shadow,
// and dims the rest of the screen with black, 20% in light and 48% in dark,
// with no blur. The preview keeps 16 points inside the safe area. The menu
// sits 16 points below it, or above it when no room is left below. A custom
// preview keeps its own size and rounds its corners by 32 points.
const LIFT_SCALE: f32 = 1.15;
const MARGIN: f32 = 16.0;
const MENU_GAP: f32 = 16.0;
const DIM_LIGHT: f32 = 0.20;
const DIM_DARK: f32 = 0.48;
const PREVIEW_RADIUS: f32 = 32.0;
// The light backdrop reads 30 levels darker at the preview's edge and still
// 7 levels darker 27 points out.
const SHADOW_RADIUS: f32 = 36.0;
const SHADOW_ALPHA: f32 = 0.29;
const LIFT_STIFFNESS: f32 = 300.0;

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
    ContextMenuBody(
        modifier,
        reader_label.into(),
        Rc::new(actions),
        Lift::Content(shape),
        Rc::new(content),
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
    ContextMenuBody(
        modifier,
        reader_label.into(),
        Rc::new(actions),
        Lift::Preview(Rc::new(preview)),
        Rc::new(content),
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

// The states of one context menu: whether it is open, and whether the
// overlay shows, which outlasts the menu by its closing motion.
#[derive(Clone, Copy)]
struct MenuStates {
    open: MutableState<bool>,
    shown: MutableState<bool>,
}

#[composable(no_skip)]
fn ContextMenuBody(
    modifier: Modifier,
    reader_label: String,
    actions: Rc<dyn Fn(&LiquidMenuScope)>,
    lift: Lift,
    content: Rc<dyn Fn()>,
) {
    let states = remember(|| MenuStates {
        open: mutableStateOf(false),
        shown: mutableStateOf(false),
    })
    .with(|states| *states);
    let source = rememberMutableStateOf(|| Rect::EMPTY);
    let body = rememberMovableContentOf(move || content());
    let preview_size = rememberMutableStateOf(|| Size::ZERO);
    let shown = states.shown.get();
    let moved = shown && matches!(lift, Lift::Content(_));
    let held = source.get();
    let wrapper =
        modifier
            .report_window_rect_state(source)
            .on_long_press(reader_label, move || {
                states.open.set(true);
                states.shown.set(true);
            });
    // The place the content leaves keeps its size, and a content that a
    // preview stands in for hides.
    let wrapper = match (moved, shown) {
        (true, _) => wrapper.size(Size::new(held.width, held.height)),
        (false, true) => wrapper.graphics_layer(|| GraphicsLayer {
            alpha: 0.0,
            ..GraphicsLayer::default()
        }),
        (false, false) => wrapper,
    };
    let in_place = body.clone();
    Box(wrapper, BoxSpec::default(), move || {
        if !moved {
            in_place.show();
        }
    });
    if !shown {
        return;
    }
    let size = match lift {
        Lift::Content(_) => Size::new(held.width, held.height),
        Lift::Preview(_) => preview_size.get(),
    };
    let target = lifted_rect(
        held,
        size,
        lift.scale(),
        local_popup_viewport().current().get(),
        local_safe_area_insets().current(),
    );
    LiftOverlay(states, lift, body, held, target, preview_size);
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

// Where the lifted preview of `size` and `scale` stands: centred on
// `source`, moved to keep `MARGIN` inside the safe area of `viewport`.
fn lifted_rect(source: Rect, size: Size, scale: f32, viewport: Size, safe: EdgeInsets) -> Rect {
    let (width, height) = (size.width * scale, size.height * scale);
    let centre = Point::new(
        source.x + source.width * 0.5,
        source.y + source.height * 0.5,
    );
    let fit = |centre: f32, extent: f32, low: f32, high: f32| {
        let start = centre - extent * 0.5;
        if high - low < extent {
            low
        } else {
            start.clamp(low, high - extent)
        }
    };
    Rect {
        x: fit(
            centre.x,
            width,
            safe.left + MARGIN,
            viewport.width - safe.right - MARGIN,
        ),
        y: fit(
            centre.y,
            height,
            safe.top + MARGIN,
            viewport.height - safe.bottom - MARGIN,
        ),
        width,
        height,
    }
}

// The dimmed screen and the lifted preview, which rise from `source` to
// `target` as the menu opens and settle back as it closes.
#[composable(no_skip)]
fn LiftOverlay(
    states: MenuStates,
    lift: Lift,
    body: MovableContent,
    source: Rect,
    target: Rect,
    preview_size: MutableState<Size>,
) {
    let open = states.open.get();
    let progress = animate_float_as_state_with_initial(
        0.0,
        if open { 1.0 } else { 0.0 },
        spring(1.0, LIFT_STIFFNESS),
        "context-menu-lift",
    );
    if !open && progress.get() < 0.001 {
        let shown = states.shown;
        SideEffect(move || shown.set(false));
    }
    let dim = if liquid_colors().is_dark {
        DIM_DARK
    } else {
        DIM_LIGHT
    };
    Popup(Rect::EMPTY, Point::ZERO, move || {
        let (lift, body) = (lift.clone(), body.clone());
        Box(
            Modifier::empty()
                .fill_max_size()
                .graphics_layer(move || GraphicsLayer {
                    alpha: progress.get(),
                    ..GraphicsLayer::default()
                })
                .background(Color::BLACK.with_alpha(dim)),
            BoxSpec::default(),
            || {},
        );
        let scale = lift.scale();
        // The preview lays out at its own size, centred where it lands, and
        // the lift scales it about its centre.
        let laid = Size::new(target.width / scale, target.height / scale);
        let from = Point::new(
            source.x + (source.width - laid.width) * 0.5,
            source.y + (source.height - laid.height) * 0.5,
        );
        let to = Point::new(
            target.x + (target.width - laid.width) * 0.5,
            target.y + (target.height - laid.height) * 0.5,
        );
        let shape = lift.shape();
        let preview = Modifier::empty()
            .absolute_offset_with(move || {
                let t = progress.get();
                Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
            })
            .graphics_layer(move || {
                let grown = 1.0 + (scale - 1.0) * progress.get();
                GraphicsLayer {
                    scale_x: grown,
                    scale_y: grown,
                    ..GraphicsLayer::default()
                }
            })
            .drop_shadow_value(
                LayerShape::Rounded(shape.clip_shape()),
                Shadow {
                    radius: Dp(SHADOW_RADIUS),
                    alpha: SHADOW_ALPHA,
                    ..Shadow::default()
                },
            );
        match lift {
            Lift::Content(_) => Box(preview.size(laid), BoxSpec::default(), move || body.show()),
            // The clip sits inside the shadow, which it would cut away.
            Lift::Preview(preview_content) => Box(
                preview.report_size_state(preview_size),
                BoxSpec::default(),
                move || {
                    let preview_content = Rc::clone(&preview_content);
                    Box(
                        Modifier::empty().graphics_layer(move || GraphicsLayer {
                            shape: LayerShape::Rounded(shape.clip_shape()),
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
