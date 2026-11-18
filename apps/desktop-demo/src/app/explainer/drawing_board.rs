// The wooden drawing board that holds the flat copy: a slab of timber
// leaning back by the camera's pitch on a ledge at its foot, "flat copy"
// stamped on the wood under the sheet, and the face the paragraph's flat
// copy is drawn on, sheet, tape and timber in one shader.

use std::sync::{Arc, OnceLock};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded},
    Box, BoxSpec, Color, Modifier, Point, Rect, Size, Text,
};

use super::{
    figure::{world_to_scene, Figure, Plan},
    lettering::strong,
    sheet_mesh::{label_surface, sheet_mesh},
    walk::{ease, ease_softly},
};

const WOOD: Color = Color(0.70, 0.52, 0.32, 1.0);
const LEDGE: Color = Color(0.56, 0.38, 0.21, 1.0);
const STAMP: Color = Color(0.16, 0.08, 0.03, 1.0);
// How thick the board is.
const THICKNESS: f32 = 14.0;
const FACE_WGSL: &str =
    cranpose_ui::include_wgsl!("noise.wgsl", "sheet_paper.wgsl", "board_face.wgsl");

// When the view finder's corners close in round the sheet.
const FINDER_IN: (f32, f32) = (0.222, 0.25);
const FINDER_OUT: (f32, f32) = (0.262, 0.29);
// How far past the board the finder's corners reach.
const FINDER_BAND: f32 = 70.0;

// The board's body and ledge, and its label once `labelled`, planned by
// `plan`, in a scene node of `node`'s size, posed by `pose`.
#[composable(no_skip)]
pub(super) fn DrawingBoard(
    (node, plan): (Size, Plan),
    labelled: bool,
    pose: impl Fn() -> Figure + Copy + 'static,
) {
    let block = remember(|| Arc::new(Mesh::cuboid())).with(Arc::clone);
    let ledge_block = Arc::clone(&block);
    Slab(Arc::clone(&block), WOOD, move || {
        let figure = pose();
        let plan = figure.plan;
        let (right, up, out) = figure.board_axes();
        let u = plan.u;
        let middle = Point::new(
            plan.sheet.width * 0.5,
            (plan.sheet.height + plan.margin_bottom - plan.margin) * 0.5,
        );
        let thickness = THICKNESS * u;
        Mat4::from_axes(
            right * plan.outer_width(),
            up * plan.outer_height(),
            out * thickness,
            figure.on_board(middle, -thickness * 0.5 - 0.5 * u),
        )
        .then(&world_to_scene(node))
    });
    Slab(ledge_block, LEDGE, move || {
        let figure = pose();
        let plan = figure.plan;
        let (right, up, out) = figure.board_axes();
        let u = plan.u;
        let foot = Point::new(
            plan.sheet.width * 0.5,
            plan.sheet.height + plan.margin_bottom + u,
        );
        Mat4::from_axes(
            right * (plan.outer_width() + 24.0 * u),
            up * (16.0 * u),
            out * (22.0 * u),
            figure.on_board(foot, 2.0 * u),
        )
        .then(&world_to_scene(node))
    });
    if labelled {
        FlatLabel(node, plan, pose);
    }
}

// "flat copy", stamped on the wood under the sheet.
#[composable(no_skip)]
fn FlatLabel(node: Size, plan: Plan, pose: impl Fn() -> Figure + Copy + 'static) {
    let u = plan.u;
    let size = Size::new(196.0 * u, 44.0 * u);
    Box(
        Modifier::empty().required_size(size).mesh_surface(move || {
            let figure = pose();
            let plan = figure.plan;
            let at = Rect {
                x: 66.0 * u,
                y: plan.sheet.height + (plan.margin_bottom - size.height) * 0.5,
                width: size.width,
                height: size.height,
            };
            let (_, _, out) = figure.board_axes();
            let mesh = sheet_mesh(at, at, 1, |point| (figure.on_board(point, 0.6 * u), out));
            label_surface(mesh, node)
        }),
        BoxSpec::default(),
        move || {
            Text("flat copy", Modifier::empty(), strong(34.0 * u, STAMP));
        },
    );
}

// A block of wood placed by `place`, read each time it is drawn.
#[composable(no_skip)]
fn Slab(block: Arc<Mesh>, color: Color, place: impl Fn() -> Mat4 + 'static) {
    Box(
        Modifier::empty()
            .required_size(Size::new(16.0, 16.0))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&block),
                transform: place(),
                material: Material::Lit,
                casts_shadow: true,
                solid: true,
            })
            .background(color),
        BoxSpec::default(),
        || {},
    );
}

// The board's face that the paragraph's flat copy is drawn on, posed as
// `figure`, in a scene node of `node`'s size; `progress` along the walk
// sets how strong the ruled boxes and the finder show.
pub(super) fn flat_copy(figure: &Figure, node: Size, progress: f32) -> MeshSurface {
    let plan = figure.plan;
    let u = plan.u;
    let sheet = Rect::from_size(plan.sheet.size());
    // A band round the board, where the finder's corners show.
    let band = FINDER_BAND * u;
    let face = Rect {
        x: -plan.margin - band,
        y: -plan.margin - band,
        width: plan.outer_width() + 2.0 * band,
        height: plan.outer_height() + 2.0 * band,
    };
    let (_, _, out) = figure.board_axes();
    let mesh = sheet_mesh(face, sheet, 1, |point| (figure.on_board(point, 0.0), out));
    // Once the camera has pulled back the boxes are quieter; at step 4 they
    // almost go.
    let guides = (1.0 + (0.62 - 1.0) * figure.pull) * (1.0 - 0.86 * ease(0.72, 0.80, progress));
    let finder = ease(FINDER_IN.0, FINDER_IN.1, progress)
        * (1.0 - ease(FINDER_OUT.0, FINDER_OUT.1, progress));
    let close_in = ease_softly(FINDER_IN.0, FINDER_OUT.0, progress);
    let mut params = [0.0; Shaded::PARAMS];
    params[..8].copy_from_slice(&[
        sheet.width,
        sheet.height,
        u,
        guides,
        plan.margin,
        plan.margin_bottom,
        finder,
        plan.margin + (40.0 + (12.0 - 40.0) * close_in) * u,
    ]);
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    MeshSurface {
        mesh: Arc::new(mesh),
        transform: world_to_scene(node),
        material: Material::Shaded(Shaded {
            shader: Arc::clone(SHADER.get_or_init(|| Arc::from(FACE_WGSL))),
            params,
        }),
        casts_shadow: false,
        solid: true,
    }
}
