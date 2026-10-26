use super::ShapeDepth;

// Whether a pass draws against a depth buffer its opaque interiors fill
// first, which picks the variant of each pipeline its draws take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PassDepth {
    Off,
    On,
}

impl PassDepth {
    pub(crate) const BOTH: [Self; 2] = [Self::Off, Self::On];

    pub(crate) const fn is_on(self) -> bool {
        matches!(self, Self::On)
    }

    // The index of a pipeline kept once per depth.
    pub(crate) const fn index(self) -> usize {
        self.is_on() as usize
    }

    // The depth state of a glyph or image pipeline: in a pass with a depth
    // buffer it paints what later opaque interiors leave visible, as shapes
    // do.
    pub(crate) fn overlay_state(self) -> Option<wgpu::DepthStencilState> {
        match self {
            Self::Off => None,
            Self::On => ShapeDepth::Tested.stencil_state(),
        }
    }
}
