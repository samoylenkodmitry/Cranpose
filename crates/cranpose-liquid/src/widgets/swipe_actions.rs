use std::rc::Rc;

use cranpose_core::remember;
use cranpose_foundation::SemanticsCustomAction;
use cranpose_macros::composable;
use cranpose_ui::{
    AnchoredDraggableState, DraggableAnchors, Modifier, Point, SharedText,
    text::{AnnotatedString, TextStyle, measure_text},
    widgets::{Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii};
use cranpose_ui_layout::{Alignment, Axis, LinearArrangement, VerticalAlignment};

use super::content_scope::ScopeContent;
use crate::theme::liquid_typography;

// The iOS 27 swipe actions, measured on the simulator at 3x in a 44 point
// row: each action is a pill 9 points shorter than the row, filled with its
// tint, under a white 13 point label. The pills sit 10 points from each
// other, from the row's edge and from the content. Every pill on a side takes
// the width of the side's widest label and 12 points on each side of it.
const PILL_INSET: f32 = 4.5;
const GAP: f32 = 10.0;
const LABEL_PADDING: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum SwipeValue {
    Closed,
    TrailingOpen,
    LeadingOpen,
}

#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Leading,
    Trailing,
}

struct SwipeAction {
    edge: Edge,
    label: SharedText,
    tint: Color,
    on_click: Rc<dyn Fn()>,
}

/// The scope a [`LiquidSwipeActions`] row's actions are declared in.
pub struct LiquidSwipeActionsScope {
    actions: ScopeContent<SwipeAction>,
}

impl LiquidSwipeActionsScope {
    /// An action at the row's trailing edge, filled with `tint`, which a swipe
    /// toward the leading edge shows. The first one sits at the edge.
    pub fn trailing(
        &self,
        label: impl Into<SharedText>,
        tint: Color,
        on_click: impl Fn() + 'static,
    ) {
        self.push(Edge::Trailing, label.into(), tint, Rc::new(on_click));
    }

    /// An action at the row's leading edge, filled with `tint`, which a swipe
    /// toward the trailing edge shows. The first one sits at the edge.
    pub fn leading(
        &self,
        label: impl Into<SharedText>,
        tint: Color,
        on_click: impl Fn() + 'static,
    ) {
        self.push(Edge::Leading, label.into(), tint, Rc::new(on_click));
    }

    fn push(&self, edge: Edge, label: SharedText, tint: Color, on_click: Rc<dyn Fn()>) {
        self.actions.push(SwipeAction {
            edge,
            label,
            tint,
            on_click,
        });
    }
}

/// A row whose content slides aside under a horizontal swipe to show action
/// buttons along its edges. The row rests open on its actions; a tap on one
/// runs it and closes the row. Readers find every action among the row's
/// custom actions.
///
/// SwiftUI's `swipeActions` and UIKit's `UISwipeActionsConfiguration`.
#[composable]
pub fn LiquidSwipeActions(
    modifier: Modifier,
    actions: impl FnOnce(&LiquidSwipeActionsScope),
    content: impl Fn() + 'static,
) {
    // Only this shell is generic, so every call site shares the body below.
    SwipeRow(
        modifier,
        ScopeContent::collect(|actions| LiquidSwipeActionsScope { actions }, actions).into(),
        Rc::new(content),
    );
}

// How far the content travels to show the actions of `edge`, and the width
// of each of their pills.
fn reveal(actions: &[SwipeAction], edge: Edge, style: &TextStyle) -> (f32, f32) {
    let mut count = 0_u8;
    let mut widest = 0.0_f32;
    for action in actions.iter().filter(|action| action.edge == edge) {
        count += 1;
        let label = AnnotatedString::from(action.label.as_str());
        widest = widest.max(measure_text(&label, style).width);
    }
    if count == 0 {
        return (0.0, 0.0);
    }
    let pill = widest + 2.0 * LABEL_PADDING;
    (f32::from(count) * (pill + GAP) + GAP, pill)
}

#[composable(no_skip)]
fn SwipeRow(modifier: Modifier, actions: Rc<[SwipeAction]>, content: Rc<dyn Fn()>) {
    let mut style = liquid_typography().footnote;
    style.span_style.color = Some(Color::WHITE);
    let (trailing, trailing_pill) = reveal(&actions, Edge::Trailing, &style);
    let (leading, leading_pill) = reveal(&actions, Edge::Leading, &style);
    let anchors = DraggableAnchors::new(
        [
            Some((SwipeValue::Closed, 0.0)),
            (trailing > 0.0).then_some((SwipeValue::TrailingOpen, -trailing)),
            (leading > 0.0).then_some((SwipeValue::LeadingOpen, leading)),
        ]
        .into_iter()
        .flatten(),
    );
    let state = remember(|| AnchoredDraggableState::new(SwipeValue::Closed, anchors.clone()))
        .with(Clone::clone);
    state.update_anchors(anchors, state.settled_value());
    let readers = Rc::clone(&actions);
    let open = state.settled_value() != SwipeValue::Closed;
    Box(
        modifier
            .clip_to_bounds()
            .anchored_draggable(Axis::Horizontal, state.clone())
            .semantics(move |config| {
                for action in readers.iter() {
                    let on_click = Rc::clone(&action.on_click);
                    config.custom_actions.push(SemanticsCustomAction::new(
                        action.label.as_str().to_owned(),
                        move || on_click(),
                    ));
                }
            }),
        BoxSpec::default(),
        move || {
            let moved = state.clone();
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .offset_with(move || Point::new(moved.offset().unwrap_or(0.0), 0.0)),
                BoxSpec::default(),
                {
                    let content = Rc::clone(&content);
                    move || content()
                },
            );
            if open {
                let closing = state.clone();
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .clickable(move |_| closing.animate_to(SwipeValue::Closed)),
                    BoxSpec::default(),
                    || {},
                );
            }
            for (edge, travel, pill) in [
                (Edge::Trailing, trailing, trailing_pill),
                (Edge::Leading, leading, leading_pill),
            ] {
                if travel > 0.0 {
                    Pills(
                        Rc::clone(&actions),
                        edge,
                        travel,
                        pill,
                        style.clone(),
                        state.clone(),
                    );
                }
            }
        },
    );
}

// The pills of `edge`, which ride in from that edge with the content.
#[composable(no_skip)]
fn Pills(
    actions: Rc<[SwipeAction]>,
    edge: Edge,
    travel: f32,
    pill: f32,
    style: TextStyle,
    state: AnchoredDraggableState<SwipeValue>,
) {
    let (alignment, rest) = match edge {
        Edge::Trailing => (Alignment::CENTER_END, travel),
        Edge::Leading => (Alignment::CENTER_START, -travel),
    };
    let moved = state.clone();
    Box(
        Modifier::empty()
            .fill_max_size()
            .offset_with(move || Point::new(rest + moved.offset().unwrap_or(0.0), 0.0)),
        BoxSpec::default().content_alignment(alignment),
        move || {
            let (actions, style, state) = (Rc::clone(&actions), style.clone(), state.clone());
            Row(
                Modifier::empty()
                    .fill_max_height()
                    .padding_each(GAP, PILL_INSET, GAP, PILL_INSET),
                RowSpec::default()
                    .horizontal_arrangement(LinearArrangement::spaced_by(GAP))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    // The first action of a side sits at its edge.
                    let side = actions.iter().filter(|action| action.edge == edge);
                    if edge == Edge::Trailing {
                        for action in side.rev() {
                            Pill(action, pill, style.clone(), state.clone());
                        }
                    } else {
                        for action in side {
                            Pill(action, pill, style.clone(), state.clone());
                        }
                    }
                },
            );
        },
    );
}

#[composable(no_skip)]
fn Pill(
    action: &SwipeAction,
    width: f32,
    style: TextStyle,
    state: AnchoredDraggableState<SwipeValue>,
) {
    let tint = action.tint;
    let on_click = Rc::clone(&action.on_click);
    let label = action.label.clone();
    Box(
        Modifier::empty()
            .width(width)
            .fill_max_height()
            .draw_behind(move |scope| {
                let radius = scope.size().height * 0.5;
                scope.draw_round_rect(Brush::solid(tint), CornerRadii::uniform(radius));
            })
            .clickable(move |_| {
                on_click();
                state.animate_to(SwipeValue::Closed);
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(label.clone(), Modifier::empty(), style.clone());
        },
    );
}
