// Drips of chocolate that grow from the bottoms of the letters as the
// scroll melts them. Where each one starts, how long it grows and how fat
// it is are planted once per layout, seeded by its place, so a layout
// always melts the same way. Each drip is a composable that draws its shape
// in the drip ink; the chocolate's effect joins it to its letter.

use cranpose_text::TextLayout;
use cranpose_ui::{composable, Brush, Canvas, Color, CornerRadii, Modifier, Point, Rect};

use super::{
    melting::Melting,
    roots::{Ink, Root},
    walk::ease,
    whole,
};

// The chocolate's effect reads black content as a drip.
pub(super) const DRIP_INK: Color = Color(0.0, 0.0, 0.0, 1.0);
// In units.
const WARM_DRIP_REACH: f32 = 95.0;
const RUNNERS: usize = 4;

pub(super) struct TypeBlock {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub size: f32,
    pub cap: f32,
    pub baselines: Vec<f32>,
}

impl TypeBlock {
    pub(super) fn new(layout: &TextLayout, origin: Point, size: f32, cap: f32) -> Self {
        let lines = 0..layout.line_count();
        let (left, right) = lines
            .clone()
            .fold((f32::MAX, f32::MIN), |(left, right), line| {
                (
                    left.min(layout.line_left(line)),
                    right.max(layout.line_right(line)),
                )
            });
        Self {
            left: origin.x + left,
            right: origin.x + right,
            top: origin.y,
            bottom: origin.y + layout.height(),
            size,
            cap: size * cap,
            baselines: lines
                .map(|line| origin.y + layout.line_baseline(line))
                .collect(),
        }
    }

    fn first_baseline(&self) -> f32 {
        self.baselines.first().copied().unwrap_or(self.top)
    }

    fn last_baseline(&self) -> f32 {
        self.baselines.last().copied().unwrap_or(self.bottom)
    }

    fn pitch(&self) -> f32 {
        match self.baselines.as_slice() {
            [first, .., last] => (last - first) / (self.baselines.len() - 1) as f32,
            _ => self.size,
        }
    }

    fn line_of(&self, y: f32) -> usize {
        let line = whole::count(((y - self.first_baseline()) / self.pitch()).round());
        line.min(self.baselines.len().saturating_sub(1))
    }
}

pub(super) fn split(head: &TypeBlock, copy: &TypeBlock) -> f32 {
    let between = (head.bottom + copy.top) * 0.5;
    between.max(0.0) * 0.5 + (head.last_baseline() + copy.first_baseline() - copy.cap) * 0.25
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DripPlan {
    x: f32,
    y: f32,
    root: f32,
    neck: f32,
    tip: f32,
    flare: f32,
    length: f32,
    delay: f32,
    power: f32,
    from_copy: bool,
    spills: bool,
}

struct DripPose {
    root: f32,
    neck: f32,
    tip: f32,
    flare: f32,
    length: f32,
}

impl DripPlan {
    pub(super) fn landing(&self) -> Option<f32> {
        self.spills.then_some(self.x)
    }

    fn pose(&self, melting: Melting) -> Option<DripPose> {
        let u = melting.u;
        let t = ((melting.progress() - self.delay) / (0.97 - self.delay)).clamp(0.0, 1.0);
        let mut grown = ease(0.0, 0.22, t);
        let mut length = self.length * (0.2 * t + 0.8 * t.powf(self.power));
        let warmth = melting.warmth();
        if warmth.strength > 0.0 {
            let reach = u * WARM_DRIP_REACH;
            let (dx, dy) = (
                (self.x - warmth.at.x) / reach,
                (self.y - warmth.at.y) / reach,
            );
            let warm = warmth.strength * (-(dx * dx + dy * dy) * 0.5).exp();
            length += warm * if self.from_copy { 9.0 } else { 26.0 } * u;
            grown = grown.max(warm * 0.75);
        }
        if grown < 0.01 {
            return None;
        }
        if self.spills {
            let floor = melting
                .sag()
                .reaching(self.x, melting.size.height + 4.0 * u);
            length = length.min(floor - self.y);
        }
        // Drawn a little fatter than planted: the planted sizes are for
        // outlines that also swell.
        Some(DripPose {
            root: self.root * (0.35 + 0.65 * grown),
            neck: self.neck * 1.5 * (1.0 - 0.22 * t) * grown,
            tip: self.tip * 1.25 * grown * (0.7 + 0.3 * (t * 2.0).min(1.0)),
            flare: self.flare.min(length * 0.36 + 2.0 * u),
            length: length.max(0.0),
        })
    }
}

#[composable(no_skip)]
pub(super) fn Drip(plan: DripPlan, melting: Melting) {
    Canvas(Modifier::empty().fill_max_size(), move |scope| {
        let Some(pose) = plan.pose(melting) else {
            return;
        };
        let ink = Brush::solid(DRIP_INK);
        let (x, y) = (plan.x, plan.y);
        scope.draw_circle(ink.clone(), Point::new(x, y), pose.root);
        let shoulder = pose.neck + (pose.root - pose.neck) * 0.45;
        scope.draw_circle(ink.clone(), Point::new(x, y + pose.flare * 0.7), shoulder);
        if pose.neck > 0.0 && pose.length > 0.0 {
            scope.draw_round_rect_at(
                Rect {
                    x: x - pose.neck,
                    y,
                    width: pose.neck * 2.0,
                    height: pose.length,
                },
                ink.clone(),
                CornerRadii::uniform(pose.neck),
            );
        }
        let bulb = y + pose.length - pose.tip * 0.25;
        scope.draw_circle(ink, Point::new(x, bulb), pose.tip);
    });
}

pub(super) fn plant(
    (head, copy): (&TypeBlock, &TypeBlock),
    ink: &Ink,
    (u, split, floor): (f32, f32, f32),
) -> Vec<DripPlan> {
    let bed = Bed::new((head, copy), ink, (u, split, floor));
    let runners = bed.runners();
    let mut drips = Vec::new();
    bed.run_down(&runners, &mut drips);
    bed.stop_in_head(&runners, &mut drips);
    bed.bead_copy(&runners, &mut drips);
    drips
}

// What every drip of the page reads: the two blocks of type, the ink, the
// floor and the stroke ends the drips grow from.
struct Bed<'a> {
    head: &'a TypeBlock,
    copy: &'a TypeBlock,
    ink: &'a Ink,
    u: f32,
    floor: f32,
    head_scale: f32,
    copy_scale: f32,
    copy_top: f32,
    copy_bottom: f32,
    roots: Vec<Placed>,
}

impl<'a> Bed<'a> {
    fn new(
        (head, copy): (&'a TypeBlock, &'a TypeBlock),
        ink: &'a Ink,
        (u, split, floor): (f32, f32, f32),
    ) -> Self {
        Self {
            head,
            copy,
            ink,
            u,
            floor,
            head_scale: (head.size / (226.0 * u)).clamp(0.2, 1.2),
            copy_scale: (copy.size / (41.0 * u)).clamp(0.4, 1.2),
            copy_top: copy.first_baseline() - copy.cap,
            copy_bottom: copy.last_baseline(),
            roots: ink
                .roots()
                .into_iter()
                .map(|root| Placed::new(root, (head, copy), (u, split)))
                .collect(),
        }
    }

    // A drip rooted a little inside the stroke at `placed`, before its kind
    // sets its length and its shape.
    fn drip(&self, placed: &Placed) -> DripPlan {
        let block = if placed.in_head { self.head } else { self.copy };
        let z = self.u
            * if placed.in_head {
                self.head_scale
            } else {
                self.copy_scale
            };
        let root = &placed.root;
        let r = placed.random;
        DripPlan {
            x: root.x,
            y: root.y - (root.thickness * 0.5).min(block.size * 0.045),
            root: ((root.width * 0.5).min(block.size * 0.085) * 1.02).max(block.size * 0.03),
            neck: 3.0 * z,
            tip: 6.0 * z,
            flare: if placed.in_head {
                11.0 + 20.0 * r[2]
            } else {
                5.0 + 4.0 * r[2]
            } * z,
            length: 0.0,
            delay: 0.2,
            power: 1.7,
            from_copy: !placed.in_head,
            spills: false,
        }
    }

    // Whether a heading drip at `placed` has a clear way down beside the
    // paragraph.
    fn runs_clear(&self, placed: &Placed) -> bool {
        let (head, copy, u) = (self.head, self.copy, self.u);
        let beside = placed.root.x > copy.right + 12.0 * u || placed.root.x < copy.left - 12.0 * u;
        placed.in_head
            && placed.under > -head.cap * 0.2
            && placed.under < head.size * 0.4
            && placed.root.thickness > head.size * 0.04
            && beside
            && self
                .ink
                .clear_below(placed.root.x, placed.root.y)
                .is_none_or(|clear| placed.root.y + clear > self.copy_bottom)
    }

    // The heading drips that run past the paragraph, apart from each other.
    fn runners(&self) -> Vec<&Placed> {
        let mut candidates: Vec<&Placed> = self
            .roots
            .iter()
            .filter(|placed| self.runs_clear(placed))
            .collect();
        candidates.sort_by(|a, b| b.random[1].total_cmp(&a.random[1]));
        let mut runners: Vec<&Placed> = Vec::new();
        for placed in candidates {
            if runners.len() >= RUNNERS {
                break;
            }
            if runners
                .iter()
                .all(|runner| (runner.root.x - placed.root.x).abs() >= 34.0 * self.u)
            {
                runners.push(placed);
            }
        }
        runners
    }

    // Runners: the fattest and some others reach the floor, the rest stop
    // in the paragraph.
    fn run_down(&self, runners: &[&Placed], drips: &mut Vec<DripPlan>) {
        let fattest = runners
            .iter()
            .max_by(|a, b| a.random[0].total_cmp(&b.random[0]))
            .map(|placed| placed.root.x);
        let z = self.u * self.head_scale.max(0.5);
        for placed in runners {
            let r = placed.random;
            let fat = fattest == Some(placed.root.x);
            let floors = fat || r[0] > 0.55;
            let mut drip = self.drip(placed);
            drip.spills = floors;
            drip.length = if floors {
                self.floor + 6.0 * self.u - drip.y
            } else {
                self.copy_top + (self.copy_bottom - self.copy_top) * (0.25 + 0.8 * r[3]) - drip.y
            };
            drip.neck = if fat { 4.4 } else { 2.1 + 1.7 * r[4] } * z;
            drip.tip = if fat { 10.5 } else { 5.2 + 3.6 * r[3] } * z;
            drip.delay = 0.06 + 0.2 * r[3];
            drip.power = 1.05 + 1.1 * r[2];
            drips.push(drip);
        }
    }

    // How far a heading drip that is no runner falls, or `None` when it
    // stays a bead: it stops above the paragraph or runs into the line of
    // the headline below.
    fn head_length(&self, placed: &Placed) -> Option<f32> {
        let (head, copy, u) = (self.head, self.copy, self.u);
        let r = placed.random;
        let clear = self.ink.clear_below(placed.root.x, placed.root.y);
        let merges = placed.line + 1 < head.baselines.len()
            && clear.is_some_and(|clear| placed.root.y + clear < self.copy_top - copy.size * 0.4);
        if merges {
            let clear = clear.unwrap_or(0.0);
            if r[1] < 0.14 {
                return None;
            }
            return Some(if r[1] > 0.34 {
                clear + 22.0 * u * self.head_scale
            } else {
                clear * (0.25 + 0.4 * r[2])
            });
        }
        if r[0] < 0.12 {
            return None;
        }
        let room = self.copy_top - placed.root.y - 5.0 * u;
        Some((room * (0.14 + 0.8 * r[2] * r[2]) - 24.0 * u).max(6.0 * u))
    }

    fn stop_in_head(&self, runners: &[&Placed], drips: &mut Vec<DripPlan>) {
        for placed in thin_out(&self.roots, self.head, true) {
            if runners.iter().any(|runner| {
                runner.line == placed.line
                    && (runner.root.x - placed.root.x).abs() < self.head.size * 0.12
            }) {
                continue;
            }
            let Some(length) = self.head_length(placed) else {
                continue;
            };
            let r = placed.random;
            let mut drip = self.drip(placed);
            drip.length = length;
            drip.neck = (3.2 + 4.8 * r[4] * r[4]) * self.u * self.head_scale;
            drip.tip = (4.6 + 6.6 * r[3] * r[3]) * self.u * self.head_scale;
            drip.delay = 0.12 + 0.5 * r[3];
            drip.power = 1.2 + 1.4 * r[0];
            drips.push(drip);
        }
    }

    // The paragraph: short beads, and from its last line a few long ones
    // away from the runners' lanes.
    fn bead_copy(&self, runners: &[&Placed], drips: &mut Vec<DripPlan>) {
        let (copy, u) = (self.copy, self.u);
        for placed in thin_out(&self.roots, copy, false) {
            let r = placed.random;
            let last = placed.line + 1 == copy.baselines.len();
            let long = last
                && r[1] > 0.74
                && runners
                    .iter()
                    .all(|runner| (runner.root.x - placed.root.x).abs() > 22.0 * u)
                && placed.root.y - self.copy_bottom > -copy.cap * 0.2;
            let floors = long && r[2] > 0.55;
            let mut length = if floors {
                self.floor + 4.0 * u - placed.root.y
            } else if long {
                (24.0 + 46.0 * r[2]) * u
            } else {
                (4.0 + 15.0 * r[2] * r[1] + if last { 10.0 * r[3] } else { 0.0 }) * u
            };
            let clear = self.ink.clear_below(placed.root.x, placed.root.y);
            if let (false, Some(clear)) = (last, clear) {
                if length > clear - 3.0 * u {
                    length = clear * 0.6;
                }
            }
            let mut drip = self.drip(placed);
            drip.spills = floors;
            drip.length = length;
            drip.neck = if long {
                2.0 + 2.0 * r[4]
            } else {
                1.6 + 0.5 * r[4]
            } * u
                * self.copy_scale;
            drip.tip = if long {
                4.0 + 2.8 * r[4]
            } else {
                2.6 + 1.1 * r[3]
            } * u
                * self.copy_scale;
            drip.delay = 0.16 + 0.5 * r[3];
            drip.power = 1.3 + r[0];
            drips.push(drip);
        }
    }
}

struct Placed {
    root: Root,
    in_head: bool,
    line: usize,
    under: f32,
    random: [f32; 5],
}

impl Placed {
    fn new(root: Root, (head, copy): (&TypeBlock, &TypeBlock), (u, split): (f32, f32)) -> Self {
        let in_head = root.y < split;
        let block = if in_head { head } else { copy };
        let line = block.line_of(root.y);
        let seed = i32::try_from(line).expect("a block has far fewer lines than an i32 counts") * 7
            + if in_head { 1 } else { 2 };
        Self {
            root,
            in_head,
            line,
            under: root.y - block.baselines.get(line).copied().unwrap_or(root.y),
            random: [0, 1, 2, 3, 4].map(|index| hash(root.x / u / 3.0, seed, index)),
        }
    }
}

fn thin_out<'a>(roots: &'a [Placed], block: &TypeBlock, in_head: bool) -> Vec<&'a Placed> {
    let size = block.size;
    let mut sound: Vec<&Placed> = roots
        .iter()
        .filter(|placed| {
            placed.in_head == in_head
                && placed.under > -block.cap * 0.16
                && placed.under < size * 0.4
                && placed.root.width > size * 0.03
                && placed.root.thickness > size * 0.05
        })
        .collect();
    sound.sort_by(|a, b| a.line.cmp(&b.line).then(a.root.x.total_cmp(&b.root.x)));
    let mut kept = Vec::new();
    let (mut line, mut last_x) = (usize::MAX, f32::MIN);
    for placed in sound {
        if placed.line != line {
            line = placed.line;
            last_x = f32::MIN;
        }
        let last_line = line + 1 == block.baselines.len();
        let spacing = if in_head {
            size * 0.12
        } else {
            size * if last_line { 0.85 } else { 1.3 }
        };
        if placed.root.x - last_x < spacing * (0.6 + 0.9 * placed.random[4]) {
            continue;
        }
        if !in_head && !last_line && placed.random[3] < 0.55 {
            continue;
        }
        last_x = placed.root.x;
        kept.push(placed);
    }
    kept
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the cell of a coordinate is a whole number far inside an i32, and the hash mixes its bits"
)]
fn hash(a: f32, b: i32, c: i32) -> f32 {
    let mut h = (a as i32).wrapping_mul(374_761_393)
        ^ b.wrapping_mul(668_265_263)
        ^ c.wrapping_add(11).wrapping_mul(2_147_483_642);
    h = (h ^ (h.cast_unsigned() >> 13).cast_signed()).wrapping_mul(1_274_126_177);
    h ^= (h.cast_unsigned() >> 16).cast_signed();
    h.cast_unsigned() as f32 / 4_294_967_296.0
}
