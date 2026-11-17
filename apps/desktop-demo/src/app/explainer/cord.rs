// Step 4 of "How it works": a cord ties the spot on the bent copy to the
// same spot on the flat copy. From the pointed word it runs along the
// word's own line to the paper's right edge, leaves the print, swings
// through the air to the board and runs back along the same line of the
// flat copy to the same word. Orange beads mark both spots and the points
// where the cord leaves the paper.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    Box, BoxSpec, Color, Modifier, Point, Rect, Size,
};

use super::{
    figure::{timeline, world_to_scene, Figure},
    paragraph::LayoutNow,
    point_back::PointBack,
    sheet_layout::SheetLayout,
    walk::{ease, Walk},
};

const BEAD: Color = Color(1.0, 0.36, 0.08, 1.0);
const CORD_WGSL: &str = concat!(
    include_str!("noise.wgsl"),
    include_str!("sheet_paper.wgsl"),
    include_str!("cord.wgsl")
);
// Points along the cord on each copy, and through the air.
const ALONG: usize = 24;
const AIR: usize = 44;
// Vertices round the cord's tube.
const ROUND: usize = 8;

// The cord and its beads, in a scene node of `node`'s size, posed by
// `pose`, `walk` along, between the spots `point` holds.
#[composable(no_skip)]
pub(super) fn Cord(
    node: Size,
    (walk, pose): (Walk, impl Fn() -> Figure + Copy + 'static),
    point: PointBack,
    layout: LayoutNow,
) {
    let ends = Rc::new(move || CordEnds::of(&pose(), point, &layout.borrow()));
    let thread = Rc::clone(&ends);
    Box(
        Modifier::empty()
            .required_size(Size::new(16.0, 16.0))
            .mesh_surface(move || {
                let ends = thread();
                let u = ends.figure.plan.u;
                let shows = ease(timeline::CORD.0, timeline::CORD.1, walk.progress());
                let (mesh, along) = tube(&ends.path(), 1.9 * u);
                let length = along.last().copied().unwrap_or(0.0);
                // Over the words of the line the cord is drawn faint.
                let (print_words, board_words) = ends.over_words();
                let mut params = [0.0; Shaded::PARAMS];
                params[..6].copy_from_slice(&[
                    shows,
                    point.pulse.get(),
                    length,
                    u,
                    along[print_words],
                    along[board_words],
                ]);
                static SHADER: OnceLock<Arc<str>> = OnceLock::new();
                MeshSurface {
                    mesh: Arc::new(mesh),
                    transform: world_to_scene(node),
                    material: Material::Shaded(Shaded {
                        shader: Arc::clone(SHADER.get_or_init(|| Arc::from(CORD_WGSL))),
                        params,
                    }),
                    casts_shadow: true,
                    solid: false,
                }
            })
            .background(BEAD),
        BoxSpec::default(),
        || {},
    );
    let ball = remember(|| Arc::new(Mesh::sphere(16, 10))).with(Arc::clone);
    for bead in Bead::ALL {
        let (ball, ends) = (Arc::clone(&ball), Rc::clone(&ends));
        cranpose_core::key(bead as usize, move || {
            Box(
                Modifier::empty()
                    .required_size(Size::new(16.0, 16.0))
                    .mesh_surface(move || {
                        let ends = ends();
                        let shows = ease(timeline::CORD.0, timeline::CORD.1, walk.progress());
                        let radius = bead.radius() * ends.figure.plan.u * shows;
                        MeshSurface {
                            mesh: Arc::clone(&ball),
                            transform: Mat4::scaling(Vec3::new(radius, radius, radius))
                                .then(&Mat4::translation(ends.bead(bead)))
                                .then(&world_to_scene(node)),
                            material: Material::Lit,
                            casts_shadow: true,
                            solid: false,
                        }
                    })
                    .background(BEAD),
                BoxSpec::default(),
                || {},
            );
        });
    }
}

// The beads on the cord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bead {
    // The spot on the flat copy.
    OnBoard,
    // The spot on the bent copy.
    OnPrint,
    // Where the cord leaves the print.
    Leaves,
    // Where it reaches the board.
    Arrives,
}

impl Bead {
    const ALL: [Self; 4] = [Self::OnBoard, Self::OnPrint, Self::Leaves, Self::Arrives];

    fn radius(self) -> f32 {
        match self {
            Self::OnBoard | Self::OnPrint => 7.5,
            Self::Leaves => 4.6,
            Self::Arrives => 4.1,
        }
    }
}

// Where the cord runs on the paper: along the pointed word's line, from
// just past the word to the paper's right edge, on the posed figure.
struct CordEnds {
    figure: Figure,
    spot: Point,
    // Where the words of the line end, past which the cord shows whole.
    words_end: f32,
    line_y: f32,
    start_x: f32,
    edge_x: f32,
}

impl CordEnds {
    fn of(figure: &Figure, point: PointBack, layout: &SheetLayout) -> Self {
        let plan = figure.plan;
        let spot = point.spot.get().unwrap_or_default();
        // Off the words the cord starts at the spot itself.
        let word = point
            .word
            .get()
            .or_else(|| layout.word_near(spot.x, spot.y))
            .unwrap_or(Rect {
                x: spot.x,
                y: spot.y,
                width: 0.0,
                height: 0.0,
            });
        let line_y = word.y + word.height * 0.5;
        Self {
            figure: figure.clone(),
            spot,
            line_y,
            words_end: layout
                .lines
                .iter()
                .find(|line| (line.y + line.height * 0.5 - line_y).abs() < line.height * 0.5)
                .map_or(0.0, |line| line.x + line.width + 3.0 * plan.u),
            start_x: word.x + word.width + 6.0 * plan.u,
            edge_x: plan.sheet.width + plan.margin * 0.55,
        }
    }

    fn lift(&self) -> f32 {
        1.5 * self.figure.plan.u
    }

    fn leaves(&self) -> Vec3 {
        self.figure
            .on_print(Point::new(self.edge_x, self.line_y), self.lift())
            .0
    }

    fn arrives(&self) -> Vec3 {
        self.figure
            .on_board(Point::new(self.edge_x, self.line_y), self.lift())
    }

    fn bead(&self, bead: Bead) -> Vec3 {
        match bead {
            Bead::OnBoard => self.figure.on_board(self.spot, self.lift()),
            Bead::OnPrint => self.figure.on_print(self.spot, self.lift()).0,
            Bead::Leaves => self.leaves(),
            Bead::Arrives => self.arrives(),
        }
    }

    // The points of [`Self::path`] where the cord leaves the words of the
    // print's line, and where it reaches those of the board's.
    fn over_words(&self) -> (usize, usize) {
        let share =
            ((self.words_end - self.start_x) / (self.edge_x - self.start_x)).clamp(0.0, 1.0);
        let on_print = (share * ALONG as f32).ceil() as usize;
        (on_print, 2 * ALONG + AIR - on_print)
    }

    // The cord's points in world space: along the print's line, through
    // the air, and back along the board's line.
    fn path(&self) -> Vec<Vec3> {
        let (figure, lift, u) = (&self.figure, self.lift(), self.figure.plan.u);
        let across =
            |step: usize, from: f32, to: f32| from + (to - from) * step as f32 / ALONG as f32;
        let mut points = Vec::with_capacity(2 * ALONG + AIR + 1);
        for step in 0..=ALONG {
            let x = across(step, self.start_x, self.edge_x);
            points.push(figure.on_print(Point::new(x, self.line_y), lift).0);
        }
        let (leave, arrive) = (self.leaves(), self.arrives());
        let bulge = 84.0 * u + (leave - arrive).length() * 0.14;
        let right = Vec3::new(1.0, 0.0, 0.0);
        let (c1, c2) = (
            leave + right * (bulge * 1.1),
            arrive + right * (bulge * 2.0),
        );
        for step in 1..AIR {
            let t = step as f32 / AIR as f32;
            let k = 1.0 - t;
            points.push(
                leave * (k * k * k)
                    + c1 * (3.0 * k * k * t)
                    + c2 * (3.0 * k * t * t)
                    + arrive * (t * t * t),
            );
        }
        for step in 0..=ALONG {
            let x = across(step, self.edge_x, self.start_x);
            points.push(figure.on_board(Point::new(x, self.line_y), lift));
        }
        points
    }
}

// A tube `radius` thick round `points`, and how far along it each point
// lies.
fn tube(points: &[Vec3], radius: f32) -> (Mesh, Vec<f32>) {
    let count = points.len();
    let mut along = Vec::with_capacity(count);
    let mut length = 0.0;
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            length += (*point - points[index - 1]).length();
        }
        along.push(length);
    }
    let mut positions = Vec::with_capacity(count * (ROUND + 1));
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut uvs = Vec::with_capacity(positions.capacity());
    for (index, &point) in points.iter().enumerate() {
        let (before, after) = (
            points[index.saturating_sub(1)],
            points[(index + 1).min(count - 1)],
        );
        let tangent = (after - before)
            .normalized()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let side = tangent
            .cross(Vec3::new(0.0, 1.0, 0.0))
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let up = side.cross(tangent);
        for round in 0..=ROUND {
            let angle = round as f32 / ROUND as f32 * std::f32::consts::TAU;
            let normal = side * angle.cos() + up * angle.sin();
            positions.push((point + normal * radius).to_array());
            normals.push(normal.to_array());
            uvs.push([
                along[index] / length.max(1.0e-3),
                round as f32 / ROUND as f32,
            ]);
        }
    }
    let stride = (ROUND + 1) as u32;
    let mut indices = Vec::with_capacity((count - 1) * ROUND * 6);
    for index in 0..(count - 1) as u32 {
        for round in 0..ROUND as u32 {
            let at = index * stride + round;
            indices.extend_from_slice(&[
                at,
                at + stride,
                at + 1,
                at + 1,
                at + stride,
                at + stride + 1,
            ]);
        }
    }
    let mesh = Mesh::new(positions, uvs, Some(normals), indices).expect("a whole tube");
    (mesh, along)
}
