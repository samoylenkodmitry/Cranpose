//! 3D content in a subspace: Cranpose's own scene, drawn by a headset once for each eye.

use std::{cell::RefCell, rc::Rc};

use cranpose::{
    Box, BoxSpec, Modifier, PointerEvent, composable,
    space::{Camera, Light, Multisample, Scene3d, Vec3},
};
use cranpose_core::{
    DisposableEffect, DisposableEffectResult, MutableState, mutableStateOf, remember,
};

use crate::{
    SubspaceModifier,
    callback_slot::{Callback, invoke},
    space::{Space, local_space, local_subspace_origin},
    subspace_modifier::DEFAULT_VOLUME_SIZE,
    surface_input::PointerSink,
    volumes::VolumeContent,
};

// The callback that the content registered for the pointer's events.
#[derive(Default)]
struct PointerSlot(RefCell<Option<Callback<PointerEvent>>>);

/// The scope of the content of a [`SpatialVolume`], where the content sets its light and the
/// callback for a controller's ray. Jetpack XR has no such scope.
pub struct SpatialVolumeScope {
    pointer: Rc<PointerSlot>,
    light: MutableState<Option<Light>>,
}

impl SpatialVolumeScope {
    /// Sets the light that shines on the volume's meshes, in the volume's own axes: x to the right,
    /// y down and z away from the viewer. A volume that sets none has no light in its scene.
    ///
    /// A headset draws every volume in one scene, which has one light. The scene takes the light
    /// of the first volume that sets one and turns it with that volume's pose. A window draws each
    /// volume in a scene of its own, with that volume's light.
    pub fn light(&self, light: Light) {
        self.light.set(Some(light));
    }

    /// Sets the callback that receives the pointer events of a controller's ray on the volume's
    /// box, as Compose's pointer input gives them to a composable. A ray stops at the box whether
    /// or not the callback is set. The positions are in dp from the top left corner of the
    /// box's front face, seen from the front, along its width and height: the place where the ray
    /// entered the box, projected onto that face. The depth is not reported. The events are
    /// `Enter` when the ray first meets the box, `Move` while it moves over it, `Down` when the
    /// trigger goes down, `Up` when it goes up and `Exit` when the ray leaves. An `Up` comes also
    /// when the ray left the box during the press, or the controller was lost.
    ///
    /// The composables of the content do not receive the ray: it does not pick the meshes. A
    /// window shows the content under the mouse, where the meshes take pointer input as
    /// [`mesh_surface`](cranpose::Modifier::mesh_surface) documents, and never runs this callback.
    /// Jetpack XR gives such input through the `interactionPolicy` parameter of its panels.
    pub fn on_pointer_event(&self, callback: impl FnMut(PointerEvent) + 'static) {
        *self.pointer.0.borrow_mut() = Some(Box::new(callback));
    }
}

/// 3D content in a box of the size that `modifier` gives, centered at the modifier's pose in its
/// subspace.
///
/// Jetpack XR shows 3D content through `SpatialGltfModel`, which renders a glTF model, and
/// `SceneCoreEntity`, which attaches any SceneCore entity. Its current subspace package has no
/// `Volume`. Cranpose has no SceneCore entities, so its composable has its own name, after the box
/// that the content stands in. A glTF model has its own composable, `SpatialGltfModel`, which the
/// `gltf` feature adds. It is a volume whose content is the model.
///
/// The content is ordinary composables whose nodes carry
/// [`mesh_surface`](cranpose::Modifier::mesh_surface) or
/// [`mesh_surfaces`](cranpose::Modifier::mesh_surfaces). Cranpose's own 3D scene lays them out and
/// draws them, as a [`scene3d`](cranpose::Modifier::scene3d) does. A mesh comes from
/// [`Mesh`](cranpose::space::Mesh), such as `Mesh::from_obj`, and shows the composable content of
/// its node on its surface.
///
/// The content's space has its origin at the box's center, x to the right, y down and z away from
/// the viewer, and a unit is a dp, which is a millimeter in a subspace. The box is `modifier`'s
/// [`width`](SubspaceModifier::width), [`height`](SubspaceModifier::height) and
/// [`depth`](SubspaceModifier::depth), 500 dp each by default. The box is where a controller's ray
/// stops. The content is not scaled to the box and may reach outside it. The camera cuts away
/// what lies nearer to the eye than a 64th of its focal length in pixels: 12 mm for a focal length
/// of 800 pixels.
///
/// A headset draws the content of all volumes together, into one projection layer with a view for
/// each eye. The content composes once. Each eye then draws it from that eye's own pose and field
/// of view, with no composition in between. The scene draws with one sample for each pixel, so
/// the edges of a mesh are not smoothed. Its depth buffer hides a farther mesh behind a nearer
/// one, also between volumes. The layer carries that depth where the runtime takes it. While the
/// app has no volume, the headset draws no scene and makes no layer.
///
/// The composables of the content compose at the root of the volumes' scene, which is a window of
/// the app's composition. They share the composition's state. A composition local that the app
/// provides between the subspace and the volume does not reach them. The headset provides its own
/// locals there.
///
/// The modifier's [`movable`](SubspaceModifier::movable) and
/// [`resizable`](SubspaceModifier::resizable) work as on a [`SpatialPanel`](crate::SpatialPanel).
/// A handle at the box's front face under it carries the box, and one at its bottom right corner
/// resizes its width and height. A resize changes the box and not the content. The content's size
/// follows the state that the app feeds into the modifier.
///
/// In a window the volume shows as a box of its width and height in the subspace's row, with a
/// camera in front of it that sees the front face at its size.
#[composable]
pub fn SpatialVolume(
    modifier: SubspaceModifier,
    content: impl FnMut(&SpatialVolumeScope) + 'static,
) {
    let size = modifier.size_3d_or(DEFAULT_VOLUME_SIZE);
    match local_space().current() {
        Some(space) => PlacedVolume(space, modifier, size, content),
        None => FlatVolume(size, content),
    }
}

#[composable]
fn PlacedVolume(
    space: Space,
    modifier: SubspaceModifier,
    size: [f32; 3],
    content: impl FnMut(&SpatialVolumeScope) + 'static,
) {
    let panel = remember({
        let space = space.clone();
        move || space.new_panel()
    })
    .with(|panel| *panel);
    let origin = local_subspace_origin().current();
    space.place_volume(panel, size, origin, origin.then(&modifier.pose()));
    let id = panel.id;
    space.set_move_policy(id, modifier.move_policy());
    space.set_point_source(id, modifier.attached_point_source());
    space.set_resizable(id, modifier.resize());
    let parts = rememberScopeParts();
    // The sink stays for the volume's life. A volume with no callback takes no events.
    let sink = remember({
        let pointer = Rc::clone(&parts.pointer);
        move || {
            let sink: PointerSink = Rc::new(move |event| invoke(&pointer.0, event));
            sink
        }
    })
    .with(Clone::clone);
    space.set_surface_sink(id, Some(sink));
    // The scene composes the content at its own root, with the scope that this call made
    let mut content = content;
    let light = parts.light;
    let compose: Callback<()> = Box::new(move |()| content(&parts.scope()));
    space.publish_volume(VolumeContent {
        id,
        pose: panel.volume_pose,
        light,
        compose: Rc::new(RefCell::new(Some(compose))),
    });
    DisposableEffect(id, move |_| {
        DisposableEffectResult::new(move || space.remove(id))
    });
}

// The parts of a scope that a volume keeps while it stays in the composition.
#[derive(Clone)]
struct ScopeParts {
    pointer: Rc<PointerSlot>,
    light: MutableState<Option<Light>>,
}

#[composable]
fn rememberScopeParts() -> ScopeParts {
    remember(|| ScopeParts {
        pointer: Rc::new(PointerSlot::default()),
        light: mutableStateOf(None),
    })
    .with(Clone::clone)
}

impl ScopeParts {
    fn scope(&self) -> SpatialVolumeScope {
        SpatialVolumeScope {
            pointer: Rc::clone(&self.pointer),
            light: self.light,
        }
    }
}

// A volume in a window: a box of its size with a camera in front of its content.
#[composable]
fn FlatVolume(size: [f32; 3], content: impl FnMut(&SpatialVolumeScope) + 'static) {
    let parts = rememberScopeParts();
    let light = parts.light;
    let camera = flat_camera(size);
    let mut content = content;
    Box(
        Modifier::empty()
            .size_points(size[0], size[1])
            .scene3d(move || Scene3d {
                camera,
                light: light.get(),
                eye: None,
                multisample: Multisample::X4,
            }),
        BoxSpec::default(),
        move || content(&parts.scope()),
    );
}

// The camera that sees a volume's box from the front. Its focal length is one and a half times the
// larger of the box's width and height. It stands that far in front of the box's front face, so the
// face shows at its size and the content behind it shows smaller with depth.
fn flat_camera(size: [f32; 3]) -> Camera {
    let focal_length = size[0].max(size[1]).max(1.0) * 1.5;
    Camera::look_at(
        Vec3::new(0.0, 0.0, -(focal_length + size[2] / 2.0)),
        Vec3::ZERO,
        Vec3::new(0.0, 1.0, 0.0),
        focal_length,
    )
}
