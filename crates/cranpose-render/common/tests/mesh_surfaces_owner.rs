// A layer drawn on two meshes takes input on both, and its owner, the first
// mesh, places it for the window: the bounds of its content are where the
// owner shows it, whichever mesh a pointer found it on.

use std::sync::Arc;

use cranpose_render_common::{HitTestTarget, RenderScene};
use cranpose_ui::{
    Modifier, Rect,
    space::{Material, Mesh, MeshSurface, MeshSurfaces, Multisample, Scene3d, Vec3},
    widgets::{Box, BoxSpec},
};

use crate::mesh_surface_hits::{PANEL, VIEWPORT, camera, centered, with_scene};

const OWNER_AT: Vec3 = Vec3::new(-200.0, 0.0, 0.0);
const OTHER_AT: Vec3 = Vec3::new(200.0, 60.0, 0.0);

// The window rect where a panel whose middle stands at `middle` on the
// plane z = 0 shows.
fn shown_at(middle: Vec3) -> Rect {
    let corner = |x: f32, y: f32| {
        camera()
            .project(
                centered(PANEL, middle).transform_point(Vec3::new(x, y, 0.0)),
                VIEWPORT,
            )
            .expect("the panel is in front of the eye")
    };
    let (low, high) = (corner(0.0, 0.0), corner(1.0, 1.0));
    Rect {
        x: low.x,
        y: low.y,
        width: high.x - low.x,
        height: high.y - low.y,
    }
}

#[test]
fn the_owner_places_the_content_of_a_layer_on_two_meshes() {
    let mut composition = cranpose_ui::run_test_composition(move || {
        let plane = Arc::new(Mesh::plane());
        Box(
            Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                camera: camera(),
                light: None,
                eye: None,
                multisample: Multisample::Off,
            }),
            BoxSpec::default(),
            move || {
                let plane = Arc::clone(&plane);
                Box(
                    Modifier::empty()
                        .required_size(PANEL)
                        .mesh_surfaces(move || {
                            let placed = |middle| MeshSurface {
                                mesh: Arc::clone(&plane),
                                transform: centered(PANEL, middle),
                                material: Material::Unlit,
                                casts_shadow: false,
                                solid: true,
                            };
                            MeshSurfaces::new(placed(OWNER_AT), vec![placed(OTHER_AT)])
                        }),
                    BoxSpec::default(),
                    || {
                        Box(
                            Modifier::empty().fill_max_size().clickable(|_| {}),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    });
    let (owner, other) = (shown_at(OWNER_AT), shown_at(OTHER_AT));
    let bounds = with_scene(&mut composition, |scene, _, _| {
        [owner, other].map(|shown| {
            let (x, y) = (shown.x + shown.width * 0.5, shown.y + shown.height * 0.5);
            let target = scene
                .hit_test(x, y)
                .into_iter()
                .next()
                .expect("the content takes a press on each mesh");
            let node = target.node_id();
            let mut node_bounds = None;
            scene.node_window_bounds(&mut |visited, rect| {
                if visited == node {
                    node_bounds = Some(rect);
                }
            });
            [scene.surface_window_bounds(node), node_bounds]
        })
    });
    for found in bounds.into_iter().flatten() {
        let found = found.expect("the content has window bounds");
        let off = (found.x - owner.x).abs()
            + (found.y - owner.y).abs()
            + (found.width - owner.width).abs()
            + (found.height - owner.height).abs();
        assert!(
            off < 1.0,
            "the content's window bounds {found:?} are where the owner shows it, {owner:?}"
        );
    }
}
