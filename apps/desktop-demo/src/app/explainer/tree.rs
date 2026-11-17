// A tree over the page section's sheet, seen only by its shadow. Each
// bough is a composable that draws its twigs and leaves in black on a
// plane held up in the sun, between the sun and the paper. Its plane glows
// nothing and stands outside the camera's view, so neither the eye nor the
// pointer meets it, but its leaves cast the scene's shadow: sharp from the
// lowest boughs and soft from the higher ones, the way a sun of some size
// blurs what stands farther from what it shades. The boughs sway in a
// breeze. Two glazing bars of a window cast their shadow too.

use std::{f32::consts::PI, rc::Rc, sync::Arc};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
    Box, BoxSpec, Brush, Canvas, Color, DrawStyle, Modifier, Path, Point, Rect, Size, Stroke,
    StrokeCap,
};

use super::{stage::stage_camera, whole};

// On the sheet, per pixel of height above it, how far a shadow lands from
// what casts it: the sun is low in the upper left.
pub(super) const SHADOW_REACH: (f32, f32) = (1.6, 1.2);
// How far the four layers stand above the lowest, nearest first, per pixel
// of the stage's unit: a shadow softens with its caster's height above the
// lowest caster.
const LAYER_RISE: [f32; 4] = [0.0, 110.0, 360.0, 660.0];
// How far the glazing bars stand above the lowest bough, per pixel of the
// stage's unit.
const BAR_RISE: f32 = 250.0;
// Room round a bough for its sway, as a share of its size.
const SWAY_ROOM: f32 = 0.12;
const LEAF: Color = Color(0.0, 0.0, 0.0, 1.0);

// A small seeded generator (mulberry32): the tree is the same on every
// visit.
struct Seeded(u32);

impl Seeded {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the unit fraction narrows to the f32 the tree grows with"
    )]
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(1 | t);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        ((t ^ (t >> 14)) as f64 / 4_294_967_296.0) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

#[derive(Clone, Copy, Debug)]
struct Leaf {
    at: Point,
    angle: f32,
    length: f32,
    half_width: f32,
}

#[derive(Clone, Copy, Debug)]
struct Twig {
    from: Point,
    to: Point,
    half_thickness: f32,
}

// How a bough grows, lengths in units of the sheet's mean side.
#[derive(Clone, Copy, Debug)]
struct Growth {
    gap: f32,
    size: f32,
    fork: f32,
    thick: f32,
    droop: f32,
    // The share of a twig's base that has no leaves.
    bare: f32,
    // The chance of a second leaf at a node.
    second: f32,
    // A leaf's half width over its length.
    breadth: f32,
}

const GROWTH: Growth = Growth {
    gap: 0.05,
    size: 0.115,
    fork: 0.12,
    thick: 0.006,
    droop: 0.0,
    bare: 0.15,
    second: 0.0,
    breadth: 0.225,
};

// Where a bough starts and how it grows.
#[derive(Clone, Copy, Debug)]
struct BoughPlan {
    layer: usize,
    from: (f32, f32),
    degrees: f32,
    length: f32,
    growth: Growth,
}

// One bough's shadow, drawn where it lands on the sheet.
#[derive(Debug)]
struct Bough {
    layer: usize,
    // Where it grows from, on the sheet.
    base: Point,
    // What it covers on the sheet, with room to sway.
    bounds: Rect,
    twigs: Vec<Twig>,
    leaves: Vec<Leaf>,
}

struct Grower {
    random: Seeded,
    unit: f32,
    twigs: Vec<Twig>,
    leaves: Vec<Leaf>,
}

impl Grower {
    fn leaf(&mut self, at: Point, angle: f32, length: f32, breadth: f32) {
        let apparent = 0.42 + 0.66 * self.random.next().sqrt();
        self.leaves.push(Leaf {
            at,
            angle,
            length,
            half_width: length * breadth * apparent * self.random.between(0.9, 1.1),
        });
    }

    // A twig of leaves along a wandering curve, side twigs near its middle
    // and a leaf at the tip.
    fn twig(
        &mut self,
        from: Point,
        angle: f32,
        length: f32,
        (thick, growth): (f32, Growth),
        depth: u32,
    ) {
        let unit = self.unit;
        let segments = whole::count((length / 0.022).round()).max(5);
        let step = length / segments as f32;
        let mut bend = self.random.between(-0.4, 0.4) / segments as f32 * 2.0;
        let mut side = if self.random.next() < 0.5 { 1.0 } else { -1.0 };
        let leaf_every = whole::count((growth.gap / step).round()).max(1);
        let (mut at, mut angle) = (from, angle);
        for i in 0..segments {
            let share = (i + 1) as f32 / segments as f32;
            let next = Point::new(
                at.x + angle.cos() * step * unit,
                at.y + angle.sin() * step * unit,
            );
            self.twigs.push(Twig {
                from: at,
                to: next,
                half_thickness: thick * (1.1 - share * 0.8) * unit,
            });
            at = next;
            bend += self.random.between(-0.25, 0.25) * 0.12;
            angle += bend * 0.35 + self.random.between(-0.05, 0.05) + growth.droop * step;
            if i as f32 > segments as f32 * growth.bare && i % leaf_every == 0 {
                let leaf_angle = angle + side * self.random.between(0.55, 1.15);
                let size = growth.size * self.random.between(0.72, 1.15) * unit;
                self.leaf(at, leaf_angle, size, growth.breadth);
                side = -side;
                if self.random.next() < growth.second {
                    let second = angle + side * self.random.between(0.5, 1.2);
                    let size = growth.size * self.random.between(0.6, 1.05) * unit;
                    self.leaf(at, second, size, growth.breadth);
                }
            }
            if depth > 0 && i > 1 && i + 2 < segments && self.random.next() < growth.fork {
                let turn = if self.random.next() < 0.5 { -1.0 } else { 1.0 };
                let fork_angle = angle + turn * self.random.between(0.5, 0.95);
                let fork_length = length * self.random.between(0.38, 0.62) * (1.1 - share * 0.5);
                self.twig(
                    at,
                    fork_angle,
                    fork_length,
                    (thick * 0.7, growth),
                    depth - 1,
                );
            }
        }
        let tip_angle = angle + self.random.between(-0.3, 0.3);
        let size = growth.size * self.random.between(0.9, 1.2) * unit;
        self.leaf(at, tip_angle, size, growth.breadth);
    }

    // The thick of the tree: a clump of leaves round (`x`, `y`).
    fn clump(&mut self, centre: Point, (radius_x, radius_y): (f32, f32), leaves: usize) {
        for _ in 0..leaves {
            let angle = self.random.between(0.0, 2.0 * PI);
            let reach = self.random.next().sqrt();
            let at = Point::new(
                centre.x + angle.cos() * reach * radius_x,
                centre.y + angle.sin() * reach * radius_y,
            );
            let leaf_angle = self.random.between(0.0, 2.0 * PI);
            let size = self.random.between(0.10, 0.15) * self.unit;
            self.leaf(at, leaf_angle, size, 0.3);
        }
    }

    fn finish(&mut self, layer: usize, base: Point) -> Bough {
        let (twigs, leaves) = (
            std::mem::take(&mut self.twigs),
            std::mem::take(&mut self.leaves),
        );
        let mut low = base;
        let mut high = base;
        let mut grow = |point: Point, margin: f32| {
            low = Point::new(low.x.min(point.x - margin), low.y.min(point.y - margin));
            high = Point::new(high.x.max(point.x + margin), high.y.max(point.y + margin));
        };
        for twig in &twigs {
            grow(twig.to, twig.half_thickness);
        }
        for leaf in &leaves {
            grow(leaf.at, leaf.length);
        }
        Bough {
            layer,
            base,
            bounds: Rect {
                x: low.x,
                y: low.y,
                width: high.x - low.x,
                height: high.y - low.y,
            },
            twigs,
            leaves,
        }
    }
}

// The boughs of the tree, from the lowest layer to the highest, for a
// portrait sheet or a landscape one.
fn bough_plans(portrait: bool) -> [BoughPlan; 12] {
    let near = Growth {
        gap: 0.036,
        size: 0.074,
        fork: 0.07,
        thick: 0.0032,
        droop: -0.7,
        ..GROWTH
    };
    let willow = Growth {
        gap: 0.032,
        size: 0.072,
        fork: 0.05,
        thick: 0.0026,
        breadth: 0.12,
        ..GROWTH
    };
    let crown = Growth {
        gap: 0.034,
        size: 0.078,
        fork: 0.09,
        second: 0.2,
        bare: 0.0,
        ..GROWTH
    };
    let high = Growth {
        gap: 0.042,
        size: 0.095,
        fork: 0.06,
        second: 0.3,
        bare: 0.0,
        thick: 0.010,
        ..GROWTH
    };
    let plan = |layer, from, degrees, length, growth| BoughPlan {
        layer,
        from,
        degrees,
        length,
        growth,
    };
    [
        plan(
            0,
            (1.10, -0.62),
            168.0,
            if portrait { 0.30 } else { 0.44 },
            near,
        ),
        plan(
            0,
            (1.10, 0.40),
            196.0,
            if portrait { 0.16 } else { 0.26 },
            Growth { droop: 0.0, ..near },
        ),
        plan(0, (-1.10, 0.66), -52.0, 0.17, willow),
        plan(1, (0.62, -1.30), 100.0, 0.32, Growth { droop: 0.0, ..near }),
        plan(
            1,
            (1.15, 0.95),
            205.0,
            0.34,
            Growth {
                droop: 0.0,
                breadth: 0.235,
                ..near
            },
        ),
        plan(1, (-1.12, -0.10), 30.0, 0.18, willow),
        plan(2, (0.30, -1.3), 70.0, 0.24, crown),
        plan(2, (1.15, 0.10), 175.0, 0.26, crown),
        plan(2, (-0.55, -1.3), 100.0, 0.20, crown),
        plan(2, (-1.2, 0.30), 8.0, 0.20, crown),
        plan(3, (1.15, 0.75), 190.0, 0.26, high),
        plan(3, (-1.2, -0.6), 10.0, 0.22, high),
    ]
}

// Grows the tree for a sheet of `size`: boughs that start at fractions of
// the half sheet from its middle and head off at an angle in degrees.
fn grow(size: Size) -> Vec<Bough> {
    let unit = (size.width * size.height).sqrt();
    let mut grower = Grower {
        random: Seeded(20_611),
        unit,
        twigs: Vec::new(),
        leaves: Vec::new(),
    };
    let at = |fx: f32, fy: f32| {
        Point::new(
            size.width * 0.5 * (1.0 + fx),
            size.height * 0.5 * (1.0 + fy),
        )
    };
    let boughs = bough_plans(size.height > size.width);
    let mut tree: Vec<Bough> = boughs
        .into_iter()
        .map(|bough| {
            let base = at(bough.from.0, bough.from.1);
            let growth = bough.growth;
            grower.twig(
                base,
                bough.degrees.to_radians(),
                bough.length,
                (growth.thick, growth),
                1,
            );
            grower.finish(bough.layer, base)
        })
        .collect();
    for (fx, fy, radius, leaves) in [
        (1.02, -1.02, (0.52, 0.52), 130),
        (0.25, -1.26, (0.22, 0.2), 24),
        (1.16, 0.12, (0.20, 0.34), 36),
        (-1.10, -0.66, (0.22, 0.36), 40),
    ] {
        let centre = at(fx, fy);
        let radii = (radius.0 * size.width * 0.5, radius.1 * size.height * 0.5);
        grower.clump(centre, radii, leaves);
        tree.push(grower.finish(3, centre));
    }
    tree
}

// The tree over a sheet of `size` on a stage of unit `u`, swaying as
// `breeze()` says, read each time it is drawn.
#[composable(no_skip)]
pub(super) fn Tree(size: Size, u: f32, breeze: impl Fn() -> f32 + Copy + 'static) {
    let (tree, plane, base) = remember(move || {
        let tree = grow(size);
        let base = lowest_height(&tree, size, u);
        (Rc::new(tree), Arc::new(Mesh::plane()), base)
    })
    .with(Clone::clone);
    for index in 0..tree.len() {
        let (tree, plane) = (Rc::clone(&tree), Arc::clone(&plane));
        cranpose_core::key(index, move || {
            BoughShadow(tree, index, (plane, base, u), breeze);
        });
    }
    let plane = Arc::clone(&plane);
    GlazingBars(size, base + BAR_RISE * u, plane);
}

// Where a caster stands so that its shadow lands on `shadow`, `height`
// above the sheet.
fn caster_at(shadow: Point, height: f32) -> Vec3 {
    Vec3::new(
        shadow.x - SHADOW_REACH.0 * height,
        shadow.y - SHADOW_REACH.1 * height,
        -height,
    )
}

// The glazing bars: where their shadows' middles lie, how they turn, and
// how long they are, on a sheet of `size`.
fn bars(size: Size) -> [(Point, f32, f32); 2] {
    let length = size.width.max(size.height) * 1.15;
    [
        (
            Point::new(size.width * 0.31, size.height * 0.5),
            PI * 0.5 + 0.07,
            length,
        ),
        (
            Point::new(size.width * 0.5, size.height * 0.1),
            -0.055,
            length,
        ),
    ]
}

// The least height of the lowest boughs at which every caster, swayed as
// far as it goes, stands above or left of what the stage's camera shows.
fn lowest_height(tree: &[Bough], size: Size, u: f32) -> f32 {
    let camera = stage_camera(size);
    let hidden = |shadow: Rect, height: f32| {
        let corners = [
            (shadow.x, shadow.y),
            (shadow.x + shadow.width, shadow.y),
            (shadow.x, shadow.y + shadow.height),
            (shadow.x + shadow.width, shadow.y + shadow.height),
        ];
        let shown = corners.map(|(x, y)| camera.project(caster_at(Point::new(x, y), height), size));
        let all = |test: fn(Point) -> bool| shown.iter().all(|point| point.is_some_and(test));
        all(|point| point.y < 0.0) || all(|point| point.x < 0.0)
    };
    let swayed = |rect: Rect| {
        let (dx, dy) = (rect.width * SWAY_ROOM, rect.height * SWAY_ROOM);
        Rect {
            x: rect.x - dx,
            y: rect.y - dy,
            width: rect.width + 2.0 * dx,
            height: rect.height + 2.0 * dy,
        }
    };
    let bar_rects = bars(size).map(|(middle, angle, length)| {
        let (half_x, half_y) = (
            (angle.cos() * length * 0.5).abs(),
            (angle.sin() * length * 0.5).abs(),
        );
        Rect {
            x: middle.x - half_x,
            y: middle.y - half_y,
            width: 2.0 * half_x,
            height: 2.0 * half_y,
        }
    });
    let focal_length = camera.focal_length;
    (3..18)
        .map(|step| focal_length * step as f32 * 0.05)
        .find(|&base| {
            tree.iter()
                .all(|bough| hidden(swayed(bough.bounds), base + LAYER_RISE[bough.layer] * u))
                && bar_rects
                    .iter()
                    .all(|&rect| hidden(rect, base + BAR_RISE * u))
        })
        .unwrap_or(focal_length * 0.5)
}

#[composable(no_skip)]
fn BoughShadow(
    tree: Rc<Vec<Bough>>,
    index: usize,
    (plane, base, u): (Arc<Mesh>, f32, f32),
    breeze: impl Fn() -> f32 + Copy + 'static,
) {
    let bough = &tree[index];
    let bounds = bough.bounds;
    let height = base + LAYER_RISE[bough.layer] * u;
    let root = bough.base;
    let pivot = Vec3::new(root.x - bounds.x, root.y - bounds.y, 0.0);
    let swing = 0.012 + 0.008 * bough.layer as f32;
    let drawing = Rc::clone(&tree);
    Box(
        Modifier::empty()
            .required_size(Size::new(bounds.width, bounds.height))
            .mesh_surface(move || {
                let time = breeze();
                let phase = index as f32 * 1.7;
                let angle = swing
                    * ((time * 0.83 + phase).sin() + 0.45 * (time * 1.47 + phase * 1.7).sin());
                MeshSurface {
                    mesh: Arc::clone(&plane),
                    transform: Mat4::scaling(Vec3::new(bounds.width, bounds.height, 1.0))
                        .then(&Mat4::translation(-pivot))
                        .then(&Mat4::rotation_z(angle))
                        .then(&Mat4::translation(caster_at(root, height))),
                    material: Material::Glow,
                    casts_shadow: true,
                    solid: false,
                }
            }),
        BoxSpec::default(),
        move || {
            let drawing = Rc::clone(&drawing);
            Canvas(Modifier::empty().fill_max_size(), move |scope| {
                let bough = &drawing[index];
                let corner = Point::new(bough.bounds.x, bough.bounds.y);
                let local = |point: Point| Point::new(point.x - corner.x, point.y - corner.y);
                for twig in &bough.twigs {
                    let stroke = Stroke::new(twig.half_thickness * 2.0).with_cap(StrokeCap::Round);
                    scope.draw_line(Brush::solid(LEAF), local(twig.from), local(twig.to), stroke);
                }
                let mut path = Path::new();
                for leaf in &bough.leaves {
                    leaf_outline(&mut path, leaf, corner);
                }
                scope.draw_path(&path, Brush::solid(LEAF), DrawStyle::Fill);
            });
        },
    );
}

// A leaf: a bare stalk for its first fifth, then a blade that is widest
// past its middle.
fn leaf_outline(path: &mut Path, leaf: &Leaf, corner: Point) {
    let (sin, cos) = leaf.angle.sin_cos();
    let at = |along: f32, across: f32| {
        Point::new(
            leaf.at.x - corner.x + cos * along - sin * across,
            leaf.at.y - corner.y + sin * along + cos * across,
        )
    };
    let length = leaf.length;
    let width = leaf.half_width * 2.0;
    path.move_to(at(0.18 * length, 0.0));
    path.quadratic_to(at(0.62 * length, width), at(length, 0.0));
    path.quadratic_to(at(0.62 * length, -width), at(0.18 * length, 0.0));
    path.close();
}

// Two glazing bars of a window, `height` above the sheet of `size`: one
// nearly upright left of the middle, one nearly level near the top.
#[composable(no_skip)]
fn GlazingBars(size: Size, height: f32, plane: Arc<Mesh>) {
    let thickness = 0.025 * (size.width * size.height).sqrt();
    for (index, (middle, angle, length)) in bars(size).into_iter().enumerate() {
        let plane = Arc::clone(&plane);
        cranpose_core::key(("bar", index), move || {
            Box(
                Modifier::empty()
                    .required_size(Size::new(32.0, 32.0))
                    .mesh_surface(move || MeshSurface {
                        mesh: Arc::clone(&plane),
                        transform: Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                            .then(&Mat4::scaling(Vec3::new(length, thickness, 1.0)))
                            .then(&Mat4::rotation_z(angle))
                            .then(&Mat4::translation(caster_at(middle, height))),
                        material: Material::Glow,
                        casts_shadow: true,
                        solid: false,
                    })
                    .background(LEAF),
                BoxSpec::default(),
                || {},
            );
        });
    }
}
