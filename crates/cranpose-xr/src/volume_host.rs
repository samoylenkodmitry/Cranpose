//! The one scene that draws the content of every volume of a headset.
//!
//! A volume composes its content where the app wrote it, but the headset draws all volumes
//! together: one scene has one depth buffer, so a nearer volume hides a farther one. Each volume
//! hands its content to the scene, which composes it under a node that carries the volume's pose.
//! The scene is the root of its own window of the composition, so the atlas of the panels does not
//! draw it. The headset sets the camera of one eye, draws, and sets the camera of the other eye.

use std::rc::Rc;

use cranpose::{
    Box, BoxSpec, Modifier, composable,
    space::{Light, Scene3d},
};
use cranpose_core::{key, remember};

#[cfg(feature = "gltf")]
use crate::controller_loaders::ControllerLoaders;
use crate::{
    callback_slot::invoke,
    controller_scene::ControllerNodes,
    controller_states::ControllerStates,
    default_light::DEFAULT_LIGHT,
    space::Space,
    volume_transform::scene_from_volume,
    volume_window::VolumeWindow,
    volumes::{VolumeContent, Volumes},
};

// The window root of the scene, which holds the content of every volume and the models of the
// controllers in use. It holds nothing while no volume exists, no controller shows a model and the
// frames no longer hold the scene. The loads of the controllers' models belong here, because they
// go on while the scene is empty.
#[composable]
pub(crate) fn VolumeScene(space: Space) {
    let volumes = space.volumes();
    let controllers = space.volumes().controllers();
    #[cfg(feature = "gltf")]
    ControllerLoaders(controllers);
    let contents = volumes.contents().get();
    if contents.is_empty() && !controllers.any_shown() && !volumes.held().get() {
        return;
    }
    let window = remember(|| Rc::new(VolumeWindow::new(space.scene_size()))).with(Rc::clone);
    Box(
        Modifier::empty().window_root(window),
        BoxSpec::default(),
        move || {
            let contents = contents.clone();
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .scene3d(move || scene_of(&volumes, &controllers)),
                BoxSpec::default(),
                move || {
                    for content in &contents {
                        let content = content.clone();
                        key(content.id, move || VolumeNode(content));
                    }
                    ControllerNodes(controllers);
                },
            );
        },
    );
}

// One volume's content, moved to the volume's pose. A volume that stands in no place composes
// nothing, so the scene draws nothing of it.
#[composable]
fn VolumeNode(content: VolumeContent) {
    if !content.shown.get() {
        content.node.set(None);
        return;
    }
    let pose = content.pose;
    let compose = content.compose;
    let node = Box(
        Modifier::empty().transform3d(move || scene_from_volume(&pose.get())),
        BoxSpec::default(),
        move || invoke(&compose, ()),
    );
    content.node.set(Some(node));
}

// The scene as it draws now: the eye's camera and the first light that a volume sets, turned from
// the volume's axes into the scene's. A scene has one light, so volumes share it. Where no volume
// sets one, the controllers' models stand in the default light. A draw reads the states, so a
// camera or a pose that changes draws again without composing. The samples are the ones that the
// headset's device takes: the renderer stores the layer's depth from a second pass of one sample.
fn scene_of(volumes: &Volumes, controllers: &ControllerStates) -> Scene3d {
    let light = volumes
        .contents()
        .read(|contents| {
            contents.iter().find_map(|content| {
                if !content.shown.get() {
                    return None;
                }
                let light = content.light.get()?;
                let turn = scene_from_volume(&content.pose.get());
                Some(Light {
                    direction: turn.transform_vector(light.direction),
                    ..light
                })
            })
        })
        .or_else(|| controllers.any_shown().then_some(DEFAULT_LIGHT));
    Scene3d {
        camera: volumes.camera().get(),
        light,
        eye: None,
        multisample: volumes.samples().get(),
    }
}

#[cfg(test)]
#[path = "tests/volume_host_tests.rs"]
mod tests;
