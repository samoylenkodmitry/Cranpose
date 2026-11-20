//! The state of a glTF model that a [`SpatialGltfModel`](crate::SpatialGltfModel) shows.

use std::{cell::RefCell, sync::Arc};

use cranpose::{LaunchedEffect, composable, space::Mat4, withBlocking};
use cranpose_core::{MutableState, mutableStateOf, rememberKeyed};

use crate::{
    gltf_animation::GltfAnimation, gltf_clip::Trs, gltf_error::GltfError, gltf_import::import,
    gltf_model::GltfModel, spatial_gltf_model_source::SpatialGltfModelSource,
    spatial_gltf_model_status::SpatialGltfModelStatus,
};

// The transforms of the nodes as the animations pose them, and the room that working them out
// needs.
#[derive(Default)]
pub(crate) struct NodePoses {
    pub(crate) locals: Vec<Trs>,
    pub(crate) worlds: Vec<Mat4>,
}

// The draws of one layer: the first places the layer, and the others show the same picture too.
pub(crate) struct LayerDraws {
    pub(crate) owner: usize,
    pub(crate) more: Vec<usize>,
}

// A loaded model with the parts that only the composition can hold.
pub(crate) struct LoadedModel {
    pub(crate) model: GltfModel,
    pub(crate) animations: Vec<GltfAnimation>,
    // For each layer of the model, the draws that it covers
    pub(crate) layers: Vec<LayerDraws>,
    // Counts the starts and stops of the animations, which change the pose
    pub(crate) requests: MutableState<u32>,
    poses: RefCell<NodePoses>,
    // Counts the times the poses changed. A draw that reads it draws again when they do.
    revision: MutableState<u32>,
}

// A loaded model is the same model only when it is the one value.
impl PartialEq for LoadedModel {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl LoadedModel {
    // Takes `model` into the composition, with its animations stopped and its nodes at rest.
    fn new(model: GltfModel) -> Self {
        let requests = mutableStateOf(0);
        let animations = model
            .clips
            .iter()
            .enumerate()
            .map(|(index, clip)| {
                GltfAnimation::new(index, clip.name.as_deref(), clip.duration, requests)
            })
            .collect();
        let mut layers: Vec<Option<LayerDraws>> = model.layers.iter().map(|_| None).collect();
        for (draw, placed) in model.draws.iter().enumerate() {
            match &mut layers[placed.layer] {
                Some(layer) => layer.more.push(draw),
                none => {
                    *none = Some(LayerDraws {
                        owner: draw,
                        more: Vec::new(),
                    });
                }
            }
        }
        let loaded = Self {
            // Every layer comes from a draw, so each has an owner
            layers: layers.into_iter().flatten().collect(),
            model,
            animations,
            requests,
            poses: RefCell::new(NodePoses::default()),
            revision: mutableStateOf(0),
        };
        loaded.pose();
        loaded
    }

    // Places the nodes where the playing animations put them, and tells the draws.
    pub(crate) fn pose(&self) {
        let poses = self
            .animations
            .iter()
            .enumerate()
            .filter_map(|(index, animation)| animation.pose_time().map(|time| (index, time)));
        {
            let mut nodes = self.poses.borrow_mut();
            let NodePoses { locals, worlds } = &mut *nodes;
            self.model.world_transforms(poses, locals, worlds);
        }
        self.revision
            .update(|revision| *revision = revision.wrapping_add(1));
    }

    // Runs `read` on the transform of each node, from the node's own space to the model's. A
    // composable that calls it draws again when an animation moves the nodes.
    pub(crate) fn read_worlds<R>(&self, read: impl FnOnce(&[Mat4]) -> R) -> R {
        let _revision = self.revision.get();
        read(&self.poses.borrow().worlds)
    }
}

/// The state of a glTF model: whether it has loaded, and its animations. Jetpack XR's
/// `SpatialGltfModelState`, which also lists the model's `nodes`. Cranpose has no counterpart of
/// `nodes`.
///
/// A copy of the state is a handle to the same state. Jetpack XR's state is `AutoCloseable`,
/// because it holds a native model. This one holds none, and it frees the model when the
/// [`rememberSpatialGltfModelState`] that made it leaves the composition.
#[derive(Clone, PartialEq)]
pub struct SpatialGltfModelState {
    load: MutableState<Load>,
}

// How far the load has come.
#[derive(Clone)]
pub(crate) enum Load {
    Loading,
    Loaded(Arc<LoadedModel>),
    Failed(GltfError),
}

impl PartialEq for Load {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Loading, Self::Loading) => true,
            (Self::Loaded(a), Self::Loaded(b)) => Arc::ptr_eq(a, b),
            (Self::Failed(a), Self::Failed(b)) => a == b,
            _ => false,
        }
    }
}

impl SpatialGltfModelState {
    /// Whether the model is loading, loaded or failed. A composable that reads it composes again
    /// when it changes.
    pub fn status(&self) -> SpatialGltfModelStatus {
        match self.load.get() {
            Load::Loading => SpatialGltfModelStatus::Loading,
            Load::Loaded(_) => SpatialGltfModelStatus::Loaded,
            Load::Failed(error) => SpatialGltfModelStatus::Failed(error),
        }
    }

    /// The animations of the model, in the order of the file's `animations` array. The list is
    /// empty until the model has loaded. Jetpack XR's `GltfModelEntity.animations`.
    pub fn animations(&self) -> Vec<GltfAnimation> {
        match self.load.get() {
            Load::Loaded(loaded) => loaded.animations.clone(),
            Load::Loading | Load::Failed(_) => Vec::new(),
        }
    }

    /// Stops every animation that plays or is paused, and puts the model back at rest. Jetpack
    /// XR's `GltfModelEntity.stopAllAnimations`.
    pub fn stop_all_animations(&self) {
        for animation in self.animations() {
            animation.stop();
        }
    }

    // The model, once it has loaded.
    pub(crate) fn loaded(&self) -> Option<Arc<LoadedModel>> {
        match self.load.get() {
            Load::Loaded(loaded) => Some(loaded),
            Load::Loading | Load::Failed(_) => None,
        }
    }
}

/// Remembers a [`SpatialGltfModelState`] that loads the model of `source` on a worker thread, so
/// that reading and decoding it never holds a frame. The state starts as
/// [`Loading`](SpatialGltfModelStatus::Loading). It loads again when `source` changes. Jetpack
/// XR's `rememberSpatialGltfModelState`.
#[composable]
pub fn rememberSpatialGltfModelState(source: SpatialGltfModelSource) -> SpatialGltfModelState {
    let load = rememberKeyed(source.clone(), |_| mutableStateOf(Load::Loading));
    LaunchedEffect(source.clone(), async move |_| {
        let source = source.clone();
        let loaded = withBlocking(move || import(&source)).await;
        load.set(match loaded {
            Ok(Ok(model)) => Load::Loaded(Arc::new(LoadedModel::new(model))),
            Ok(Err(error)) => failed(error),
            Err(error) => failed(GltfError::Aborted(error.to_string())),
        });
    });
    SpatialGltfModelState { load }
}

fn failed(error: GltfError) -> Load {
    log::warn!("glTF model: {error}");
    Load::Failed(error)
}
