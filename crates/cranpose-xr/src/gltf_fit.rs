//! How a model fits the box of its volume.

use cranpose::space::{Mat4, Vec3};

use crate::{METERS_PER_DP, gltf_model::GltfModel};

// The size of a volume's box and the map from the model's units to the box's content space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ModelFit {
    // The width, height and depth of the box, in dp
    pub(crate) size: [f32; 3],
    // The map from the model's glTF units to the content space of the volume
    pub(crate) root: Mat4,
}

impl ModelFit {
    // The fit of `model` to a box whose sides are `limits` in dp, where a side that no modifier set
    // is `None`.
    //
    // The model keeps its proportions. Each side has a ratio of its limit to the model's own side,
    // and the model scales by the smallest ratio, up or down, so it touches the box where that ratio
    // is smallest and lies inside it elsewhere. A side with no limit has the ratio one, so a limit
    // on the other sides never makes the model larger than its own size, which has a glTF unit for
    // a meter. A side with no limit is the model's scaled side. A side shorter than a dp counts as
    // a dp, so a flat model does not scale by the ratio of a thin side. The model's middle goes to
    // the box's middle. Jetpack XR's measure policy does the same, with the sides of its layout
    // constraints for limits.
    pub(crate) fn new(model: &GltfModel, limits: [Option<f32>; 3]) -> Self {
        let natural = model.extent().map(|side| side / METERS_PER_DP);
        let scale = limits
            .iter()
            .zip(natural)
            .map(|(limit, side)| limit.map_or(1.0, |limit| limit.max(0.0) / side.max(1.0)))
            .fold(f32::INFINITY, f32::min);
        let size = std::array::from_fn(|axis| limits[axis].unwrap_or(natural[axis] * scale));
        let center = Vec3::from_array(model.center());
        let per_unit = scale / METERS_PER_DP;
        // glTF has y up and z toward the viewer. The content space has y down and z away, so the
        // model turns half a turn about x, which keeps its handedness.
        let root = Mat4::translation(-center)
            .then(&Mat4::scaling(Vec3::new(per_unit, per_unit, per_unit)))
            .then(&Mat4::scaling(Vec3::new(1.0, -1.0, -1.0)));
        Self { size, root }
    }
}

#[cfg(test)]
#[path = "tests/gltf_fit_tests.rs"]
mod tests;
