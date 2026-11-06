// What a collect keeps from frame to frame: the raster scales of animated
// layers, and the contents of isolated layers that returned packets gave
// back.

use super::{KeptContents, LayerMotion};

#[derive(Default)]
pub(crate) struct CollectHistory {
    pub(crate) motion: LayerMotion,
    pub(crate) contents: KeptContents,
}
