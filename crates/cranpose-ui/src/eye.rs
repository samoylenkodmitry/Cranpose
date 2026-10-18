//! The eye a window's 3D scene is seen from, as a state an app holds: it
//! turns as the app asks, and walks through the spaces behind the scene's
//! doorways, stopped by solid surfaces where the renderer last placed them.

use cranpose_core::{CompositionLocal, MutableState, compositionLocalOf, mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui_graphics::space::{EyePlace, EyePose, Light, Scene3d, Vec3, WorldSink};

/// The eye of a window's 3D scene.
///
/// Read in a [`scene3d`](crate::Modifier::scene3d) closure through
/// [`Eye::scene`], it moves the scene's camera without recomposition. The
/// scene tells the renderer where the eye stands among the spaces behind its
/// doorways, and the renderer leaves the eye what it can bump into there
/// each time it places the scene, which [`Eye::walk`] reads.
#[derive(Clone)]
pub struct Eye {
    pose: MutableState<EyePose>,
    world: WorldSink,
    radius: f32,
}

impl Eye {
    /// An eye at `pose`: a ball of `radius`, in the units of the space it
    /// stands in, that stops short of solid surfaces.
    pub fn new(pose: EyePose, radius: f32) -> Self {
        Self {
            pose: mutableStateOf(pose),
            world: WorldSink::default(),
            radius,
        }
    }

    /// Where the eye stands and which way it looks.
    pub fn pose(&self) -> EyePose {
        self.pose.get()
    }

    /// The scene seen through the eye: its camera, with `focal_length`, in
    /// the space it stands in, and `light`.
    pub fn scene(&self, focal_length: f32, light: Option<Light>) -> Scene3d {
        let pose = self.pose.get();
        Scene3d {
            camera: pose.camera(focal_length),
            light,
            eye: Some(EyePlace {
                doorways: pose.doorways,
                world: self.world.clone(),
            }),
        }
    }

    /// Walks the eye `step`, in the coordinates of the space it stands in:
    /// through the doorways the step passes, and pushed back from the solid
    /// surfaces of the space it walks into, along which it slides.
    pub fn walk(&self, step: Vec3) {
        let world = self.world.current();
        let radius = self.radius;
        self.pose
            .update(|pose| *pose = world.walk(pose, step, radius));
    }

    /// Turns the eye `yaw` radians to the right and `pitch` up (see
    /// [`EyePose::turn`]).
    pub fn turn(&self, yaw: f32, pitch: f32) {
        self.pose.update(|pose| pose.turn(yaw, pitch));
    }

    /// Puts the eye at `pose`, through no doorway and stopped by nothing.
    pub fn place(&self, pose: EyePose) {
        self.pose.set(pose);
    }
}

/// Two handles are equal when they hold the same eye.
impl PartialEq for Eye {
    fn eq(&self, other: &Self) -> bool {
        self.world == other.world
    }
}

/// An [`Eye`] kept across recompositions, made at `pose` the first time.
#[allow(non_snake_case)]
#[composable]
pub fn rememberEye(pose: impl FnOnce() -> EyePose, radius: f32) -> Eye {
    remember(move || Eye::new(pose(), radius)).with(Clone::clone)
}

/// CompositionLocal carrying the eye of the window scene the content lies
/// in, or `None` outside one.
///
/// An app that shows a scene through an [`Eye`] provides the eye around the
/// scene's content, so a scene composed behind one of its doorways knows the
/// window's eye walks into it and leaves its own camera standing in the
/// doorway.
pub fn local_eye() -> CompositionLocal<Option<Eye>> {
    crate::environment_locals::cached_local(|locals| &locals.eye, || compositionLocalOf(|| None))
}
