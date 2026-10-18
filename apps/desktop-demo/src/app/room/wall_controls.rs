//! Controls from the controls tab on the room's walls. Each control's card
//! lies on its wall under the wall's text, and the control's parts stand
//! out of the card as meshes inside the wall's content, which join the
//! room's scene: lit, casting their shadows and solid.

use cranpose_core::{key, MutableState};
use cranpose_ui::{composable, Box, BoxSpec, Modifier, Point, Size};

use crate::app::controls_ui::{
    rememberControl, ControlKind, ControlModel, STAGE_HEIGHT, WALL_CARD_WIDTH as CARD_WIDTH,
};

const CARD_GAP: f32 = 30.0;
const WALL_MARGIN: f32 = 48.0;
/// The smallest a card shrinks to fit its wall; a wall with less room shows
/// no cards.
const MIN_CARD_SCALE: f32 = 0.3;
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
/// whose size `head` holds; none when the wall has no room for them.
#[composable(no_skip)]
pub(super) fn WallControls(size: Size, head: MutableState<Size>, kinds: &'static [ControlKind]) {
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
                key(kind, move || WallControl(kind, spot));
            }
        },
    );
}

/// One control's card at `spot`, with the control standing out of it.
#[composable]
fn WallControl(kind: ControlKind, spot: CardSpot) {
    let control = rememberControl(kind);
    let at = Modifier::empty().absolute_offset(spot.at.x, spot.at.y);
    ControlModel(kind, control, at, spot.card());
}
