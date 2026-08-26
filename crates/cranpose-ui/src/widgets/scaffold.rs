//! Compose-like window scaffold with framework-owned system insets.

#![allow(non_snake_case)]

use std::rc::Rc;

use cranpose_core::{NodeId, SlotId};
use cranpose_ui_graphics::{Color, EdgeInsets};
use cranpose_ui_layout::{Constraints, Placement};

use super::SubcomposeLayout;
use crate::{
    Modifier, composable,
    layout_direction::{LayoutDirection, layout_direction},
    subcompose_layout::{SubcomposeLayoutScope, SubcomposeMeasureScope},
};

/// Insets supplied to a scaffold's content slot, in reading order.
///
/// `start` and `end` are the reading-order edges: in a left-to-right layout
/// `start` is the left edge, in a right-to-left layout it is the right one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PaddingValues {
    pub start: f32,
    pub top: f32,
    pub end: f32,
    pub bottom: f32,
}

impl PaddingValues {
    pub const fn new(start: f32, top: f32, end: f32, bottom: f32) -> Self {
        Self {
            start,
            top,
            end,
            bottom,
        }
    }

    /// The same padding on every edge.
    pub const fn all(value: f32) -> Self {
        Self::new(value, value, value, value)
    }

    /// Applies these values in the composition's current reading order.
    pub fn apply_to(self, modifier: Modifier) -> Modifier {
        self.apply_to_in(modifier, layout_direction())
    }

    /// Applies these values against an explicit reading order.
    pub fn apply_to_in(self, modifier: Modifier, direction: LayoutDirection) -> Modifier {
        modifier.padding_relative_in(direction, self.start, self.top, self.end, self.bottom)
    }

    /// The physical left/top/right/bottom edges in `direction`.
    pub fn physical(self, direction: LayoutDirection) -> EdgeInsets {
        let (left, right) = direction.resolve(self.start, self.end);
        EdgeInsets::from_components(left, self.top, right, self.bottom)
    }

    /// The larger of each edge, used to merge bar heights with window insets.
    pub fn max(self, other: Self) -> Self {
        Self::new(
            self.start.max(other.start),
            self.top.max(other.top),
            self.end.max(other.end),
            self.bottom.max(other.bottom),
        )
    }
}

/// The surfaces a scaffold paints behind its slots.
///
/// A scaffold that states no colours paints nothing and leaves every surface to
/// its slots, which is what a full-bleed game or a custom-chrome window wants.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScaffoldColors {
    /// Painted across the whole scaffold, behind every slot.
    pub background: Option<Color>,
    /// Painted behind the top bar, including the status-bar area it draws under.
    pub top_bar: Option<Color>,
    /// Painted behind the bottom bar, including the navigation-bar area.
    pub bottom_bar: Option<Color>,
}

impl ScaffoldColors {
    /// A scaffold whose background is `background` and whose bars are painted
    /// with the same colour.
    pub fn uniform(background: Color) -> Self {
        Self {
            background: Some(background),
            top_bar: Some(background),
            bottom_bar: Some(background),
        }
    }

    /// Sets the whole-scaffold background.
    pub fn with_background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }
}

/// Which window insets a scaffold consumes on the application's behalf.
///
/// The default consumes every side, which is what an ordinary screen wants. A
/// screen that draws its own edge-to-edge content — a map, a photo viewer —
/// turns off the sides it handles itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaffoldContentInsets {
    /// Consume the top window inset (status bar, notch).
    pub top: bool,
    /// Consume the bottom window inset (navigation bar, home indicator, IME).
    pub bottom: bool,
    /// Consume the reading-order start inset (a cutout, a curved edge).
    pub start: bool,
    /// Consume the reading-order end inset.
    pub end: bool,
}

impl Default for ScaffoldContentInsets {
    fn default() -> Self {
        Self {
            top: true,
            bottom: true,
            start: true,
            end: true,
        }
    }
}

impl ScaffoldContentInsets {
    /// Consume nothing: every slot sees the raw window bounds.
    pub const fn none() -> Self {
        Self {
            top: false,
            bottom: false,
            start: false,
            end: false,
        }
    }

    /// Keeps only the sides this asked for.
    fn filter(self, insets: PaddingValues) -> PaddingValues {
        PaddingValues::new(
            if self.start { insets.start } else { 0.0 },
            if self.top { insets.top } else { 0.0 },
            if self.end { insets.end } else { 0.0 },
            if self.bottom { insets.bottom } else { 0.0 },
        )
    }
}

/// Everything a scaffold shows besides its content.
#[derive(Clone)]
struct ScaffoldSlots {
    top_bar: Rc<dyn Fn()>,
    bottom_bar: Rc<dyn Fn()>,
    floating_action: Rc<dyn Fn()>,
    content: Rc<dyn Fn(PaddingValues)>,
}

impl PartialEq for ScaffoldSlots {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.top_bar, &other.top_bar)
            && Rc::ptr_eq(&self.bottom_bar, &other.bottom_bar)
            && Rc::ptr_eq(&self.floating_action, &other.floating_action)
            && Rc::ptr_eq(&self.content, &other.content)
    }
}

/// How a scaffold is configured beyond its slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScaffoldSpec {
    /// Surfaces painted behind the slots.
    pub colors: ScaffoldColors,
    /// Which window insets the scaffold consumes.
    pub content_insets: ScaffoldContentInsets,
}

impl ScaffoldSpec {
    /// Sets the surfaces.
    pub fn with_colors(mut self, colors: ScaffoldColors) -> Self {
        self.colors = colors;
        self
    }
}

/// Places optional window bars and a floating action over a full-size content
/// slot, and reports the space occupied by the bars and by platform
/// obstructions as reading-order inner padding.
///
/// Bars receive the full window bounds so they can draw behind system areas; a
/// bar that places controls there uses [`crate::window_insets`].
pub fn Scaffold<T, B, C>(modifier: Modifier, top_bar: T, bottom_bar: B, content: C) -> NodeId
where
    T: Fn() + 'static,
    B: Fn() + 'static,
    C: Fn(PaddingValues) + 'static,
{
    ScaffoldWith(
        modifier,
        ScaffoldSpec::default(),
        top_bar,
        bottom_bar,
        || {},
        content,
    )
}

/// A scaffold with surfaces, inset policy and a floating-action slot.
pub fn ScaffoldWith<T, B, F, C>(
    modifier: Modifier,
    spec: ScaffoldSpec,
    top_bar: T,
    bottom_bar: B,
    floating_action: F,
    content: C,
) -> NodeId
where
    T: Fn() + 'static,
    B: Fn() + 'static,
    F: Fn() + 'static,
    C: Fn(PaddingValues) + 'static,
{
    ScaffoldImpl(
        modifier,
        spec,
        ScaffoldSlots {
            top_bar: Rc::new(top_bar),
            bottom_bar: Rc::new(bottom_bar),
            floating_action: Rc::new(floating_action),
            content: Rc::new(content),
        },
    )
}

/// The gap a floating action keeps from the window edges.
const FLOATING_ACTION_MARGIN: f32 = 16.0;

#[composable]
fn ScaffoldImpl(modifier: Modifier, spec: ScaffoldSpec, slots: ScaffoldSlots) -> NodeId {
    let direction = layout_direction();
    let insets = crate::safe_area::window_insets().combined();
    let (start_inset, end_inset) = match direction {
        LayoutDirection::Ltr => (insets.left, insets.right),
        LayoutDirection::Rtl => (insets.right, insets.left),
    };
    let window_padding = spec.content_insets.filter(PaddingValues::new(
        start_inset,
        insets.top,
        end_inset,
        insets.bottom,
    ));

    let colors = spec.colors;
    let modifier = match colors.background {
        Some(background) => modifier.background(background),
        None => modifier,
    };

    let top_bar = slots.top_bar;
    let bottom_bar = slots.bottom_bar;
    let floating_action = slots.floating_action;
    let content = slots.content;

    SubcomposeLayout(modifier, move |scope, constraints| {
        let width = constraints.max_width.max(constraints.min_width);
        let height = constraints.max_height.max(constraints.min_height);
        let loose = Constraints::loose(width, height);

        let top_content = Rc::clone(&top_bar);
        let top_nodes = scope.subcompose(SlotId::new(0), move || top_content());
        let mut top_height = 0.0_f32;
        let mut top_placements = Vec::with_capacity(top_nodes.len());
        for node in top_nodes {
            let placeable = scope.measure(node, loose);
            top_height = top_height.max(placeable.height());
            top_placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 1));
        }

        let bottom_content = Rc::clone(&bottom_bar);
        let bottom_nodes = scope.subcompose(SlotId::new(1), move || bottom_content());
        let mut bottom_height = 0.0_f32;
        let mut bottom_placeables = Vec::with_capacity(bottom_nodes.len());
        for node in bottom_nodes {
            let placeable = scope.measure(node, loose);
            bottom_height = bottom_height.max(placeable.height());
            bottom_placeables.push(placeable);
        }

        // A bar already covers the window inset behind it, so the content is
        // pushed past whichever is larger rather than past both.
        let padding = window_padding.max(PaddingValues::new(0.0, top_height, 0.0, bottom_height));
        let content_slot = Rc::clone(&content);
        let content_nodes = scope.subcompose(SlotId::new(2), move || content_slot(padding));

        let mut placements = Vec::with_capacity(
            top_placements.len() + bottom_placeables.len() + content_nodes.len() + 1,
        );
        for node in content_nodes {
            let placeable = scope.measure(node, Constraints::tight(width, height));
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
        }
        placements.extend(top_placements);
        for placeable in bottom_placeables {
            placements.push(Placement::new(
                placeable.node_id(),
                0.0,
                (height - placeable.height()).max(0.0),
                1,
            ));
        }

        // The floating action sits above everything, inside the bottom bar and
        // the window insets, on the reading-order end side.
        let floating_content = Rc::clone(&floating_action);
        let floating_nodes = scope.subcompose(SlotId::new(3), move || floating_content());
        for node in floating_nodes {
            let placeable = scope.measure(node, loose);
            let bottom_gap = padding.bottom + FLOATING_ACTION_MARGIN;
            let end_gap = padding.end + FLOATING_ACTION_MARGIN;
            let x = match direction {
                LayoutDirection::Ltr => (width - placeable.width() - end_gap).max(0.0),
                LayoutDirection::Rtl => end_gap.min((width - placeable.width()).max(0.0)),
            };
            let y = (height - placeable.height() - bottom_gap).max(0.0);
            placements.push(Placement::new(placeable.node_id(), x, y, 2));
        }

        scope.layout(width, height, placements)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_values_preserve_each_edge() {
        assert_eq!(
            PaddingValues::new(1.0, 2.0, 3.0, 4.0),
            PaddingValues {
                start: 1.0,
                top: 2.0,
                end: 3.0,
                bottom: 4.0,
            }
        );
        assert_eq!(
            PaddingValues::all(6.0),
            PaddingValues::new(6.0, 6.0, 6.0, 6.0)
        );
    }

    #[test]
    fn reading_order_padding_swaps_sides_in_a_right_to_left_layout() {
        let padding = PaddingValues::new(24.0, 2.0, 8.0, 4.0);
        assert_eq!(
            padding.physical(LayoutDirection::Ltr),
            EdgeInsets::from_components(24.0, 2.0, 8.0, 4.0)
        );
        assert_eq!(
            padding.physical(LayoutDirection::Rtl),
            EdgeInsets::from_components(8.0, 2.0, 24.0, 4.0)
        );
    }

    #[test]
    fn merging_takes_the_larger_of_each_edge() {
        let bars = PaddingValues::new(0.0, 56.0, 0.0, 0.0);
        let insets = PaddingValues::new(0.0, 24.0, 0.0, 48.0);
        assert_eq!(insets.max(bars), PaddingValues::new(0.0, 56.0, 0.0, 48.0));
    }

    #[test]
    fn a_screen_can_keep_the_insets_it_draws_under() {
        let insets = PaddingValues::new(4.0, 24.0, 4.0, 48.0);
        let consumed = ScaffoldContentInsets {
            top: false,
            ..ScaffoldContentInsets::default()
        }
        .filter(insets);
        assert_eq!(consumed, PaddingValues::new(4.0, 0.0, 4.0, 48.0));
        assert_eq!(
            ScaffoldContentInsets::none().filter(insets),
            PaddingValues::default()
        );
    }

    #[test]
    fn a_scaffold_states_no_surfaces_by_default() {
        assert_eq!(ScaffoldSpec::default().colors, ScaffoldColors::default());
        let colors = ScaffoldColors::uniform(Color(0.1, 0.1, 0.1, 1.0));
        assert_eq!(colors.background, colors.top_bar);
        assert_eq!(colors.background, colors.bottom_bar);
    }
}
