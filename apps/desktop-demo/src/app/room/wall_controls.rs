//! Controls from the controls tab on the room's walls. Each control's card
//! lies on its wall under the wall's text, and its stage stands out of the
//! card as a volume: a mesh surface inside the wall's content, which joins
//! the room's scene, so the room's eye sees the control solid from wherever
//! it stands.

use std::sync::Arc;

use cranpose_core::{key, MutableState};
use cranpose_ui::{
    composable,
    space::{Mat4, Mesh, Vec3},
    Box, BoxSpec, Modifier, Point, Size,
};

use crate::app::controls_ui::{
    rememberControl, ControlKind, ControlVolume, WallCard, STAGE_HEIGHT, WALL_STAGE_UNITS,
};

const CARD_WIDTH: f32 = 300.0;
const CARD_GAP: f32 = 30.0;
const WALL_MARGIN: f32 = 48.0;
/// The smallest a card shrinks to fit its wall; a wall with less room shows
/// no cards.
const MIN_CARD_SCALE: f32 = 0.3;
/// How far in front of its wall a stage's ground lies, so the wall does not
/// show through it.
const STAGE_LIFT: f32 = 1.0;

/// The controls on the far wall, under its screen.
pub(super) const NORTH_CONTROLS: &[ControlKind] = &[ControlKind::VolumeDial, ControlKind::Toggle];
/// The controls on the wall behind the screen's eye.
pub(super) const SOUTH_CONTROLS: &[ControlKind] = &[
    ControlKind::Slider,
    ControlKind::LeverSwitch,
    ControlKind::PushButton,
    ControlKind::Checkmark,
];

/// Where a control's card lies on its wall's content: its top left corner,
/// and its scale against a card of [`CARD_WIDTH`] by [`STAGE_HEIGHT`].
#[derive(Clone, Copy, PartialEq)]
struct CardSpot {
    at: Point,
    scale: f32,
}

impl CardSpot {
    /// The card's size, which its stage fills.
    fn card(self) -> Size {
        Size {
            width: CARD_WIDTH * self.scale,
            height: STAGE_HEIGHT * self.scale,
        }
    }

    /// Where the stage's cube stands in the frame of the card's node, which
    /// layout placed on the wall: x along the card's width, y out of the
    /// wall, toward the room, and z down the wall, its ground just in front
    /// of the card.
    fn stage_place(self) -> Mat4 {
        let card = self.card();
        let rise = WALL_STAGE_UNITS.1 * card.width / WALL_STAGE_UNITS.0;
        Mat4::from_axes(
            Vec3::new(card.width, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -rise),
            Vec3::new(0.0, card.height, 0.0),
            Vec3::new(
                card.width * 0.5,
                card.height * 0.5,
                -(STAGE_LIFT + rise * 0.5),
            ),
        )
    }
}

/// The largest scale, at most one, at which `count` cards fit in rows on a
/// wall of `size` under its text, `head` tall, and how many cards a row
/// takes at that scale; `None` when they fit only below
/// [`MIN_CARD_SCALE`].
fn fit_cards(size: Size, head: f32, count: usize) -> Option<(f32, usize)> {
    let width = size.width - 2.0 * WALL_MARGIN;
    let height = size.height - head - WALL_MARGIN;
    (1..=count)
        .map(|per_row| {
            let rows = count.div_ceil(per_row);
            let across = (width - (per_row - 1) as f32 * CARD_GAP) / (per_row as f32 * CARD_WIDTH);
            let down = (height - (rows - 1) as f32 * CARD_GAP) / (rows as f32 * STAGE_HEIGHT);
            (across.min(down).min(1.0), per_row)
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|(scale, _)| *scale >= MIN_CARD_SCALE)
}

/// The controls `kinds` on a wall of `size`, in rows under the wall's text,
/// whose size `head` holds; none when the wall has no room for them. Each
/// stage is the unit cube `cube`.
#[composable(no_skip)]
pub(super) fn WallControls(
    size: Size,
    head: MutableState<Size>,
    (kinds, cube): (&'static [ControlKind], Arc<Mesh>),
) {
    let top = head.get().height;
    let Some((scale, per_row)) = fit_cards(size, top, kinds.len()) else {
        return;
    };
    let card = CardSpot {
        at: Point::ZERO,
        scale,
    }
    .card();
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            for (index, &kind) in kinds.iter().enumerate() {
                let (row, column) = (index / per_row, index % per_row);
                let spot = CardSpot {
                    at: Point::new(
                        WALL_MARGIN + column as f32 * (card.width + CARD_GAP),
                        top + row as f32 * (card.height + CARD_GAP),
                    ),
                    scale,
                };
                let cube = Arc::clone(&cube);
                key(kind, move || WallControl(kind, spot, cube));
            }
        },
    );
}

/// One control's card at `spot`, with its stage standing out of it.
#[composable]
fn WallControl(kind: ControlKind, spot: CardSpot, cube: Arc<Mesh>) {
    let control = rememberControl(kind);
    let at = Modifier::empty().absolute_offset(spot.at.x, spot.at.y);
    WallCard(at.clone().size(spot.card()));
    ControlVolume(kind, control, at, (cube, spot.stage_place(), spot.card()));
}
