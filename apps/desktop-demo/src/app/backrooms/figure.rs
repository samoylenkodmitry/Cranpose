// The figure that stands far down a corridor now and then. It shows up on a
// way the eye is not looking along, and when the eye looks at it for a
// moment or walks toward it, the lamps go out for an instant and it is
// gone.

use cranpose_ui::space::{EyePose, Vec3};

use super::maze::{Cell, Lamp, Toward, CELL};

// How long the figure waits before it first shows, and before it shows
// again: the least and how much longer at most.
const FIRST_WAIT: f32 = 20.0;
const WAIT: f32 = 25.0;
const WAIT_SPREAD: f32 = 30.0;
// How soon it tries again when no way suits it.
const RETRY: f32 = 2.0;
// How many cells away it stands.
const NEAREST_CELLS: u32 = 3;
const FARTHEST_CELLS: u32 = 7;
// How long it stands at most, how long the eye may look at it and how near
// the eye may come before it goes.
const STAYS: f32 = 30.0;
const SEEN_FOR: f32 = 1.2;
const CLOSEST: f32 = 2.0 * CELL;
// How long the lamps stay out when it goes.
const BLACKOUT: f32 = 0.3;
// The cosines of the angle from where the eye looks within which the eye
// sees the figure, and beyond which the figure may show up.
const SEEN_COS: f32 = 0.9;
const UNSEEN_COS: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Stand {
    cell: Cell,
    seen: f32,
    left: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Figure {
    stands: Option<Stand>,
    // The seconds until it tries to show up.
    wait: f32,
    // The seconds the lamps stay out.
    blackout: f32,
    // How many times it has looked for a place, which picks the next.
    picks: u32,
}

impl Default for Figure {
    fn default() -> Self {
        Self {
            stands: None,
            wait: FIRST_WAIT,
            blackout: 0.0,
            picks: 0,
        }
    }
}

impl Figure {
    // Where it stands on the floor, measured from the corner of `from`,
    // while it stands there.
    pub(super) fn at(&self, from: Cell) -> Option<Vec3> {
        self.stands.map(|stand| stand.cell.middle_from(from))
    }

    // How far the lamps are out: all the way, during a blackout.
    pub(super) fn dim(&self) -> f32 {
        if self.blackout > 0.0 {
            1.0
        } else {
            0.0
        }
    }

    // Lets `seconds` pass with the eye at `eye`, measured from the corner of
    // `cell`.
    pub(super) fn tick(&mut self, seconds: f32, eye: &EyePose, cell: Cell) {
        self.blackout = (self.blackout - seconds).max(0.0);
        let (ahead, _) = eye.walking_axes();
        let Some(mut stand) = self.stands else {
            self.wait -= seconds;
            if self.wait <= 0.0 {
                self.wait = RETRY;
                self.stands = self.place(cell, ahead);
            }
            return;
        };
        let at = stand.cell.middle_from(cell);
        let to = Vec3::new(at.x - eye.position.x, 0.0, at.z - eye.position.z);
        let distance = to.length();
        if distance > 0.0 && to.dot(ahead) > SEEN_COS * distance {
            stand.seen += seconds;
        }
        stand.left -= seconds;
        let noticed = stand.seen >= SEEN_FOR || distance < CLOSEST;
        if noticed || stand.left <= 0.0 {
            self.stands = None;
            self.wait = WAIT + WAIT_SPREAD * self.random();
            if noticed {
                self.blackout = BLACKOUT;
            }
        } else {
            self.stands = Some(stand);
        }
    }

    // A lit place a few cells down a straight way from the eye's cell that
    // the eye does not look along.
    fn place(&mut self, cell: Cell, ahead: Vec3) -> Option<Stand> {
        let first = self.picks as usize;
        self.picks = self.picks.wrapping_add(1);
        (0..Toward::ALL.len())
            .map(|turn| Toward::ALL[(first + turn) % Toward::ALL.len()])
            .filter(|toward| {
                let (x, z) = toward.step();
                ahead.x * x + ahead.z * z < UNSEEN_COS
            })
            .find_map(|toward| {
                let run = cell.open_line(toward, FARTHEST_CELLS);
                let far = (0..run).fold(cell, |far, _| far.next(toward));
                (run >= NEAREST_CELLS && matches!(far.lamp(), Lamp::Lit | Lamp::Flickering))
                    .then_some(Stand {
                        cell: far,
                        seen: 0.0,
                        left: STAYS,
                    })
            })
    }

    // A number from 0 up to 1 that follows from the picks so far.
    fn random(&self) -> f32 {
        (self.picks.wrapping_mul(0x9e37_79b1) >> 8) as f32 / 16_777_216.0
    }
}
