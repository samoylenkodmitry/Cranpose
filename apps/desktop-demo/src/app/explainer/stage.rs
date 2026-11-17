// A stage: one window-high page of the tab, which a scene plays on. Its
// scene's camera shows the plane z = 0 at its own size, as a flat page
// shows it, so a composable laid on that plane lands where layout put it.

use std::sync::Arc;

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Camera, Light, Mat4, Material, Mesh, MeshSurface, Multisample, Scene3d, Vec3},
    Box, BoxSpec, Color, Modifier, Size,
};

// The camera's field of view up and down: narrow, so things near the wall
// keep their size.
const FIELD_OF_VIEW: f32 = 0.42;

// The light of a stage lit as a room is: high on the left, in front.
pub(super) const STAGE_LIGHT: Light = Light {
    direction: Vec3::new(0.42, 0.55, 0.72),
    ambient: 0.74,
    shadows: true,
    size: 0.45,
};

// The camera that shows the plane z = 0 over `size`, from in front.
pub(super) fn stage_camera(size: Size) -> Camera {
    let focal_length = Camera::focal_length_for(size.height.max(1.0), FIELD_OF_VIEW);
    let middle = Vec3::new(size.width * 0.5, size.height * 0.5, 0.0);
    Camera::look_at(
        middle - Vec3::new(0.0, 0.0, focal_length),
        middle,
        Vec3::new(0.0, 1.0, 0.0),
        focal_length,
    )
}

// A stage `size` big that shows `content` in a scene lit by `light`: what
// `content` lays on meshes stands in front of the wall.
#[composable(no_skip)]
pub(super) fn SceneStage(size: Size, light: Option<Light>, content: impl FnMut() + 'static) {
    StageIn(
        size,
        move || Scene3d {
            camera: stage_camera(size),
            light,
            eye: None,
            multisample: Multisample::X4,
        },
        content,
    );
}

// A stage `size` big that shows `content` in `scene`, read each time the
// scene is drawn.
#[composable(no_skip)]
pub(super) fn StageIn(
    size: Size,
    scene: impl Fn() -> Scene3d + 'static,
    content: impl FnMut() + 'static,
) {
    Box(
        Modifier::empty().required_size(size).scene3d(scene),
        BoxSpec::default(),
        content,
    );
}

// The stage's back wall, a flat `color` that takes the shadows cast on it.
#[composable(no_skip)]
pub(super) fn Wall(size: Size, color: Color) {
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0)),
                material: Material::Unlit,
                casts_shadow: false,
                solid: true,
            })
            .background(color),
        BoxSpec::default(),
        || {},
    );
}
