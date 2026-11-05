// The plan of a room of the backrooms: its size, where its doorways open,
// and what waits in it, all picked from the room's id. A room measures whole
// units along each side, and a doorway, a writing or a lever takes the
// middle of one unit of a side. The floor lies at y = 0, y points down and
// the room spans x from 0 to its width and z from 0 to its depth.
//
// A room is entered by its door 0 from the room before it. The room behind
// any other door is new, so the rooms never end; the room behind the door
// the eye came in by is the room it left.

use cranpose_ui::space::{Mat4, Vec3};

pub(super) const UNIT: f32 = 300.0;
pub(super) const HEIGHT: f32 = 280.0;
pub(super) const DOOR_WIDTH: f32 = 120.0;
pub(super) const DOOR_HEIGHT: f32 = 220.0;
const DOWN: Vec3 = Vec3::new(0.0, 1.0, 0.0);

// A dice that rolls the same numbers for the same room.
struct Dice(u64);

impl Dice {
    fn roll(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix64(self.0)
    }

    fn below(&mut self, count: u32) -> u32 {
        u32::try_from(self.roll() % u64::from(count.max(1))).unwrap_or(0)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.roll() % 100 < percent
    }

    // A room's units along x and z: a corridor, a room or a hall, and
    // whether it is a hall.
    fn units(&mut self) -> ((u32, u32), bool) {
        match self.below(10) {
            0 | 1 => ((1, 3 + self.below(3)), false),
            2 => ((3 + self.below(3), 1), false),
            3..=7 => ((2 + self.below(2), 2 + self.below(2)), false),
            _ => ((4 + self.below(2), 4 + self.below(2)), true),
        }
    }

    // The doors of a room of `units`, taken from its `free` slots: the
    // entry on the south side, then one to three ways on.
    fn doors(&mut self, units: (u32, u32), free: &mut Vec<Slot>) -> Vec<Slot> {
        let entry = take(free, |slot| slot.side == Side::South, self);
        let exits = 1 + self.below(if units.0 * units.1 >= 4 { 3 } else { 2 });
        let mut doors = vec![entry.unwrap_or(Slot {
            side: Side::South,
            unit: 0,
        })];
        for _ in 0..exits {
            if let Some(slot) = take(free, |slot| slot.side != Side::South, self) {
                doors.push(slot);
            }
        }
        doors
    }

    fn mood(&mut self) -> Mood {
        match self.below(10) {
            0..=6 => Mood::Lit,
            7 | 8 => Mood::Dim,
            _ => Mood::Dark,
        }
    }

    // The pillars of a hall of `units`, at the corners of its units; none
    // stands in the middle, where a thing may stand.
    fn pillars(&mut self, units: (u32, u32)) -> Vec<(u32, u32)> {
        let middle = (units.0 / 2, units.1 / 2);
        (1..units.0)
            .flat_map(|x| (1..units.1).map(move |z| (x, z)))
            .filter(|&corner| corner != middle && self.chance(45))
            .collect()
    }

    fn thing(&mut self) -> Option<Thing> {
        self.chance(40).then(|| match self.below(3) {
            0 => Thing::Watcher,
            1 => Thing::Television,
            _ => Thing::Poltergeist,
        })
    }
}

fn mix64(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct RoomId(u64);

impl RoomId {
    pub(super) const FIRST: Self = Self(0x6261_636b_726f_6f6d);

    // The new room behind `door` of this one, entered by its door 0.
    fn beyond(self, door: usize) -> Self {
        let door = u64::try_from(door).unwrap_or(u64::MAX);
        Self(mix64(
            self.0 ^ door.wrapping_add(1).wrapping_mul(0xd6e8_feb8_6659_fd93),
        ))
    }

    // A number the room's lamps flicker by, small enough for an f32.
    pub(super) fn lamp_seed(self) -> u32 {
        u32::try_from(self.0 & 0x00ff_ffff).unwrap_or(0)
    }
}

// The room the eye stands in, and the room behind the door it came in by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Visit {
    pub(super) room: RoomId,
    pub(super) back: Option<Back>,
}

// The way back: the door of the eye's room it came in by, the room it left,
// and that room's door it went out by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Back {
    pub(super) door: usize,
    pub(super) room: RoomId,
    pub(super) its_door: usize,
}

impl Visit {
    pub(super) const START: Self = Self {
        room: RoomId::FIRST,
        back: None,
    };

    // The room behind `door` of the visited room, and the door of that room
    // the eye comes in by.
    pub(super) fn beyond(self, door: usize) -> (RoomId, usize) {
        match self.back {
            Some(back) if back.door == door => (back.room, back.its_door),
            _ => (self.room.beyond(door), 0),
        }
    }

    // The visit once the eye has walked through `door`.
    pub(super) fn through(self, door: usize) -> Self {
        let (room, entry) = self.beyond(door);
        Self {
            room,
            back: Some(Back {
                door: entry,
                room: self.room,
                its_door: door,
            }),
        }
    }
}

// A side of a room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    // z = 0.
    South,
    // x = width.
    East,
    // z = depth.
    North,
    // x = 0.
    West,
}

impl Side {
    const ALL: [Self; 4] = [Self::South, Self::East, Self::North, Self::West];

    // The way into the room from the side.
    pub(super) fn inward(self) -> Vec3 {
        match self {
            Self::South => Vec3::new(0.0, 0.0, 1.0),
            Self::East => Vec3::new(-1.0, 0.0, 0.0),
            Self::North => Vec3::new(0.0, 0.0, -1.0),
            Self::West => Vec3::new(1.0, 0.0, 0.0),
        }
    }

    // The way to the right of an eye in the room that faces the side.
    pub(super) fn right(self) -> Vec3 {
        DOWN.cross(-self.inward())
    }

    // Whether the side runs along x, rather than along z.
    fn along_x(self) -> bool {
        matches!(self, Self::South | Self::North)
    }

    // The point on the floor `along` the side from its start, which lies
    // at the smaller x or z, in a room of `size`.
    pub(super) fn point(self, along: f32, (width, depth): (f32, f32)) -> Vec3 {
        match self {
            Self::South => Vec3::new(along, 0.0, 0.0),
            Self::East => Vec3::new(width, 0.0, along),
            Self::North => Vec3::new(along, 0.0, depth),
            Self::West => Vec3::new(0.0, 0.0, along),
        }
    }
}

// The middle of one unit of a side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Slot {
    pub(super) side: Side,
    pub(super) unit: u32,
}

impl Slot {
    fn along(self) -> f32 {
        (self.unit as f32 + 0.5) * UNIT
    }

    // Places a flat thing of `width` facing into a room of `size` with its
    // middle on the slot, its top `top` above the floor and its face `out`
    // into the room: its content's x, y and z (into the wall) in the room.
    pub(super) fn frame(self, (width, top): (f32, f32), out: f32, size: (f32, f32)) -> Mat4 {
        let right = self.side.right();
        let middle = self.side.point(self.along(), size) + self.side.inward() * out;
        Mat4::from_axes(
            right,
            DOWN,
            -self.side.inward(),
            middle - right * (width * 0.5) + Vec3::new(0.0, -top, 0.0),
        )
    }
}

// A stretch of wall: on `side`, from `from` to `to` along it, from `top`
// down to `bottom` above the floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Panel {
    pub(super) side: Side,
    pub(super) from: f32,
    pub(super) to: f32,
    pub(super) top: f32,
    pub(super) bottom: f32,
}

impl Panel {
    // Places the unit plane on the panel, its content upright from inside
    // a room of `size`.
    pub(super) fn transform(self, size: (f32, f32)) -> Mat4 {
        let right = self.side.right();
        let along_axis = if self.side.along_x() {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };
        let left = if right.dot(along_axis) > 0.0 {
            self.from
        } else {
            self.to
        };
        let across = right * (self.to - self.from);
        let down = DOWN * (self.top - self.bottom);
        Mat4::from_axes(
            across,
            down,
            -self.side.inward(),
            self.side.point(left, size) + Vec3::new(0.0, -self.top, 0.0),
        )
    }
}

// How a room's lamps burn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mood {
    Lit,
    Dim,
    Dark,
}

// A thing that should not be in a room, standing in its middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Thing {
    // An eye that turns to watch the eye.
    Watcher,
    // A television that types what the entity says.
    Television,
    // Switches that flip by themselves.
    Poltergeist,
}

// What the entity wrote on a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Writing {
    pub(super) slot: Slot,
    pub(super) words: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Plan {
    pub(super) id: RoomId,
    // The units along x and along z.
    pub(super) units: (u32, u32),
    pub(super) doors: Vec<Slot>,
    pub(super) mood: Mood,
    // Corners of units inside the room that hold a pillar.
    pub(super) pillars: Vec<(u32, u32)>,
    pub(super) writings: Vec<Writing>,
    pub(super) thing: Option<Thing>,
    pub(super) lever: Option<Slot>,
    // Whether a figure stands in the room while the eye has not reached it.
    pub(super) figure: bool,
}

impl Plan {
    pub(super) fn of(id: RoomId) -> Self {
        if id == RoomId::FIRST {
            return Self::first();
        }
        let mut dice = Dice(id.0);
        let (units, hall) = dice.units();
        let mut free = Self::slots(units);
        let doors = dice.doors(units, &mut free);
        let mood = dice.mood();
        let pillars = if hall {
            dice.pillars(units)
        } else {
            Vec::new()
        };
        let writings = (0..1 + dice.below(2))
            .filter_map(|_| {
                let slot = take(&mut free, |_| true, &mut dice)?;
                let words = usize::try_from(dice.below(WORDS)).unwrap_or(0);
                Some(Writing { slot, words })
            })
            .collect();
        let roomy = units.0 >= 2 && units.1 >= 2;
        let thing = if roomy { dice.thing() } else { None };
        let lever = if dice.chance(15) {
            take(&mut free, |_| true, &mut dice)
        } else {
            None
        };
        Self {
            id,
            units,
            doors,
            mood,
            pillars,
            writings,
            thing,
            lever,
            figure: roomy && dice.chance(25),
        }
    }

    // The room the eye starts in: a door straight ahead of where it stands
    // and one to its right, and a word of welcome on its left.
    fn first() -> Self {
        let slot = |side, unit| Slot { side, unit };
        Self {
            id: RoomId::FIRST,
            units: (3, 3),
            doors: vec![
                slot(Side::South, 1),
                slot(Side::North, 1),
                slot(Side::East, 1),
            ],
            mood: Mood::Lit,
            pillars: Vec::new(),
            writings: vec![Writing {
                slot: slot(Side::West, 1),
                words: 0,
            }],
            thing: None,
            lever: None,
            figure: false,
        }
    }

    // Every slot of a room of `units`.
    fn slots(units: (u32, u32)) -> Vec<Slot> {
        Side::ALL
            .into_iter()
            .flat_map(|side| {
                let count = if side.along_x() { units.0 } else { units.1 };
                (0..count).map(move |unit| Slot { side, unit })
            })
            .collect()
    }

    pub(super) fn size(&self) -> (f32, f32) {
        (self.units.0 as f32 * UNIT, self.units.1 as f32 * UNIT)
    }

    // Where the eye stands at the start: in front of the entry, looking
    // into the room.
    pub(super) fn start(&self) -> Vec3 {
        let size = self.size();
        let entry = self.doors.first().copied().unwrap_or(Slot {
            side: Side::South,
            unit: 0,
        });
        entry.side.point(entry.along(), size) + entry.side.inward() * (UNIT * 0.5)
    }

    // The middle of the floor, where a thing stands.
    pub(super) fn middle(&self) -> Vec3 {
        let (width, depth) = self.size();
        Vec3::new(width * 0.5, 0.0, depth * 0.5)
    }

    // Where a figure stands: in the middle of the unit across the room from
    // the entry, which lies on the south side.
    pub(super) fn far_spot(&self) -> Vec3 {
        let entry = self.doors.first().map_or(0, |door| door.unit);
        let across = self.units.0.saturating_sub(entry + 1);
        Vec3::new(
            (across as f32 + 0.5) * UNIT,
            0.0,
            (self.units.1 as f32 - 0.5) * UNIT,
        )
    }

    // The middle of `door`, halfway up it.
    pub(super) fn door_middle(&self, door: usize) -> Option<Vec3> {
        let slot = self.doors.get(door)?;
        Some(slot.side.point(slot.along(), self.size()) + Vec3::new(0.0, -DOOR_HEIGHT * 0.5, 0.0))
    }

    // The doorway's rect on its wall, as a doorway's node lays it out: its
    // content's x, y and z (beyond the wall) in the room.
    pub(super) fn door_frame(&self, door: usize) -> Option<Mat4> {
        let slot = self.doors.get(door)?;
        Some(slot.frame((DOOR_WIDTH, DOOR_HEIGHT), 0.0, self.size()))
    }

    // The door nearest `point`.
    pub(super) fn nearest_door(&self, point: Vec3) -> Option<usize> {
        (0..self.doors.len()).min_by(|&a, &b| {
            let distance = |door| {
                self.door_middle(door)
                    .map_or(f32::INFINITY, |middle| (middle - point).length())
            };
            distance(a).total_cmp(&distance(b))
        })
    }

    // The stretches of wall around the doorways: a panel from each door to
    // the next along each side, and one over each door.
    pub(super) fn panels(&self) -> Vec<Panel> {
        let mut panels = Vec::new();
        for side in Side::ALL {
            let length = if side.along_x() {
                self.size().0
            } else {
                self.size().1
            };
            let mut doors: Vec<f32> = self
                .doors
                .iter()
                .filter(|slot| slot.side == side)
                .map(|slot| slot.along())
                .collect();
            doors.sort_by(f32::total_cmp);
            let full = |from, to| Panel {
                side,
                from,
                to,
                top: HEIGHT,
                bottom: 0.0,
            };
            let mut from = 0.0;
            for middle in doors {
                let (left, right) = (middle - DOOR_WIDTH * 0.5, middle + DOOR_WIDTH * 0.5);
                panels.push(full(from, left));
                panels.push(Panel {
                    bottom: DOOR_HEIGHT,
                    ..full(left, right)
                });
                from = right;
            }
            panels.push(full(from, length));
        }
        panels
    }

    // The pillars' faces.
    pub(super) fn pillar_faces(&self) -> Vec<Mat4> {
        const HALF: f32 = 30.0;
        self.pillars
            .iter()
            .flat_map(|&(x, z)| {
                let middle = Vec3::new(x as f32 * UNIT, 0.0, z as f32 * UNIT);
                Side::ALL.into_iter().map(move |side| {
                    // A face looks out of the pillar, the way a wall on the
                    // opposite side looks into a room.
                    let out = -side.inward();
                    let right = DOWN.cross(-out);
                    Mat4::from_axes(
                        right * (2.0 * HALF),
                        DOWN * HEIGHT,
                        -out,
                        middle + out * HALF - right * HALF + Vec3::new(0.0, -HEIGHT, 0.0),
                    )
                })
            })
            .collect()
    }
}

// How many things the entity can write.
pub(super) const WORDS: u32 = 16;

// A slot of `free` that `fits`, taken out of it.
fn take(free: &mut Vec<Slot>, fits: impl Fn(&Slot) -> bool, dice: &mut Dice) -> Option<Slot> {
    let fitting: Vec<usize> = (0..free.len())
        .filter(|&index| fits(&free[index]))
        .collect();
    let count = u32::try_from(fitting.len()).unwrap_or(u32::MAX);
    let pick = usize::try_from(dice.below(count)).unwrap_or(0);
    let index = *fitting.get(pick)?;
    Some(free.swap_remove(index))
}

#[cfg(test)]
#[path = "../tests/backrooms_plan_tests.rs"]
mod tests;
