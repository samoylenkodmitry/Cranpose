//! The window root that holds the scene of every volume, which the headset draws once for each eye.

use std::any::Any;

use cranpose::{Size, WindowRootDescriptor};

// The size to lay the scene out into: the size of one eye's picture in logical pixels, at one
// pixel to the logical pixel.
pub(crate) struct VolumeWindow {
    size: Size,
}

impl VolumeWindow {
    // A window of `size` pixels.
    pub(crate) fn new(size: [f32; 2]) -> Self {
        Self {
            size: Size::new(size[0], size[1]),
        }
    }
}

impl WindowRootDescriptor for VolumeWindow {
    fn layout_size(&self) -> Size {
        self.size
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
