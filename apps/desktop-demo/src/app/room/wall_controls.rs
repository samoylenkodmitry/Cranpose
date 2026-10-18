//! Controls from the controls tab on the room's walls. Each control's card
//! lies on its wall under the wall's text, and its stage stands out of the
//! card into the room as a volume, so the room's eye sees the control solid
//! from wherever it stands.

use std::{rc::Rc, sync::Arc};

use cranpose_core::{key, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable,
    space::{Mat4, Mesh, Vec3},
    Box, BoxSpec, Modifier, Point, Size,
};

use super::space::{RoomShape, Wall};
use crate::app::controls_ui::{
    rememberControl, ControlHandle, ControlKind, ControlVolume, WallCard, STAGE_HEIGHT,
    WALL_STAGE_UNITS,
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

/// The controls on one wall, and their state, which their cards on the
/// wall and their stages in the room share.
#[derive(Clone)]
pub(super) struct WallControlSet {
    wall: Wall,
    /// The size of the wall's text, which the cards go under.
    head: MutableState<Size>,
    controls: Rc<[(ControlKind, ControlHandle)]>,
}

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

impl PartialEq for WallControlSet {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.controls, &other.controls)
    }
}

impl WallControlSet {
    /// The wall the controls are on.
    pub(super) fn wall(&self) -> Wall {
        self.wall
    }

    /// The size of the wall's text, which the cards go under: the text
    /// reports it.
    pub(super) fn head(&self) -> MutableState<Size> {
        self.head
    }

    /// Each control with its card's spot on the wall's content, in rows
    /// under the wall's text from its left; none when the wall has no room
    /// for them.
    fn cards(
        &self,
        shape: RoomShape,
    ) -> impl Iterator<Item = (ControlKind, &ControlHandle, CardSpot)> + '_ {
        let size = shape.wall_size(self.wall);
        let top = self.head.get().height;
        let fit = fit_cards(size, top, self.controls.len());
        let (scale, per_row) = fit.unwrap_or((0.0, 1));
        let shown = if fit.is_some() {
            self.controls.len()
        } else {
            0
        };
        let card = CardSpot {
            at: Point::ZERO,
            scale,
        }
        .card();
        self.controls[..shown]
            .iter()
            .enumerate()
            .map(move |(index, (kind, control))| {
                let (row, column) = (index / per_row, index % per_row);
                let spot = CardSpot {
                    at: Point::new(
                        WALL_MARGIN + column as f32 * (card.width + CARD_GAP),
                        top + row as f32 * (card.height + CARD_GAP),
                    ),
                    scale,
                };
                (*kind, control, spot)
            })
    }
}

#[composable]
pub(super) fn rememberWallControls(wall: Wall, kinds: &'static [ControlKind]) -> WallControlSet {
    let head = rememberMutableStateOf(Size::default);
    let controls = kinds
        .iter()
        .map(|&kind| (kind, key(kind, || rememberControl(kind))))
        .collect();
    WallControlSet {
        wall,
        head,
        controls,
    }
}

/// The cards of `set` on its wall's content.
#[composable(no_skip)]
pub(super) fn WallCards(shape: RoomShape, set: WallControlSet) {
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            for (kind, _, spot) in set.cards(shape) {
                key(kind, move || {
                    WallCard(
                        Modifier::empty()
                            .absolute_offset(spot.at.x, spot.at.y)
                            .size(spot.card()),
                    );
                });
            }
        },
    );
}

/// The stages of `set`, each the unit cube `cube` standing out of its card
/// into the room.
#[composable(no_skip)]
pub(super) fn WallStages(shape: RoomShape, set: WallControlSet, cube: Arc<Mesh>) {
    let size = shape.wall_size(set.wall);
    let on_wall = shape.wall_transform(set.wall);
    let point =
        |x: f32, y: f32| on_wall.transform_point(Vec3::new(x / size.width, y / size.height, 0.0));
    let origin = point(0.0, 0.0);
    let (Some(right), Some(down)) = (
        (point(1.0, 0.0) - origin).normalized(),
        (point(0.0, 1.0) - origin).normalized(),
    ) else {
        return;
    };
    // The wall shows its content upright to an eye in the room, so the room
    // lies on the side `down` turns into `right` from.
    let out = down.cross(right);
    for (kind, control, spot) in set.cards(shape) {
        let stage = spot.card();
        let rise = WALL_STAGE_UNITS.1 * stage.width / WALL_STAGE_UNITS.0;
        let middle = point(
            spot.at.x + stage.width * 0.5,
            spot.at.y + stage.height * 0.5,
        );
        let place = Mat4::from_axes(
            right * stage.width,
            out * rise,
            down * stage.height,
            middle + out * (STAGE_LIFT + rise * 0.5),
        );
        let (cube, control) = (Arc::clone(&cube), control.clone());
        key(kind, move || {
            ControlVolume(kind, control, (cube, place), stage);
        });
    }
}
