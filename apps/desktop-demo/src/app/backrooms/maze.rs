// The maze of the Backrooms tab: an endless grid of square cells on the
// floor plane, each side of which holds nothing, a wall or a wall with a
// doorway, picked by hashing where the side lies. Walls run in lines a few
// cells long, so the maze has corridors and rooms. The level's shader
// computes the same hashes from the constants that `wgsl_constants` writes,
// so the eye bumps into the walls it sees. The floor lies at y = 0, y points
// down, and the ceiling lies at y = -CEILING.

use cranpose_core::collections::map::HashSet;
use cranpose_ui::space::Vec3;

pub(super) const CELL: f32 = 400.0;
pub(super) const CEILING: f32 = 280.0;
// How far a wall reaches from the line of its side on each side of it.
const WALL_HALF: f32 = 8.0;
const DOOR_HALF_WIDTH: f32 = 55.0;
const DOOR_HEIGHT: f32 = 212.0;
// How far a doorway's middle lies from the middle of its side at most.
const DOOR_SHIFT: f32 = 60.0;
const PILLAR_HALF: f32 = 32.0;
// A line of sides is walled or open in runs of `1 << RUN_SHIFT` sides.
const RUN_SHIFT: u32 = 2;
// Of the hash's range: the walled runs, the open gaps and doorways in them,
// the cells with a pillar, the dark blocks of 4 by 4 cells, the dead and
// flickering lamps, and the flickering lamps of the dark blocks.
const WALL_RUN: u32 = share(55);
const GAP: u32 = share(12);
const GAP_OR_DOOR: u32 = share(36);
const PILLAR: u32 = share(5);
const DARK_BLOCK: u32 = share(16);
const DEAD: u32 = share(10);
const DEAD_OR_FLICKERING: u32 = share(17);
const FLICKERING_IN_DARK: u32 = share(20);
const SIDE_ACROSS_X: u32 = 0x51ed_270b;
const SIDE_ACROSS_Z: u32 = 0x2c1b_3c6d;
const RUN_SALT: u32 = 0x68e3_1da4;
const PICK_SALT: u32 = 0xb529_7a4d;
const CELL_SALT: u32 = 0xcc9e_2d51;
const DARK_SALT: u32 = 0xe654_6b64;
// A cell the eye starts in reaches at least this many cells, so the eye
// does not start shut in a closed room, and looks down a straight way at
// least this many cells long.
const SPAWN_REACH: usize = 400;
const SPAWN_VIEW: u32 = 3;
const SPAWN_SEARCH: i32 = 24;

// `percent` of the 32-bit hash's range.
const fn share(percent: u32) -> u32 {
    u32::MAX / 100 * percent
}

fn mix32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

fn hash(a: i32, b: i32, salt: u32) -> u32 {
    mix32(a.cast_unsigned().wrapping_mul(0x9e37_79b1) ^ mix32(b.cast_unsigned() ^ salt))
}

// The WGSL constants the level's shader reads, from the ones above.
pub(super) fn wgsl_constants() -> String {
    let floats = [
        ("CELL", CELL),
        ("CEILING", CEILING),
        ("WALL_HALF", WALL_HALF),
        ("DOOR_HALF_WIDTH", DOOR_HALF_WIDTH),
        ("DOOR_HEIGHT", DOOR_HEIGHT),
        ("DOOR_SHIFT", DOOR_SHIFT),
        ("PILLAR_HALF", PILLAR_HALF),
    ]
    .map(|(name, value)| format!("const {name}: f32 = {value:?};\n"));
    let integers = [
        ("RUN_SHIFT", RUN_SHIFT),
        ("RUN_MASK", (1 << RUN_SHIFT) - 1),
        ("WALL_RUN", WALL_RUN),
        ("GAP", GAP),
        ("GAP_OR_DOOR", GAP_OR_DOOR),
        ("PILLAR", PILLAR),
        ("DARK_BLOCK", DARK_BLOCK),
        ("DEAD", DEAD),
        ("DEAD_OR_FLICKERING", DEAD_OR_FLICKERING),
        ("FLICKERING_IN_DARK", FLICKERING_IN_DARK),
        ("SIDE_ACROSS_X", SIDE_ACROSS_X),
        ("SIDE_ACROSS_Z", SIDE_ACROSS_Z),
        ("RUN_SALT", RUN_SALT),
        ("PICK_SALT", PICK_SALT),
        ("CELL_SALT", CELL_SALT),
        ("DARK_SALT", DARK_SALT),
    ]
    .map(|(name, value)| format!("const {name}: u32 = {value}u;\n"));
    floats.into_iter().chain(integers).collect()
}

// What a cell's side holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Side {
    Open,
    Wall,
    // A wall with a doorway whose middle lies `middle` from the side's start.
    Door { middle: f32 },
}

// Which way the line of a side runs: across x, at x = line * CELL and
// along z, or across z.
#[derive(Clone, Copy)]
enum Across {
    X,
    Z,
}

impl Across {
    fn salt(self) -> u32 {
        match self {
            Self::X => SIDE_ACROSS_X,
            Self::Z => SIDE_ACROSS_Z,
        }
    }
}

impl Side {
    // The side on line `line` across `across`, `along` sides from the
    // line's origin.
    fn on(line: i32, along: i32, across: Across) -> Self {
        let salt = across.salt();
        let phase = (hash(line, 0, salt) & ((1 << RUN_SHIFT) - 1)) as i32;
        if hash(line, (along + phase) >> RUN_SHIFT, salt ^ RUN_SALT) >= WALL_RUN {
            return Self::Open;
        }
        let pick = hash(line, along, salt ^ PICK_SALT);
        if pick < GAP {
            Self::Open
        } else if pick < GAP_OR_DOOR {
            let shift = (pick & 0xffff) as f32 * (2.0 / 65535.0) - 1.0;
            Self::Door {
                middle: CELL * 0.5 + DOOR_SHIFT * shift,
            }
        } else {
            Self::Wall
        }
    }

    // Whether the eye walks through the side, and whether it sees through.
    fn passes(self, sight: Sight) -> bool {
        match (self, sight) {
            (Self::Open, _) | (Self::Door { .. }, Sight::Walk) => true,
            (Self::Wall, _) | (Self::Door { .. }, Sight::Look) => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Sight {
    Walk,
    Look,
}

// A way along the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Toward {
    // +z, the way the eye looks at a yaw of zero.
    North,
    // +x.
    East,
    South,
    West,
}

impl Toward {
    pub(super) const ALL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    // The eye's yaw that looks this way.
    pub(super) fn yaw(self) -> f32 {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            Self::North => 0.0,
            Self::East => FRAC_PI_2,
            Self::South => PI,
            Self::West => -FRAC_PI_2,
        }
    }

    // The unit step along the floor.
    pub(super) fn step(self) -> (f32, f32) {
        match self {
            Self::North => (0.0, 1.0),
            Self::East => (1.0, 0.0),
            Self::South => (0.0, -1.0),
            Self::West => (-1.0, 0.0),
        }
    }
}

// The kind of lamp in a cell's ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lamp {
    // A pillar stands where the lamp would hang.
    None,
    Dead,
    Flickering,
    Lit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Cell {
    pub(super) x: i32,
    pub(super) z: i32,
}

impl Cell {
    // The middle of the cell's floor, measured from the corner of `from`.
    pub(super) fn middle_from(self, from: Self) -> Vec3 {
        Vec3::new(
            ((self.x - from.x) as f32 + 0.5) * CELL,
            0.0,
            ((self.z - from.z) as f32 + 0.5) * CELL,
        )
    }

    pub(super) fn next(self, toward: Toward) -> Self {
        match toward {
            Toward::North => Self {
                z: self.z + 1,
                ..self
            },
            Toward::East => Self {
                x: self.x + 1,
                ..self
            },
            Toward::South => Self {
                z: self.z - 1,
                ..self
            },
            Toward::West => Self {
                x: self.x - 1,
                ..self
            },
        }
    }

    pub(super) fn side(self, toward: Toward) -> Side {
        match toward {
            Toward::North => Side::on(self.z + 1, self.x, Across::Z),
            Toward::East => Side::on(self.x + 1, self.z, Across::X),
            Toward::South => Side::on(self.z, self.x, Across::Z),
            Toward::West => Side::on(self.x, self.z, Across::X),
        }
    }

    // The cell's hash: its value picks a pillar, and its bits turned half
    // way round pick the lamp.
    fn pick(self) -> u32 {
        hash(self.x, self.z, CELL_SALT)
    }

    pub(super) fn has_pillar(self) -> bool {
        self.pick() < PILLAR
    }

    pub(super) fn lamp(self) -> Lamp {
        let pick = self.pick();
        if pick < PILLAR {
            return Lamp::None;
        }
        let pick = pick.rotate_left(16);
        if hash(self.x >> 2, self.z >> 2, DARK_SALT) < DARK_BLOCK {
            return if pick < FLICKERING_IN_DARK {
                Lamp::Flickering
            } else {
                Lamp::Dead
            };
        }
        if pick < DEAD {
            Lamp::Dead
        } else if pick < DEAD_OR_FLICKERING {
            Lamp::Flickering
        } else {
            Lamp::Lit
        }
    }

    // How many cells lie past this one in a straight line toward `toward`
    // that the eye sees down, up to `most`: a wall, a doorway or a pillar
    // ends the line.
    pub(super) fn open_line(self, toward: Toward, most: u32) -> u32 {
        let mut cell = self;
        for count in 0..most {
            let next = cell.next(toward);
            if !cell.side(toward).passes(Sight::Look) || next.has_pillar() {
                return count;
            }
            cell = next;
        }
        most
    }

    // Whether the eye walks from this cell to at least `enough` others.
    fn reaches(self, enough: usize) -> bool {
        let mut seen = HashSet::default();
        seen.insert(self);
        let mut queue = vec![self];
        while let Some(cell) = queue.pop() {
            for toward in Toward::ALL {
                let next = cell.next(toward);
                if cell.side(toward).passes(Sight::Walk) && seen.insert(next) {
                    if seen.len() >= enough {
                        return true;
                    }
                    queue.push(next);
                }
            }
        }
        false
    }
}

// The cell the eye starts in: a lit one near the origin with a long
// straight way out, from which it walks far, so it does not start shut in a
// closed room.
pub(super) fn spawn() -> Cell {
    (0..=SPAWN_SEARCH)
        .flat_map(|ring| {
            (-ring..=ring).flat_map(move |x| {
                (-ring..=ring)
                    .filter(move |z| x.abs() == ring || z.abs() == ring)
                    .map(move |z| Cell { x, z })
            })
        })
        .find(|cell| {
            cell.lamp() == Lamp::Lit
                && Toward::ALL
                    .into_iter()
                    .any(|toward| cell.open_line(toward, SPAWN_VIEW) == SPAWN_VIEW)
                && cell.reaches(SPAWN_REACH)
        })
        .unwrap_or(Cell { x: 0, z: 0 })
}

// A box on the floor plane that the eye cannot enter: a stretch of wall or
// a pillar.
#[derive(Clone, Copy, Debug)]
struct Block {
    low: (f32, f32),
    high: (f32, f32),
}

impl Block {
    // The blocks a side makes, measured from a cell's corner: none, a wall,
    // or the wall on each side of a doorway. The side lies across `across`,
    // on the line `lines` lines from the cell's corner, and starts `alongs`
    // sides along it.
    fn of(side: Side, (lines, alongs): (i32, i32), across: Across) -> [Option<Self>; 2] {
        let edge = lines as f32 * CELL;
        let start = alongs as f32 * CELL;
        let span = |from: f32, to: f32| {
            let (low, high) = ((edge - WALL_HALF, from), (edge + WALL_HALF, to));
            Some(match across {
                Across::X => Self { low, high },
                Across::Z => Self {
                    low: (low.1, low.0),
                    high: (high.1, high.0),
                },
            })
        };
        match side {
            Side::Open => [None, None],
            Side::Wall => [span(start, start + CELL), None],
            Side::Door { middle } => [
                span(start, start + middle - DOOR_HALF_WIDTH),
                span(start + middle + DOOR_HALF_WIDTH, start + CELL),
            ],
        }
    }

    // `at` moved the shortest way out to `radius` from the block.
    fn push(self, at: Vec3, radius: f32) -> Vec3 {
        let nearest = (
            at.x.clamp(self.low.0, self.high.0),
            at.z.clamp(self.low.1, self.high.1),
        );
        let (dx, dz) = (at.x - nearest.0, at.z - nearest.1);
        let distance = (dx * dx + dz * dz).sqrt();
        if distance >= radius {
            return at;
        }
        if distance > 1.0e-4 {
            let out = (radius - distance) / distance;
            return Vec3::new(at.x + dx * out, at.y, at.z + dz * out);
        }
        // Inside: out through the nearest face.
        let exits = [
            (self.low.0 - radius - at.x, 0.0),
            (self.high.0 + radius - at.x, 0.0),
            (0.0, self.low.1 - radius - at.z),
            (0.0, self.high.1 + radius - at.z),
        ];
        let (x, z) = exits
            .into_iter()
            .min_by(|a, b| (a.0.abs() + a.1.abs()).total_cmp(&(b.0.abs() + b.1.abs())))
            .unwrap_or((0.0, 0.0));
        Vec3::new(at.x + x, at.y, at.z + z)
    }
}

// A place in the maze: a cell, and a point measured from the cell's low
// corner, so the numbers stay small however far the eye walks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Spot {
    pub(super) cell: Cell,
    pub(super) at: Vec3,
}

impl Spot {
    // The same place, measured from the corner of the cell it lies in. The
    // point lies no farther than a cell out of its cell.
    fn rehomed(self) -> Self {
        let (x, cells_x) = home(self.at.x);
        let (z, cells_z) = home(self.at.z);
        Self {
            cell: Cell {
                x: self.cell.x + cells_x,
                z: self.cell.z + cells_z,
            },
            at: Vec3::new(x, self.at.y, z),
        }
    }
}

// A coordinate of a point no farther than a cell out of its cell, measured
// from the cell it lies in, and how many cells on that cell lies.
fn home(at: f32) -> (f32, i32) {
    if at < 0.0 {
        (at + CELL, -1)
    } else if at >= CELL {
        (at - CELL, 1)
    } else {
        (at, 0)
    }
}

// The most pieces a step is walked in.
const MOST_PIECES: f32 = 64.0;

// Where an eye of `radius` that stands at `from` ends after it walks
// `step`: walls and pillars stop it and it slides along them, but it walks
// through doorways. The step is taken in pieces no longer than half the
// radius, so no piece passes through a wall.
pub(super) fn walk(from: Spot, step: Vec3, radius: f32) -> Spot {
    let length = (step.x * step.x + step.z * step.z).sqrt();
    if length <= 0.0 {
        return from;
    }
    let (along_x, along_z) = (step.x / length, step.z / length);
    let most = radius * 0.5;
    let mut left = length.min(most * MOST_PIECES);
    let mut spot = from;
    while left > 0.0 {
        let piece = left.min(most);
        left -= piece;
        spot.at.x += along_x * piece;
        spot.at.z += along_z * piece;
        spot = push_out(spot.rehomed(), radius);
    }
    spot
}

// `spot` moved out of the walls and the pillar around its cell, twice over,
// so a push out of one wall into another in a corner is pushed back out.
fn push_out(spot: Spot, radius: f32) -> Spot {
    let mut spot = spot;
    for _ in 0..2 {
        let cell = spot.cell;
        for (across, first_line, middle) in
            [(Across::X, cell.x, cell.z), (Across::Z, cell.z, cell.x)]
        {
            for line in [first_line, first_line + 1] {
                for along in middle - 1..=middle + 1 {
                    let side = Side::on(line, along, across);
                    let from_corner = (line - first_line, along - middle);
                    for block in Block::of(side, from_corner, across).into_iter().flatten() {
                        spot.at = block.push(spot.at, radius);
                    }
                }
            }
        }
        if cell.has_pillar() {
            let (low, high) = (CELL * 0.5 - PILLAR_HALF, CELL * 0.5 + PILLAR_HALF);
            let pillar = Block {
                low: (low, low),
                high: (high, high),
            };
            spot.at = pillar.push(spot.at, radius);
        }
        spot = spot.rehomed();
    }
    spot
}

#[cfg(test)]
#[path = "../tests/backrooms_maze_tests.rs"]
mod tests;
