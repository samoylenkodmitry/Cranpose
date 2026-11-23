//! The volumes of a space: what each one hands to the scene that draws them.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose::space::{Camera, Light, Mat4, Multisample};
use cranpose_core::{MutableState, NodeId, mutableStateOf};

use crate::{Pose, callback_slot::Callback};
#[cfg(any(test, headset))]
use crate::{controller_states::ControllerStates, space::Space};

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
    // Whether the volume stands in a place: the scene leaves out a volume that stands in none
    pub(crate) shown: MutableState<bool>,
    // The light that the volume's content sets, in the volume's own axes
    pub(crate) light: MutableState<Option<Light>>,
    // Composes the volume's content
    pub(crate) compose: Rc<RefCell<Option<Callback<()>>>>,
    // The node of the scene that holds the volume's content, which the scene's composition sets and
    // a ray's pick reads
    pub(crate) node: Rc<Cell<Option<NodeId>>>,
}

impl PartialEq for VolumeContent {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Rc::ptr_eq(&self.compose, &other.compose)
    }
}

// The volumes of a space and the camera that the scene draws them through, with the models of the
// controllers that the scene draws beside them.
#[derive(Clone, Copy)]
pub(crate) struct Volumes {
    contents: MutableState<Vec<VolumeContent>>,
    #[cfg(any(test, headset))]
    controllers: ControllerStates,
    // Whether the scene keeps its window while it has nothing to draw, which the frames set
    #[cfg(any(test, headset))]
    held: MutableState<bool>,
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene reads it")
    )]
    camera: MutableState<Camera>,
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene reads it")
    )]
    samples: MutableState<Multisample>,
}

impl Volumes {
    // No volume. A composition makes it, because the states belong to the composition's runtime.
    pub(crate) fn new() -> Self {
        Self {
            contents: mutableStateOf(Vec::new()),
            #[cfg(any(test, headset))]
            controllers: ControllerStates::new(),
            #[cfg(any(test, headset))]
            held: mutableStateOf(false),
            camera: mutableStateOf(Camera {
                view: Mat4::IDENTITY,
                focal_length: 1.0,
            }),
            samples: mutableStateOf(Multisample::Off),
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

    #[cfg(any(test, headset))]
    // The states of the controllers' models, which the scene draws.
    pub(crate) fn controllers(&self) -> ControllerStates {
        self.controllers
    }

    #[cfg(any(test, headset))]
    // Whether the scene keeps its window with nothing to draw. The frames hold it a short while
    // after its last content went. A composable that reads it composes again when that changes.
    pub(crate) fn held(&self) -> MutableState<bool> {
        self.held
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
        expect(dead_code, reason = "the headset's pointer calls it")
    )]
    // The node of the scene that holds the content of the volume `id`, once the scene composed it.
    pub(crate) fn node_of(&self, id: u64) -> Option<NodeId> {
        self.contents.read(|contents| {
            contents
                .iter()
                .find(|content| content.id == id)
                .and_then(|content| content.node.get())
        })
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene calls it")
    )]
    // The camera of the eye that the next draw shows, which a draw reads.
    pub(crate) fn camera(&self) -> MutableState<Camera> {
        self.camera
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene calls it")
    )]
    // The samples that the scene takes in each pixel, which a draw reads.
    pub(crate) fn samples(&self) -> MutableState<Multisample> {
        self.samples
    }
}

// How many things the scene draws for `space`: its volumes, and the controllers that show a model.
// A frame with none makes no projection layer.
#[cfg(any(test, headset))]
pub(crate) fn scene_count(space: &Space) -> usize {
    space.volume_count() + space.volumes().controllers().shown_count()
}
