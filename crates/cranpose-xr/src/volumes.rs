//! The volumes of a space: what each one hands to the scene that draws them.

use std::{cell::RefCell, rc::Rc};

use cranpose::space::{Camera, Light, Mat4};
use cranpose_core::{MutableState, mutableStateOf};

use crate::{Pose, callback_slot::Callback};

// What the scene needs of one volume.
#[derive(Clone)]
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the headset's scene reads the pose and the light")
)]
pub(crate) struct VolumeContent {
    pub(crate) id: u64,
    // The volume's pose in the room, which the scene reads while it draws, so a volume that moves
    // composes nothing
    pub(crate) pose: MutableState<Pose>,
    // The light that the volume's content sets, in the volume's own axes
    pub(crate) light: MutableState<Option<Light>>,
    // Composes the volume's content
    pub(crate) compose: Rc<RefCell<Option<Callback<()>>>>,
}

impl PartialEq for VolumeContent {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Rc::ptr_eq(&self.compose, &other.compose)
    }
}

// The volumes of a space and the camera that the scene draws them through.
#[derive(Clone, Copy)]
pub(crate) struct Volumes {
    contents: MutableState<Vec<VolumeContent>>,
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene reads it")
    )]
    camera: MutableState<Camera>,
}

impl Volumes {
    // No volume. A composition makes it, because the states belong to the composition's runtime.
    pub(crate) fn new() -> Self {
        Self {
            contents: mutableStateOf(Vec::new()),
            camera: mutableStateOf(Camera {
                view: Mat4::IDENTITY,
                focal_length: 1.0,
            }),
        }
    }

    // The scene takes the content of the volume `content.id`, or replaces what it took.
    pub(crate) fn set(&self, content: VolumeContent) {
        self.contents.update(|contents| {
            match contents.iter_mut().find(|known| known.id == content.id) {
                Some(known) => *known = content,
                None => contents.push(content),
            }
        });
    }

    // The scene forgets the volume `id`.
    pub(crate) fn remove(&self, id: u64) {
        self.contents
            .update(|contents| contents.retain(|content| content.id != id));
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene calls it")
    )]
    // The content of each volume, which a reader of the state sees change.
    pub(crate) fn contents(&self) -> MutableState<Vec<VolumeContent>> {
        self.contents
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene calls it")
    )]
    // The camera of the eye that the next draw shows, which a draw reads.
    pub(crate) fn camera(&self) -> MutableState<Camera> {
        self.camera
    }
}
