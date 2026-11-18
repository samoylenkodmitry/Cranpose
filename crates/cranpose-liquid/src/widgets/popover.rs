use std::rc::Rc;

use cranpose_core::rememberMutableStateOf;
use cranpose_macros::composable;
use cranpose_ui::{
    Modifier, Rect, Size,
    widgets::{Box, BoxSpec, DialogSpec, DialogWithScrim, DismissReason, local_popup_viewport},
    window_insets,
};
use cranpose_ui_graphics::{Color, EdgeInsets, GraphicsLayer, Point};
use cranpose_ui_layout::Alignment;

use super::alert_dialog;
use crate::{
    glass_morph::{GlassMorph, GlassTail, GlassTailEdge},
    material::{Glass, GlassDynamics, GlassShadow, LiquidModifierExt},
    theme::liquid_colors,
};

// The iOS 27 popover, measured on the simulator at 3x: a card with corners
// of 34 points and a tail 13 points high whose sides run at 45 degrees, its
// tip rounded by 2 points and its base curved into the card over 2 points.
// The card sits beside a control when it fits there, centred on it, else
// above it, else below it.
const CARD_RADIUS: f32 = 34.0;
const TAIL_HEIGHT: f32 = 13.0;
const TAIL_TIP_RADIUS: f32 = 2.0;
const TAIL_GLUE: f32 = 2.0;
// The space the card keeps from the window's safe edges.
const EDGE: f32 = 8.0;

/// Shows `content` on a glass card while `presented`, with a tail that
/// points at `anchor_content`. The card sits beside the anchor when it fits
/// there, centred on it, else above it, else below it. A tap outside the
/// card or the back gesture calls `on_dismiss_request`.
///
/// SwiftUI's `popover` presented as a popover on iPhone; UIKit's
/// `UIPopoverPresentationController`.
#[composable]
pub fn LiquidPopover<A>(
    modifier: Modifier,
    presented: bool,
    on_dismiss_request: impl Fn() + 'static,
    anchor_content: A,
    content: impl Fn() + 'static,
) where
    A: FnMut() + 'static,
{
    let on_dismiss: Rc<dyn Fn(DismissReason)> = Rc::new(move |_| on_dismiss_request());
    let anchor = PopoverAnchor(modifier, anchor_content);
    if presented {
        PopoverCard(anchor, on_dismiss, Rc::new(content));
    }
}

// Composes `anchor_content` and returns its window rectangle.
#[composable]
pub(super) fn PopoverAnchor(modifier: Modifier, anchor_content: impl FnMut() + 'static) -> Rect {
    let anchor = rememberMutableStateOf(|| Rect::EMPTY);
    Box(
        modifier.report_window_rect_state(anchor),
        BoxSpec::default(),
        anchor_content,
    );
    anchor.get()
}

// The modal card at its place by `anchor`, its glass face and tail behind
// `content`. The card stays transparent until its size is known.
#[composable(no_skip)]
pub(super) fn PopoverCard(
    anchor: Rect,
    on_dismiss: Rc<dyn Fn(DismissReason)>,
    content: Rc<dyn Fn()>,
) {
    let measured = rememberMutableStateOf(|| Size::ZERO);
    let card = measured.get();
    let viewport = local_popup_viewport().current().get();
    let placement = PopoverPlacement::new(anchor, card, viewport, window_insets().combined());
    let spec = DialogSpec {
        apply_window_insets: false,
        alignment: Alignment::TOP_START,
        offset: Point::new(placement.node.x, placement.node.y),
        ..DialogSpec::default()
    };
    let shown = card.width > 0.0 && card.height > 0.0;
    DialogWithScrim(
        spec,
        Color::TRANSPARENT,
        move |reason| on_dismiss(reason),
        move || {
            let content = Rc::clone(&content);
            let (left, top, right, bottom) = placement.tail_space();
            Box(
                Modifier::empty()
                    .graphics_layer(move || GraphicsLayer {
                        alpha: if shown { 1.0 } else { 0.0 },
                        ..Default::default()
                    })
                    .glass_effect_with(card_material().no_clip(), move || placement.dynamics())
                    .padding_each(left, top, right, bottom),
                BoxSpec::default(),
                move || {
                    let content = Rc::clone(&content);
                    Box(
                        Modifier::empty().report_size_state(measured),
                        BoxSpec::default(),
                        move || content(),
                    );
                },
            );
        },
    );
}

// The card's face, fitted to the native popover over the checkerboard: the
// alert's face with saturation 2.2, the backdrop blurred by 24 points under
// white at 65% in light mode, or by 32 points under gray 74 at 43.5% in dark
// mode, which keeps 32 over black; a shadow wider and fainter than the
// alert's.
fn card_material() -> Glass {
    let colors = liquid_colors();
    let (blur, tint) = if colors.is_dark {
        (32.0, Color::from_rgba_u8(74, 74, 74, 111))
    } else {
        (24.0, Color::from_rgba_u8(255, 255, 255, 166))
    };
    alert_dialog::card_material(&colors)
        .blur_radius(blur)
        .tint(tint)
        .saturation(2.2)
        .shadow_style(GlassShadow::new(
            Color::BLACK.with_alpha(0.13),
            44.0,
            8.0,
            1.5,
        ))
}

// Where the card and its tail go: the glass node in window coordinates, the
// card inside it and the tail it leaves the card by.
#[derive(Clone, Copy, PartialEq)]
struct PopoverPlacement {
    node: Rect,
    card: Rect,
    tail: GlassTail,
}

impl PopoverPlacement {
    fn new(anchor: Rect, card: Size, viewport: Size, insets: EdgeInsets) -> Self {
        let safe = Rect {
            x: insets.left + EDGE,
            y: insets.top + EDGE,
            width: viewport.width - insets.left - insets.right - EDGE * 2.0,
            height: viewport.height - insets.top - insets.bottom - EDGE * 2.0,
        };
        let edge = tail_edge(anchor, card, safe);
        let (card, tip) = card_beside(edge, anchor, card, safe);
        let node = node_around(edge, card);
        Self {
            node,
            card: Rect {
                x: card.x - node.x,
                y: card.y - node.y,
                ..card
            },
            tail: GlassTail {
                tip: (tip.x - node.x, tip.y - node.y),
                edge,
                height: TAIL_HEIGHT,
                tip_radius: TAIL_TIP_RADIUS,
                glue: TAIL_GLUE,
            },
        }
    }

    // The padding that leaves the tail's side of the glass node to the tail.
    fn tail_space(self) -> (f32, f32, f32, f32) {
        match self.tail.edge {
            GlassTailEdge::Left => (TAIL_HEIGHT, 0.0, 0.0, 0.0),
            GlassTailEdge::Top => (0.0, TAIL_HEIGHT, 0.0, 0.0),
            GlassTailEdge::Right => (0.0, 0.0, TAIL_HEIGHT, 0.0),
            GlassTailEdge::Bottom => (0.0, 0.0, 0.0, TAIL_HEIGHT),
        }
    }

    fn dynamics(self) -> GlassDynamics {
        let card = self.card;
        GlassDynamics {
            morph: Some(GlassMorph {
                node_size: (self.node.width, self.node.height),
                primary: (
                    card.x + card.width * 0.5,
                    card.y + card.height * 0.5,
                    card.width,
                    card.height,
                    CARD_RADIUS,
                ),
                tail: Some(self.tail),
                ..GlassMorph::default()
            }),
            ..GlassDynamics::default()
        }
    }
}

// The card's edge that the tail leaves: a side when the card fits beside the
// anchor, the side with more room; else the bottom, the card above the
// anchor, when it fits there or has more room there; else the top.
fn tail_edge(anchor: Rect, card: Size, safe: Rect) -> GlassTailEdge {
    let room_left = anchor.x - safe.x;
    let room_right = safe.x + safe.width - (anchor.x + anchor.width);
    let room_above = anchor.y - safe.y;
    let room_below = safe.y + safe.height - (anchor.y + anchor.height);
    if room_left.max(room_right) >= card.width + TAIL_HEIGHT {
        if room_right >= room_left {
            GlassTailEdge::Left
        } else {
            GlassTailEdge::Right
        }
    } else if room_above >= card.height + TAIL_HEIGHT || room_above >= room_below {
        GlassTailEdge::Bottom
    } else {
        GlassTailEdge::Top
    }
}

// The card in window coordinates on the far side of its tail from the
// anchor, centred on the anchor and inside `safe`, and the tail's tip on the
// anchor's facing edge, kept off the card's rounded corners.
fn card_beside(edge: GlassTailEdge, anchor: Rect, card: Size, safe: Rect) -> (Rect, Point) {
    let centred = |start: f32, length: f32, size: f32, middle: f32| {
        (middle - size * 0.5).min(start + length - size).max(start)
    };
    let along = |value: f32, start: f32, length: f32| {
        value
            .min(start + length - CARD_RADIUS)
            .max(start + CARD_RADIUS)
    };
    let middle = Point::new(
        anchor.x + anchor.width * 0.5,
        anchor.y + anchor.height * 0.5,
    );
    let beside = |x: f32, tip_x: f32| {
        let y = centred(safe.y, safe.height, card.height, middle.y);
        (
            Rect {
                x,
                y,
                width: card.width,
                height: card.height,
            },
            Point::new(tip_x, along(middle.y, y, card.height)),
        )
    };
    let over = |y: f32, tip_y: f32| {
        let x = centred(safe.x, safe.width, card.width, middle.x);
        (
            Rect {
                x,
                y,
                width: card.width,
                height: card.height,
            },
            Point::new(along(middle.x, x, card.width), tip_y),
        )
    };
    match edge {
        GlassTailEdge::Left => beside(
            anchor.x + anchor.width + TAIL_HEIGHT,
            anchor.x + anchor.width,
        ),
        GlassTailEdge::Right => beside(anchor.x - TAIL_HEIGHT - card.width, anchor.x),
        GlassTailEdge::Bottom => over(anchor.y - TAIL_HEIGHT - card.height, anchor.y),
        GlassTailEdge::Top => over(
            anchor.y + anchor.height + TAIL_HEIGHT,
            anchor.y + anchor.height,
        ),
    }
}

// The glass node: `card` grown by the tail on `edge`.
fn node_around(edge: GlassTailEdge, card: Rect) -> Rect {
    match edge {
        GlassTailEdge::Left => Rect {
            x: card.x - TAIL_HEIGHT,
            width: card.width + TAIL_HEIGHT,
            ..card
        },
        GlassTailEdge::Right => Rect {
            width: card.width + TAIL_HEIGHT,
            ..card
        },
        GlassTailEdge::Bottom => Rect {
            height: card.height + TAIL_HEIGHT,
            ..card
        },
        GlassTailEdge::Top => Rect {
            y: card.y - TAIL_HEIGHT,
            height: card.height + TAIL_HEIGHT,
            ..card
        },
    }
}
