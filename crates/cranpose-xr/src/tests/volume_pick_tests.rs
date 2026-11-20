// Rays against the meshes of a volume, as the pointer asks: the volume stands a meter ahead of a
// controller at the origin, so a ray that points straight ahead crosses the volume's content plane
// z = 0 a meter along. A dp is a millimeter.

use cranpose::space::{Material, Mesh, MeshSurface};
use cranpose_core::NodeId;

use super::*;
use crate::{Pose, Ray};

const VOLUME: u64 = 7;

fn volume_pose() -> Pose {
    Pose::at([0.0, 0.0, -1.0])
}

// A triangle of 200 dp in the content's xy plane, with its corner at (-100, -100): it covers the
// lower left half of the square from (-100, -100) to (100, 100).
fn triangle() -> Arc<Mesh> {
    Arc::new(
        Mesh::new(
            vec![
                [-100.0, -100.0, 0.0],
                [100.0, -100.0, 0.0],
                [-100.0, 100.0, 0.0],
            ],
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            None,
            vec![0, 1, 2],
        )
        .expect("a triangle"),
    )
}

// A square of two triangles from (-100, -100) to (100, 100) with a diagonal from the upper right
// corner to the lower left one.
fn square() -> Arc<Mesh> {
    Arc::new(
        Mesh::new(
            vec![
                [-100.0, -100.0, 0.0],
                [100.0, -100.0, 0.0],
                [100.0, 100.0, 0.0],
                [-100.0, 100.0, 0.0],
            ],
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            None,
            vec![0, 1, 2, 0, 2, 3],
        )
        .expect("a square"),
    )
}

// The meshes `(node, mesh, to_content)` as a scene's visit reports them.
fn picks_of(meshes: &[(usize, Arc<Mesh>, Mat4)]) -> PickMeshes {
    let mut picks = PickMeshes::default();
    picks.replace(VOLUME, |visit| {
        for (index, (node, mesh, to_content)) in meshes.iter().enumerate() {
            let surface = MeshSurface {
                mesh: Arc::clone(mesh),
                transform: Mat4::IDENTITY,
                material: Material::Unlit,
                casts_shadow: false,
                solid: true,
            };
            visit(&MeshVisit {
                node: Some(NodeId(*node)),
                mesh: index,
                surface: &surface,
                to_frame: *to_content,
            });
        }
    });
    picks
}

fn at(x: f32, y: f32, z: f32) -> Mat4 {
    Mat4::translation(Vec3::new(x, y, z))
}

// The ray of a controller at `(x, y)` meters that points straight ahead.
fn ahead(x: f32, y: f32) -> ContentRay {
    ContentRay::of(
        &Ray {
            origin: [x, y, 0.0],
            direction: [0.0, 0.0, -1.0],
        },
        &volume_pose(),
    )
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-3, "{a} is not {b}");
}

#[test]
fn a_ray_that_meets_a_triangle_gives_the_distance_the_triangle_and_the_place_in_the_content() {
    let picks = picks_of(&[(3, triangle(), Mat4::IDENTITY)]);
    // 5 cm right of the middle and 3 cm up (content y runs down), inside the triangle
    let hit = picks
        .pick(VOLUME, &ahead(-0.05, 0.03), None)
        .expect("the ray meets the triangle");
    near(hit.distance, 1.0);
    assert_eq!(hit.triangle, 0);
    assert_eq!(
        hit.mesh,
        MeshRef {
            node: NodeId(3),
            index: 0
        }
    );
    near(hit.position[0], -50.0);
    near(hit.position[1], -30.0);
    near(hit.position[2], 0.0);
}

#[test]
fn a_ray_beside_a_triangle_or_past_its_hypotenuse_misses() {
    let picks = picks_of(&[(3, triangle(), Mat4::IDENTITY)]);
    // The triangle covers the points where x + y <= 0 in the content, with y down. A controller
    // 20 cm to the right points outside the triangle's box, and one 5 cm to the right of the
    // middle and 5 cm down points into the box but past the hypotenuse.
    assert_eq!(picks.pick(VOLUME, &ahead(0.2, 0.0), None), None);
    assert_eq!(picks.pick(VOLUME, &ahead(0.05, -0.05), None), None);
}

#[test]
fn the_nearer_of_two_meshes_takes_the_ray() {
    // The second mesh stands 100 dp farther from the viewer, so the ray meets it later
    let picks = picks_of(&[
        (3, square(), at(0.0, 0.0, 100.0)),
        (4, square(), Mat4::IDENTITY),
    ]);
    let hit = picks.pick(VOLUME, &ahead(0.0, 0.0), None).expect("a hit");
    assert_eq!(hit.mesh.node, NodeId(4));
    near(hit.distance, 1.0);
    // The mesh that a press began on takes the ray, though a nearer one lies in front of it
    let pressed = MeshRef {
        node: NodeId(3),
        index: 0,
    };
    let held = picks
        .pick(VOLUME, &ahead(0.0, 0.0), Some(pressed))
        .expect("a hit");
    assert_eq!(held.mesh, pressed);
    near(held.distance, 1.1);
}

#[test]
fn a_ray_on_the_edge_between_two_triangles_hits() {
    let picks = picks_of(&[(3, square(), Mat4::IDENTITY)]);
    // The square's diagonal runs through the middle, where the ray points
    let hit = picks.pick(VOLUME, &ahead(0.0, 0.0), None);
    assert!(hit.is_some(), "the ray slipped through the diagonal");
    // And a ray on the square's outer edge hits too
    assert!(picks.pick(VOLUME, &ahead(0.1, 0.0), None).is_some());
}

#[test]
fn a_mesh_behind_the_ray_origin_is_not_hit() {
    let picks = picks_of(&[(3, square(), Mat4::IDENTITY)]);
    // The controller stands behind the volume and points away from it
    let away = ContentRay::of(
        &Ray {
            origin: [0.0, 0.0, -2.0],
            direction: [0.0, 0.0, -1.0],
        },
        &volume_pose(),
    );
    assert_eq!(picks.pick(VOLUME, &away, None), None);
    // Pointing at it from there hits it, from the back
    let back = ContentRay::of(
        &Ray {
            origin: [0.0, 0.0, -2.0],
            direction: [0.0, 0.0, 1.0],
        },
        &volume_pose(),
    );
    near(
        picks
            .pick(VOLUME, &back, None)
            .expect("a hit from behind")
            .distance,
        1.0,
    );
}

#[test]
fn a_mesh_that_a_node_turns_and_moves_is_hit_where_the_node_puts_it() {
    // The square stands on its edge, turned a quarter about y, 60 dp to the right of the center and
    // 30 dp toward the viewer, where the room's z is 3 cm nearer than the volume's center
    let placed = Mat4::rotation_y(std::f32::consts::FRAC_PI_2).then(&at(60.0, 0.0, -30.0));
    let picks = picks_of(&[(3, square(), placed)]);
    // Edge on to a ray along z
    assert_eq!(picks.pick(VOLUME, &ahead(-0.06, 0.0), None), None);
    // A ray along x, from the right, through the turned square's middle
    let along_x = ContentRay::of(
        &Ray {
            origin: [1.0, 0.0, -0.97],
            direction: [-1.0, 0.0, 0.0],
        },
        &volume_pose(),
    );
    // Content x is room x, and content z is room -z, so the room's -x points along the content's -x
    let hit = picks
        .pick(VOLUME, &along_x, None)
        .expect("the ray meets the turned square");
    near(hit.position[0], 60.0);
    near(hit.position[2], -30.0);
}

#[test]
fn a_mesh_with_no_inverse_takes_no_ray_and_a_volume_without_meshes_has_none() {
    let flat = Mat4::scaling(Vec3::new(1.0, 1.0, 0.0));
    let picks = picks_of(&[(3, square(), flat)]);
    assert!(!picks.has_meshes(VOLUME));
    assert_eq!(picks.pick(VOLUME, &ahead(0.0, 0.0), None), None);
}

#[test]
fn the_meshes_of_one_volume_are_replaced_without_touching_another() {
    let mut picks = picks_of(&[(3, square(), Mat4::IDENTITY)]);
    picks.replace(8, |visit| {
        let surface = MeshSurface {
            mesh: square(),
            transform: Mat4::IDENTITY,
            material: Material::Unlit,
            casts_shadow: false,
            solid: true,
        };
        visit(&MeshVisit {
            node: Some(NodeId(9)),
            mesh: 0,
            surface: &surface,
            to_frame: Mat4::IDENTITY,
        });
    });
    assert!(picks.has_meshes(VOLUME) && picks.has_meshes(8));
    // The volume's meshes come again from a visit that finds none
    picks.replace(VOLUME, |_| {});
    assert!(!picks.has_meshes(VOLUME) && picks.has_meshes(8));
}
