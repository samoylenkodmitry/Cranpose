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
    space::{Light, Multisample, Scene3d},
};
use cranpose_core::{key, remember};

use crate::{
    callback_slot::invoke,
    space::Space,
    volume_transform::scene_from_volume,
    volume_window::VolumeWindow,
    volumes::{VolumeContent, Volumes},
};

// The window root of the scene, which holds the content of every volume. It holds nothing while
// no volume exists.
#[composable]
pub(crate) fn VolumeScene(space: Space) {
    let volumes = space.volumes();
    let contents = volumes.contents().get();
    if contents.is_empty() {
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
                    .scene3d(move || scene_of(&volumes)),
                BoxSpec::default(),
                move || {
                    for content in &contents {
                        let content = content.clone();
                        key(content.id, move || VolumeNode(content));
                    }
                },
            );
        },
    );
}

// One volume's content, moved to the volume's pose.
#[composable]
fn VolumeNode(content: VolumeContent) {
    let pose = content.pose;
    let compose = content.compose;
    Box(
        Modifier::empty().transform3d(move || scene_from_volume(&pose.get())),
        BoxSpec::default(),
        move || invoke(&compose, ()),
    );
}

// The scene as it draws now: the eye's camera and the first light that a volume sets, turned from
// the volume's axes into the scene's. A scene has one light, so volumes share it. A draw reads the
// states, so a camera or a pose that changes draws again without composing. The scene's samples
// stay at one: the depth that the layer carries is the depth of a single sample.
fn scene_of(volumes: &Volumes) -> Scene3d {
    let light = volumes.contents().read(|contents| {
        contents.iter().find_map(|content| {
            let light = content.light.get()?;
            let turn = scene_from_volume(&content.pose.get());
            Some(Light {
                direction: turn.transform_vector(light.direction),
                ..light
            })
        })
    });
    Scene3d {
        camera: volumes.camera().get(),
        light,
        eye: None,
        multisample: Multisample::Off,
    }
}

#[cfg(test)]
#[path = "tests/volume_host_tests.rs"]
mod tests;
