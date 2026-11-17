// The glossy photographic print that holds the bent copy: the paragraph's
// picture in a white border, standing on its edge on the table and curling
// as the walk goes on, with "bent copy" written in its bottom border.

use std::sync::{Arc, OnceLock};

use cranpose_ui::{
    composable,
    space::{Material, MeshSurface, Shaded},
    Box, BoxSpec, Color, Modifier, Point, Rect, Size, Text,
};

use super::{
    figure::{timeline, world_to_scene, Figure, Plan, PROFILE_SEGMENTS},
    lettering::strong,
    sheet_mesh::sheet_mesh,
    walk::ease,
};

const LABEL_INK: Color = Color(0.03, 0.035, 0.07, 1.0);
const PRINT_WGSL: &str = concat!(
    include_str!("noise.wgsl"),
    include_str!("sheet_paper.wgsl"),
    include_str!("print.wgsl")
);

// How much of the print shows `progress` along the walk.
pub(super) fn print_shows(progress: f32) -> f32 {
    ease(timeline::PRINT_IN.0, timeline::PRINT_IN.1, progress)
}

// The print the paragraph's bent copy is drawn on, posed as `figure`, in a
// scene node of `node`'s size, `progress` along the walk.
pub(super) fn bent_copy(figure: &Figure, node: Size, progress: f32) -> MeshSurface {
    let plan = figure.plan;
    let u = plan.u;
    let sheet = Rect::from_size(plan.sheet.size());
    let outline = Rect {
        x: -plan.margin,
        y: -plan.margin,
        width: plan.outer_width(),
        height: plan.outer_height(),
    };
    let mesh = sheet_mesh(outline, sheet, PROFILE_SEGMENTS, |point| {
        figure.on_print(point, 0.0)
    });
    let show = print_shows(progress);
    // A wire mesh shows on the picture while it bends.
    let wires = ease(0.50, 0.57, progress) * (1.0 - ease(0.74, 0.80, progress)) * 0.30;
    let mut params = [0.0; Shaded::PARAMS];
    params[..8].copy_from_slice(&[
        sheet.width,
        sheet.height,
        u,
        show,
        plan.margin,
        plan.margin_bottom,
        figure.bend,
        wires,
    ]);
    static SHADER: OnceLock<Arc<str>> = OnceLock::new();
    MeshSurface {
        mesh: Arc::new(mesh),
        transform: world_to_scene(node),
        material: Material::Shaded(Shaded {
            shader: Arc::clone(SHADER.get_or_init(|| Arc::from(PRINT_WGSL))),
            params,
        }),
        casts_shadow: show > 0.5,
        solid: true,
    }
}

// "bent copy", written in the print's bottom border, posed by `pose`, in a
// scene node of `node`'s size.
#[composable(no_skip)]
pub(super) fn PrintLabel(node: Size, plan: Plan, pose: impl Fn() -> Figure + Copy + 'static) {
    let u = plan.u;
    let size = Size::new(196.0 * u, 44.0 * u);
    Box(
        Modifier::empty().required_size(size).mesh_surface(move || {
            let figure = pose();
            let at = Rect {
                x: 0.0,
                y: plan.sheet.height + (plan.margin_bottom - size.height) * 0.5,
                width: size.width,
                height: size.height,
            };
            let mesh = sheet_mesh(at, at, 16, |point: Point| figure.on_print(point, 0.6 * u));
            MeshSurface {
                mesh: Arc::new(mesh),
                transform: world_to_scene(node),
                material: Material::Unlit,
                casts_shadow: false,
                solid: false,
            }
        }),
        BoxSpec::default(),
        move || {
            Text("bent copy", Modifier::empty(), strong(34.0 * u, LABEL_INK));
        },
    );
}
