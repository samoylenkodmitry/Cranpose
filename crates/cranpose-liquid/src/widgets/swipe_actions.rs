use std::rc::Rc;

use cranpose_core::remember;
use cranpose_foundation::SemanticsCustomAction;
use cranpose_macros::composable;
use cranpose_ui::{
    AnchoredDraggableState, DraggableAnchors, Modifier, Point, SharedText,
    text::{AnnotatedString, TextStyle, measure_text},
    widgets::{Box, BoxSpec, Row, RowSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii};
use cranpose_ui_layout::{Alignment, Axis, LinearArrangement, VerticalAlignment};

use super::{content_scope::ScopeContent, label_button::LabelButton};
use crate::theme::liquid_typography;

// The iOS 27 swipe actions, measured on the simulator at 3x in a 44 point
// row: each action is a pill 9 points shorter than the row, filled with its
// tint, under a white 13 point label. The pills sit 10 points from each
// other, from the row's edge and from the content. Every pill on a side takes
// the width of the side's widest label and 12 points on each side of it.
const PILL_INSET: f32 = 4.5;
const GAP: f32 = 10.0;
const LABEL_PADDING: f32 = 12.0;

// The edge whose actions a row shows; a closed row shows none.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    Leading,
    Trailing,
}

impl Edge {
    // The direction the content travels to show this edge's actions.
    fn sign(self) -> f32 {
        match self {
            Self::Leading => 1.0,
            Self::Trailing => -1.0,
        }
    }

    fn alignment(self) -> Alignment {
        match self {
            Self::Leading => Alignment::CENTER_START,
            Self::Trailing => Alignment::CENTER_END,
        }
    }
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
    let actions = ScopeContent::collect(|actions| LiquidSwipeActionsScope { actions }, actions);
    let (leading, trailing): (Vec<_>, Vec<_>) = actions
        .into_iter()
        .partition(|action| action.edge == Edge::Leading);
    // Readers hear the trailing actions first.
    SwipeRow(
        modifier,
        [trailing.into(), leading.into()],
        Rc::new(content),
    );
}

// The pills of one edge: how far the content travels to show them, and the
// width of each.
#[derive(Clone, Copy, PartialEq)]
struct Reveal {
    edge: Edge,
    travel: f32,
    pill: f32,
}

fn reveal(actions: &[SwipeAction], style: &TextStyle) -> Option<Reveal> {
    let edge = actions.first()?.edge;
    let widest = actions
        .iter()
        .map(|action| measure_text(&AnnotatedString::from(action.label.as_str()), style).width)
        .fold(0.0_f32, f32::max);
    let pill = widest + 2.0 * LABEL_PADDING;
    let count = u16::try_from(actions.len()).map_or(f32::MAX, f32::from);
    Some(Reveal {
        edge,
        travel: count * (pill + GAP) + GAP,
        pill,
    })
}

#[composable(no_skip)]
fn SwipeRow(modifier: Modifier, sides: [Rc<[SwipeAction]>; 2], content: Rc<dyn Fn()>) {
    let mut style = liquid_typography().footnote;
    style.span_style.color = Some(Color::WHITE);
    let reveals = sides.each_ref().map(|side| reveal(side, &style));
    let anchors = DraggableAnchors::new(
        std::iter::once((None, 0.0)).chain(
            reveals
                .iter()
                .flatten()
                .map(|reveal| (Some(reveal.edge), reveal.edge.sign() * reveal.travel)),
        ),
    );
    let state = remember(|| AnchoredDraggableState::new(None, anchors.clone())).with(Clone::clone);
    state.update_anchors(anchors);
    let readers: Rc<[SemanticsCustomAction]> = sides
        .iter()
        .flat_map(|side| side.iter())
        .map(|action| {
            let on_click = Rc::clone(&action.on_click);
            SemanticsCustomAction::new(action.label.as_str().to_owned(), move || on_click())
        })
        .collect();
    Box(
        modifier
            .clip_to_bounds()
            .anchored_draggable(Axis::Horizontal, state.clone())
            .stable_semantics(move |config| {
                config.custom_actions.extend(readers.iter().cloned());
            }),
        BoxSpec::default(),
        move || {
            let moved = state.clone();
            let content = Rc::clone(&content);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .offset_with(move || Point::new(moved.offset().unwrap_or(0.0), 0.0)),
                BoxSpec::default(),
                move || content(),
            );
            CloseOverlay(state.clone());
            for (side, reveal) in sides.iter().zip(reveals) {
                if let Some(reveal) = reveal {
                    Pills(Rc::clone(side), reveal, style.clone(), state.clone());
                }
            }
        },
    );
}

// While the row is open, a tap on its content closes it.
#[composable(no_skip)]
fn CloseOverlay(state: AnchoredDraggableState<Option<Edge>>) {
    if state.settled_value().is_some() {
        Box(
            Modifier::empty()
                .fill_max_size()
                .clickable(move |_| state.animate_to(None))
                .hide_from_accessibility(),
            BoxSpec::default(),
            || {},
        );
    }
}

// The pills of one edge, which ride in from that edge with the content.
#[composable(no_skip)]
fn Pills(
    actions: Rc<[SwipeAction]>,
    reveal: Reveal,
    style: TextStyle,
    state: AnchoredDraggableState<Option<Edge>>,
) {
    let rest = -reveal.edge.sign() * reveal.travel;
    let moved = state.clone();
    Box(
        Modifier::empty()
            .fill_max_size()
            .offset_with(move || Point::new(rest + moved.offset().unwrap_or(0.0), 0.0)),
        BoxSpec::default().content_alignment(reveal.edge.alignment()),
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
                    if reveal.edge == Edge::Trailing {
                        for action in actions.iter().rev() {
                            Pill(action, reveal.pill, style.clone(), state.clone());
                        }
                    } else {
                        for action in actions.iter() {
                            Pill(action, reveal.pill, style.clone(), state.clone());
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
    state: AnchoredDraggableState<Option<Edge>>,
) {
    let tint = action.tint;
    let on_click = Rc::clone(&action.on_click);
    LabelButton(
        Modifier::empty()
            .width(width)
            .fill_max_height()
            .draw_behind(move |scope| {
                let radius = scope.size().height * 0.5;
                scope.draw_round_rect(Brush::solid(tint), CornerRadii::uniform(radius));
            }),
        action.label.as_str(),
        style,
        move || {
            on_click();
            state.animate_to(None);
        },
    );
}
