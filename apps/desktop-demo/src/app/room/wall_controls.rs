//! Controls from the controls tab on the room's walls. Each control's card
//! lies on its wall, and its stage stands out of the card into the room as
//! a volume, so the room's eye sees the control solid from wherever it
//! stands.

use std::{rc::Rc, sync::Arc};

use cranpose_core::key;
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
/// A card's height: its stage and the line of its name and value.
const CARD_HEIGHT: f32 = STAGE_HEIGHT + 64.0;
const CARD_GAP: f32 = 30.0;
const WALL_MARGIN: f32 = 48.0;
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
    controls: Rc<[(ControlKind, ControlHandle)]>,
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

    /// Each control with the top left corner of its card on the wall's
    /// content, in rows from the wall's bottom left corner up.
    fn cards(
        &self,
        shape: RoomShape,
    ) -> impl Iterator<Item = (ControlKind, ControlHandle, Point)> + '_ {
        let size = shape.wall_size(self.wall);
        let per_row = (((size.width - 2.0 * WALL_MARGIN + CARD_GAP) / (CARD_WIDTH + CARD_GAP))
            as usize)
            .max(1);
        self.controls
            .iter()
            .enumerate()
            .map(move |(index, (kind, control))| {
                let (row, column) = (index / per_row, index % per_row);
                let at = Point::new(
                    WALL_MARGIN + column as f32 * (CARD_WIDTH + CARD_GAP),
                    size.height
                        - WALL_MARGIN
                        - (row + 1) as f32 * CARD_HEIGHT
                        - row as f32 * CARD_GAP,
                );
                (*kind, control.clone(), at)
            })
    }
}

#[composable]
pub(super) fn rememberWallControls(wall: Wall, kinds: &'static [ControlKind]) -> WallControlSet {
    let controls = kinds
        .iter()
        .map(|&kind| (kind, key(kind, || rememberControl(kind))))
        .collect();
    WallControlSet { wall, controls }
}

/// The cards of `set` on its wall's content.
#[composable(no_skip)]
pub(super) fn WallCards(shape: RoomShape, set: WallControlSet) {
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            for (kind, control, at) in set.cards(shape) {
                key(kind, move || {
                    WallCard(
                        kind,
                        control,
                        Modifier::empty().absolute_offset(at.x, at.y).size(Size {
                            width: CARD_WIDTH,
                            height: CARD_HEIGHT,
                        }),
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
    let rise = WALL_STAGE_UNITS.1 * CARD_WIDTH / WALL_STAGE_UNITS.0;
    for (kind, control, at) in set.cards(shape) {
        let middle = point(at.x + CARD_WIDTH * 0.5, at.y + STAGE_HEIGHT * 0.5);
        let place = Mat4::from_axes(
            right * CARD_WIDTH,
            out * rise,
            down * STAGE_HEIGHT,
            middle + out * (STAGE_LIFT + rise * 0.5),
        );
        let cube = Arc::clone(&cube);
        key(kind, move || {
            ControlVolume(kind, control, (cube, place), CARD_WIDTH);
        });
    }
}
