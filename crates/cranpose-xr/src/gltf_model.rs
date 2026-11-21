//! A glTF model as the volume draws it: the nodes that place its meshes, the materials that
//! cover them and the clips that move the nodes.

use std::sync::Arc;

use cranpose::{
    ImageBitmap,
    space::{Mat4, Mesh, Vec3},
};

use crate::{
    Pose,
    controller_parts::ControllerParts,
    gltf_clip::{Clip, Trs},
};

// What a material puts on its meshes: the content of the node that shows them.
#[derive(Clone, Debug)]
pub(crate) enum LayerContent {
    // A color with no texture, in sRGB with straight alpha
    Color([f32; 4]),
    Bitmap(ImageBitmap),
}

// One material's picture. Every primitive of the material shows it, so a material is one layer of
// the scene however many nodes use it.
#[derive(Clone, Debug)]
pub(crate) struct MaterialLayer {
    // The size of the content in pixels
    pub(crate) size: [u32; 2],
    pub(crate) content: LayerContent,
}

// One mesh primitive in the scene: the node that places it and the layer that covers it.
#[derive(Clone, Debug)]
pub(crate) struct Draw {
    pub(crate) node: usize,
    pub(crate) mesh: Arc<Mesh>,
    pub(crate) layer: usize,
}

// A loaded model. It holds no state of the composition, so a worker thread builds it.
#[derive(Debug)]
pub(crate) struct GltfModel {
    // The transform of each node as the file gives it, indexed by the node's index in the file
    pub(crate) rest: Vec<Trs>,
    // Each node's parent, indexed like `rest`
    pub(crate) parents: Vec<Option<usize>>,
    // Each node's name, indexed like `rest`
    pub(crate) names: Vec<Option<Arc<str>>>,
    // The nodes of the scene, each after its parent
    pub(crate) order: Vec<usize>,
    pub(crate) draws: Vec<Draw>,
    pub(crate) layers: Vec<MaterialLayer>,
    pub(crate) clips: Vec<Clip>,
    // The nodes that a controller's inputs move, when the model is a controller's
    pub(crate) parts: ControllerParts,
    // The lowest and highest corner of the box around the meshes at rest, in glTF units
    pub(crate) bounds: ([f32; 3], [f32; 3]),
}

// The map from a node's own space to its parent's.
pub(crate) fn local_matrix(trs: &Trs) -> Mat4 {
    let turn = Pose {
        position: [0.0; 3],
        orientation: trs.rotation,
    };
    let axis = |direction: [f32; 3]| Vec3::from_array(turn.rotate(direction));
    Mat4::from_axes(
        axis([trs.scale[0], 0.0, 0.0]),
        axis([0.0, trs.scale[1], 0.0]),
        axis([0.0, 0.0, trs.scale[2]]),
        Vec3::from_array(trs.translation),
    )
}

impl GltfModel {
    // The size of the box around the meshes at rest, in glTF units.
    pub(crate) fn extent(&self) -> [f32; 3] {
        let (low, high) = self.bounds;
        std::array::from_fn(|axis| high[axis] - low[axis])
    }

    // The middle of the box around the meshes at rest, in glTF units.
    pub(crate) fn center(&self) -> [f32; 3] {
        let (low, high) = self.bounds;
        std::array::from_fn(|axis| (low[axis] + high[axis]) / 2.0)
    }

    // Writes the transform of each node into `locals`: the rest pose, then the clips that move the
    // nodes. `poses` are the clips, each with its time in seconds, in rising order of clip index: a
    // later clip overrides an earlier one on the same property of the same node.
    pub(crate) fn local_transforms(
        &self,
        poses: impl IntoIterator<Item = (usize, f32)>,
        locals: &mut Vec<Trs>,
    ) {
        locals.clone_from(&self.rest);
        for (clip, time) in poses {
            if let Some(clip) = self.clips.get(clip) {
                clip.apply(time, locals);
            }
        }
    }

    // Writes the map from each node's space to the model's into `worlds`, for the nodes at rest
    // and moved by the clips of `poses` (see `local_transforms`). `locals` is room for the
    // transforms, so a frame allocates nothing once both have grown.
    pub(crate) fn world_transforms(
        &self,
        poses: impl IntoIterator<Item = (usize, f32)>,
        locals: &mut Vec<Trs>,
        worlds: &mut Vec<Mat4>,
    ) {
        self.local_transforms(poses, locals);
        self.worlds_of(locals, worlds);
    }

    // Writes the map from each node's space to the model's into `worlds`, from the transform of
    // each node in `locals`.
    pub(crate) fn worlds_of(&self, locals: &[Trs], worlds: &mut Vec<Mat4>) {
        worlds.resize(self.rest.len(), Mat4::IDENTITY);
        for &node in &self.order {
            let local = local_matrix(&locals[node]);
            worlds[node] = match self.parents[node] {
                Some(parent) => local.then(&worlds[parent]),
                None => local,
            };
        }
    }
}
