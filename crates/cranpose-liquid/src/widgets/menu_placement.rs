// Where a menu stands against its anchor, which side it grows toward, the
// order of its rows and the shape its morph starts from.

use cranpose_ui::Size;
use cranpose_ui_graphics::Rect;

// The menu keeps this far from the sides of the viewport.
const VIEWPORT_MARGIN: f32 = 16.0;
// A menu beside its anchor grows out of a capsule this wide and tall at its
// edge that faces the anchor.
const BESIDE_SOURCE: f32 = 44.0;

/// Where a [`super::LiquidMenu`] stands against its anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LiquidMenuPlacement {
    /// Over the anchor, which the menu grows out of: the iOS pull-down menu.
    /// It opens down from the anchor's top edge, or up from its bottom edge
    /// when the anchor sits in the lower half of the viewport, and lists its
    /// rows from that edge.
    #[default]
    OverAnchor,
    /// `gap` points below the anchor, or above it when the viewport has no
    /// room below: the iOS context menu beside its lifted preview. The menu
    /// centres on the anchor when it fits, or else lines up with the anchor's
    /// edge on the side that has no room. Its rows keep their order.
    BesideAnchor {
        /// The space between the anchor and the menu, in points.
        gap: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuPlacement {
    pub(super) bounds: Rect,
    // The menu grows from its bottom edge.
    pub(super) grows_up: bool,
    // The rows list from the bottom edge.
    pub(super) rows_up: bool,
    // The rectangle the morph starts from, in window coordinates.
    pub(super) source: Rect,
}

impl LiquidMenuPlacement {
    pub(super) fn place(
        self,
        anchor: Rect,
        viewport: Size,
        width: f32,
        height: f32,
    ) -> MenuPlacement {
        let width = if viewport.width > 0.0 {
            width.min((viewport.width - 2.0 * VIEWPORT_MARGIN).max(1.0))
        } else {
            width
        };
        let centred = anchor.x + (anchor.width - width) * 0.5;
        match self {
            Self::OverAnchor => {
                let opens_up =
                    viewport.height > 0.0 && anchor.y + anchor.height * 0.5 > viewport.height * 0.5;
                let top = if opens_up {
                    anchor.y + anchor.height - height
                } else {
                    anchor.y
                };
                MenuPlacement {
                    bounds: Rect {
                        x: clamp_to_viewport(centred, width, viewport),
                        y: top,
                        width,
                        height,
                    },
                    grows_up: opens_up,
                    rows_up: opens_up,
                    source: anchor,
                }
            }
            Self::BesideAnchor { gap } => {
                let below = anchor.y + anchor.height + gap;
                let grows_up = viewport.height > 0.0
                    && below + height > viewport.height - VIEWPORT_MARGIN
                    && anchor.y - gap - height >= VIEWPORT_MARGIN;
                let top = if grows_up {
                    anchor.y - gap - height
                } else {
                    below
                };
                let left = beside_left(anchor, centred, width, viewport);
                let edge = if grows_up { top + height } else { top };
                MenuPlacement {
                    bounds: Rect {
                        x: left,
                        y: top,
                        width,
                        height,
                    },
                    grows_up,
                    rows_up: false,
                    source: Rect {
                        x: left + (width - BESIDE_SOURCE) * 0.5,
                        y: edge - BESIDE_SOURCE * 0.5,
                        width: BESIDE_SOURCE,
                        height: BESIDE_SOURCE,
                    },
                }
            }
        }
    }
}

// A menu beside its anchor centres on it when that keeps it inside the
// viewport's margins, or else lines up with the anchor's edge on the side
// that ran out of room.
fn beside_left(anchor: Rect, centred: f32, width: f32, viewport: Size) -> f32 {
    if viewport.width <= 0.0 {
        return centred;
    }
    let left = if centred < VIEWPORT_MARGIN {
        anchor.x
    } else if centred + width > viewport.width - VIEWPORT_MARGIN {
        anchor.x + anchor.width - width
    } else {
        centred
    };
    clamp_to_viewport(left, width, viewport)
}

fn clamp_to_viewport(left: f32, width: f32, viewport: Size) -> f32 {
    if viewport.width > 0.0 {
        left.clamp(
            VIEWPORT_MARGIN,
            (viewport.width - width - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN),
        )
    } else {
        left
    }
}
