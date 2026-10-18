use std::sync::Arc;

use cranpose_ui_graphics::{
    Point, Size,
    space::{Camera, Mat4, Material, Mesh, MeshError, MeshSurface, ObjErrorKind, Vec3},
};

const VIEWPORT: Size = Size {
    width: 800.0,
    height: 600.0,
};
const CONTENT: Size = Size {
    width: 400.0,
    height: 300.0,
};

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 0.05 && (a.y - b.y).abs() < 0.05
}

fn camera() -> Camera {
    Camera::look_at(
        Vec3::new(120.0, -80.0, -900.0),
        Vec3::new(0.0, 0.0, 200.0),
        Vec3::new(0.0, 1.0, 0.0),
        700.0,
    )
}

/// A plane surface of `CONTENT`'s size, turned and moved into the scene.
fn wall() -> MeshSurface {
    MeshSurface {
        mesh: Arc::new(Mesh::plane()),
        transform: Mat4::scaling(Vec3::new(CONTENT.width, CONTENT.height, 1.0))
            .then(&Mat4::rotation_y(0.6))
            .then(&Mat4::translation(Vec3::new(-150.0, -100.0, 300.0))),
        material: Material::Unlit,
        casts_shadow: false,
    }
}

#[test]
fn a_flat_surface_maps_its_content_where_the_camera_sees_it() {
    let camera = camera();
    let wall = wall();
    let transform = wall
        .content_transform(&camera, VIEWPORT, CONTENT)
        .expect("a plane has a texture plane");
    for content in [
        Point::new(0.0, 0.0),
        Point::new(400.0, 0.0),
        Point::new(0.0, 300.0),
        Point::new(250.0, 120.0),
    ] {
        let scene = wall
            .point_at(content, CONTENT)
            .expect("the plane covers its content");
        let seen = camera
            .project(scene, VIEWPORT)
            .expect("in front of the eye");
        let mapped = transform.map_point(content);
        assert!(
            close(mapped, seen),
            "content point {content:?} maps to {mapped:?}, but the camera sees it at {seen:?}"
        );
    }
}

#[test]
fn a_camera_one_focal_length_from_a_facing_plane_shows_it_at_its_own_size() {
    let focal_length = 700.0;
    let camera = Camera::look_at(
        Vec3::new(VIEWPORT.width * 0.5, VIEWPORT.height * 0.5, -focal_length),
        Vec3::new(VIEWPORT.width * 0.5, VIEWPORT.height * 0.5, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        focal_length,
    );
    let screen = MeshSurface {
        mesh: Arc::new(Mesh::plane()),
        transform: Mat4::scaling(Vec3::new(VIEWPORT.width, VIEWPORT.height, 1.0)),
        material: Material::Unlit,
        casts_shadow: false,
    };
    let transform = screen
        .content_transform(&camera, VIEWPORT, VIEWPORT)
        .expect("a plane has a texture plane");
    for point in [
        Point::new(0.0, 0.0),
        Point::new(800.0, 600.0),
        Point::new(321.0, 77.0),
    ] {
        assert!(
            close(transform.map_point(point), point),
            "{point:?} moved to {:?}",
            transform.map_point(point)
        );
    }
}

#[test]
fn the_ray_through_a_shown_point_meets_the_content_point_shown_there() {
    let camera = camera();
    let wall = wall();
    let content = Point::new(310.0, 45.0);
    let shown = camera
        .project(
            wall.point_at(content, CONTENT).expect("on the plane"),
            VIEWPORT,
        )
        .expect("in front of the eye");
    let ray = camera.ray(shown, VIEWPORT).expect("the view inverts");
    let hit = wall.cast(ray, CONTENT).expect("the ray meets the wall");
    assert!(
        close(hit.point, content),
        "met {:?} instead of {content:?}",
        hit.point
    );
}

#[test]
fn a_ray_meets_the_nearer_of_two_surfaces_first() {
    let camera = camera();
    let far = wall();
    let near = MeshSurface {
        transform: far
            .transform
            .then(&Mat4::translation(Vec3::new(0.0, 0.0, -200.0))),
        ..far.clone()
    };
    let shown = camera
        .project(
            far.point_at(Point::new(200.0, 150.0), CONTENT)
                .expect("on the plane"),
            VIEWPORT,
        )
        .expect("in front of the eye");
    let ray = camera.ray(shown, VIEWPORT).expect("the view inverts");
    let (near_hit, far_hit) = (near.cast(ray, CONTENT), far.cast(ray, CONTENT));
    let (Some(near_hit), Some(far_hit)) = (near_hit, far_hit) else {
        panic!("the ray meets both walls: {near_hit:?} {far_hit:?}");
    };
    assert!(near_hit.distance < far_hit.distance);
}

#[test]
fn a_drag_past_a_flat_surface_goes_on_along_its_plane() {
    let camera = camera();
    let wall = wall();
    let past_the_edge = Point::new(CONTENT.width + 60.0, 100.0);
    let scene = wall.transform.transform_point(Vec3::new(
        past_the_edge.x / CONTENT.width,
        past_the_edge.y / CONTENT.height,
        0.0,
    ));
    let shown = camera
        .project(scene, VIEWPORT)
        .expect("in front of the eye");
    let ray = camera.ray(shown, VIEWPORT).expect("the view inverts");
    assert!(wall.cast(ray, CONTENT).is_none(), "the ray misses the wall");
    let held = wall
        .cast(
            camera
                .ray(
                    camera
                        .project(
                            wall.point_at(Point::new(390.0, 100.0), CONTENT)
                                .expect("on the plane"),
                            VIEWPORT,
                        )
                        .expect("in front of the eye"),
                    VIEWPORT,
                )
                .expect("the view inverts"),
            CONTENT,
        )
        .expect("the drag starts on the wall");
    let dragged = wall
        .cast_plane(ray, held.triangle, CONTENT)
        .expect("the ray meets the wall's plane");
    assert!(close(dragged, past_the_edge), "dragged to {dragged:?}");
}

#[test]
fn nothing_shows_behind_the_eye() {
    let camera = camera();
    let behind = camera.eye() - Vec3::new(0.0, 0.0, 50.0);
    assert!(camera.project(behind, VIEWPORT).is_none());
}

#[test]
fn a_sphere_faces_the_viewer_with_its_texture_middle() {
    let sphere = Mesh::sphere(32, 16);
    let plane = sphere
        .texture_plane()
        .expect("the sphere's middle lies on it");
    let middle = plane.at([0.5, 0.5]);
    assert!((middle - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-3);
    assert!(
        plane.u_axis.x > 0.0 && plane.v_axis.y > 0.0,
        "the texture reads left to right and top to bottom on the sphere's front"
    );
}

#[test]
fn an_obj_quad_makes_two_triangles_with_its_texture_counted_from_the_top() {
    let mesh = Mesh::from_obj(
        "# a square\n\
         v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\n\
         vt 0 1\nvt 1 1\nvt 1 0\nvt 0 0\n\
         usemtl ignored\n\
         f 1/1 2/2 3/3 4/4\n",
    )
    .expect("a valid file");
    assert_eq!(mesh.indices().len(), 6);
    assert_eq!(mesh.uvs()[0], [0.0, 0.0], "OBJ's v counts from the bottom");
    assert_eq!(mesh.uvs()[2], [1.0, 1.0]);
}

#[test]
fn negative_obj_indices_count_back_from_the_last_vertex() {
    let mesh = Mesh::from_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf -3 -2 -1\n").expect("a valid file");
    assert_eq!(
        mesh.positions(),
        &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(
        mesh.uvs(),
        &[[0.0, 0.0]; 3],
        "a face without texture coordinates"
    );
    let normal = Vec3::from_array(mesh.normals()[0]);
    assert!(
        (normal.length() - 1.0).abs() < 1e-5,
        "smooth normals are made"
    );
}

#[test]
fn an_obj_face_that_names_a_missing_vertex_reports_its_line() {
    let error = Mesh::from_obj("v 0 0 0\nv 1 0 0\n\nf 1 2 3\n").expect_err("vertex 3 is missing");
    assert_eq!(error.line, 4);
    assert_eq!(error.kind, ObjErrorKind::IndexOutOfRange);
}

#[test]
fn a_mesh_whose_parts_disagree_is_refused() {
    let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    assert_eq!(
        Mesh::new(positions.clone(), vec![[0.0, 0.0]; 2], None, vec![0, 1, 2]).err(),
        Some(MeshError::AttributeCountMismatch)
    );
    assert_eq!(
        Mesh::new(positions, vec![[0.0, 0.0]; 3], None, vec![0, 1, 3]).err(),
        Some(MeshError::IndexOutOfRange)
    );
}

const SDF_RADIUS: f32 = 0.8;

fn sdf_sphere() -> Mesh {
    Mesh::from_sdf(
        |point| point.length() - SDF_RADIUS,
        (Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)),
        24,
    )
    .expect("the sphere's surface lies in the box")
}

#[test]
fn a_signed_distance_field_gives_its_surface_with_outward_normals() {
    let mesh = sdf_sphere();
    let cell = 2.0 / 24.0;
    for (position, normal) in mesh.positions().iter().zip(mesh.normals()) {
        let position = Vec3::from_array(*position);
        assert!(
            (position.length() - SDF_RADIUS).abs() < cell * 0.5,
            "{position:?} lies on the sphere within half a cell"
        );
        assert!(
            Vec3::from_array(*normal).dot(position) > 0.0,
            "the normal at {position:?} points out of the sphere"
        );
    }
    // A ray from the viewer's side meets the surface where the sphere is.
    let hit = mesh
        .cast(Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 1.0))
        .expect("the ray meets the surface");
    assert!((hit.distance - (5.0 - SDF_RADIUS)).abs() < cell);
    assert!(
        (hit.uv[0] - 0.5).abs() < 0.05 && (hit.uv[1] - 0.5).abs() < 0.05,
        "the texture's middle faces the viewer: {:?}",
        hit.uv
    );
}

#[test]
fn a_field_without_a_surface_in_its_box_gives_no_mesh() {
    let empty = Mesh::from_sdf(
        |point| point.length() + 1.0,
        (Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0)),
        8,
    );
    assert_eq!(empty.err(), Some(MeshError::Empty));
}

#[test]
fn a_surface_content_space_lies_on_its_mesh_with_depth_into_it() {
    let wall = wall();
    let to_scene = wall
        .content_to_scene(CONTENT)
        .expect("a plane has a content space");
    let content = Point::new(130.0, 70.0);
    let on_mesh = wall.point_at(content, CONTENT).expect("on the wall");
    let mapped = to_scene.transform_point(Vec3::new(content.x, content.y, 0.0));
    assert!(
        (mapped - on_mesh).length() < 1e-3,
        "content point {content:?} maps to {mapped:?}, not {on_mesh:?}"
    );
    // The wall is a content pixel to a scene pixel, so 50 units of depth go
    // 50 scene pixels into it, away from the side the camera sees it from.
    let deeper = to_scene.transform_point(Vec3::new(content.x, content.y, 50.0));
    assert!(((deeper - mapped).length() - 50.0).abs() < 1e-3);
    let eye = camera().eye();
    assert!(
        (deeper - eye).length() > (mapped - eye).length(),
        "depth goes into the wall, away from the eye"
    );
    let raised = wall.placed_by(&to_scene);
    assert_eq!(
        raised.transform,
        wall.transform.then(&to_scene),
        "a surface placed by the space takes its transform after its own"
    );
}
