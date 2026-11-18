// Where a menu stands against its anchor, which side it grows toward, the
// order of its rows and the shape its morph starts from.

use cranpose_ui::{local_safe_area_insets, widgets::local_popup_viewport};
use cranpose_ui_graphics::Rect;

// Menus and lifted previews keep this far inside the safe area.
const MARGIN: f32 = 16.0;
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
    /// `gap` points below the anchor, or above it when the safe area has no
    /// room below: the iOS context menu beside its lifted preview. The menu
    /// centres on the anchor when it fits, or else lines up with the anchor's
    /// edge on the side that has no room. Its rows keep their order.
    BesideAnchor {
        /// The space between the anchor and the menu, in points.
        gap: f32,
    },
}

// Where menus and lifted previews may stand in the popup viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuArea {
    // `MARGIN` inside the safe area.
    bounds: Rect,
    // The viewport's vertical middle.
    middle: f32,
}

impl MenuArea {
    // The area of the popup viewport, once the host has measured it.
    pub(super) fn current() -> Option<Self> {
        let viewport = local_popup_viewport().current().get();
        let safe = local_safe_area_insets().current();
        (viewport.width > 0.0 && viewport.height > 0.0).then_some(Self {
            bounds: Rect {
                x: safe.left + MARGIN,
                y: safe.top + MARGIN,
                width: viewport.width - safe.left - safe.right - 2.0 * MARGIN,
                height: viewport.height - safe.top - safe.bottom - 2.0 * MARGIN,
            },
            middle: viewport.height * 0.5,
        })
    }

    // Where `rect` stands once moved inside the area, at the area's start
    // on an axis where it does not fit.
    pub(super) fn fit(self, rect: Rect) -> Rect {
        let Rect {
            x,
            y,
            width,
            height,
        } = self.bounds;
        Rect {
            x: fit(rect.x, rect.width, x, x + width),
            y: fit(rect.y, rect.height, y, y + height),
            ..rect
        }
    }
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
        area: Option<MenuArea>,
        width: f32,
        height: f32,
    ) -> MenuPlacement {
        let width = area.map_or(width, |area| width.min(area.bounds.width.max(1.0)));
        let centred = anchor.x + (anchor.width - width) * 0.5;
        match self {
            Self::OverAnchor => {
                let opens_up =
                    area.is_some_and(|area| anchor.y + anchor.height * 0.5 > area.middle);
                let top = if opens_up {
                    anchor.y + anchor.height - height
                } else {
                    anchor.y
                };
                MenuPlacement {
                    bounds: fit_x(area, centred, top, width, height),
                    grows_up: opens_up,
                    rows_up: opens_up,
                    source: anchor,
                }
            }
            Self::BesideAnchor { gap } => {
                let below = anchor.y + anchor.height + gap;
                let above = anchor.y - gap - height;
                let grows_up = area.is_some_and(|area| {
                    below + height > area.bounds.y + area.bounds.height && above >= area.bounds.y
                });
                let (top, edge) = if grows_up {
                    (above, above + height)
                } else {
                    (below, below)
                };
                let left = area.map_or(centred, |area| beside_left(area, anchor, centred, width));
                let bounds = fit_x(area, left, top, width, height);
                MenuPlacement {
                    bounds,
                    grows_up,
                    rows_up: false,
                    source: Rect {
                        x: bounds.x + (width - BESIDE_SOURCE) * 0.5,
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
// area, or else lines up with the anchor's edge on the side that ran out of
// room.
fn beside_left(area: MenuArea, anchor: Rect, centred: f32, width: f32) -> f32 {
    if centred < area.bounds.x {
        anchor.x
    } else if centred + width > area.bounds.x + area.bounds.width {
        anchor.x + anchor.width - width
    } else {
        centred
    }
}

// The menu's bounds at `left` and `top`, moved inside the area across.
fn fit_x(area: Option<MenuArea>, left: f32, top: f32, width: f32, height: f32) -> Rect {
    let x = area.map_or(left, |area| {
        fit(
            left,
            width,
            area.bounds.x,
            area.bounds.x + area.bounds.width,
        )
    });
    Rect {
        x,
        y: top,
        width,
        height,
    }
}

// The start of an extent of `extent` from `start`, moved inside `low..high`,
// or `low` when it does not fit.
fn fit(start: f32, extent: f32, low: f32, high: f32) -> f32 {
    if high - low < extent {
        low
    } else {
        start.clamp(low, high - extent)
    }
}
