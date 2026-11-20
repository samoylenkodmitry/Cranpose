// A glTF crate in 3D: a cube with a magenta and lime texture that turns about its vertical axis
// once in six seconds. The file is `assets/spinning_crate.glb`, which `scripts/dev/make_gltf_models.py`
// writes. The model loads on a worker thread, so the first frames show nothing.

use cranpose::{LaunchedEffect, prelude::*};
use cranpose_xr::{
    SpatialGltfModel, SpatialGltfModelSource, SpatialGltfModelStatus, SubspaceModifier,
    rememberSpatialGltfModelState,
};

static CRATE: &[u8] = include_bytes!("../assets/spinning_crate.glb");

// The box of the volume in dp, which are millimeters. The crate is 40 cm wide, so it scales down
// to fit.
const BOX: f32 = 300.0;

// The crate in a volume at `modifier`'s place. A crate that `spinning` turns with the file's
// animation, looped; one that does not stands as the file has it.
#[composable]
pub(crate) fn CrateModel(modifier: SubspaceModifier, spinning: bool) {
    // A source made from bytes is a new source each time, which would load the model again
    let source = remember(|| SpatialGltfModelSource::from_bytes(CRATE)).with(Clone::clone);
    let state = rememberSpatialGltfModelState(source);
    let status = state.status();
    let animated = state.clone();
    LaunchedEffect((status.clone(), spinning), async move |_| {
        if spinning && status == SpatialGltfModelStatus::Loaded {
            for animation in animated.animations() {
                animation.set_looping(true);
                animation.start();
            }
        }
    });
    SpatialGltfModel(state, modifier.size(BOX, BOX).depth(BOX), |_| {});
}
