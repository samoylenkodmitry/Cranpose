use super::{PassDepth, ViewportUniformParams};

// The space a draw lands in: the viewport uniforms it draws under and the
// scale from its scene's logical space to device pixels.
#[derive(Clone, Copy)]
pub(crate) struct DrawSpace {
    pub(crate) viewport: ViewportUniformParams,
    pub(crate) scale: f32,
}

impl DrawSpace {
    // The space with its records counted from `place` in the pass's order.
    pub(crate) fn at_place(self, place: u32) -> Self {
        Self {
            viewport: ViewportUniformParams {
                depth_base: place as f32,
                ..self.viewport
            },
            ..self
        }
    }
}

// How a segment's runs land in the run arena: its draw space, the shift
// into its binding's space, whether its pass mixes flat and turned
// segments, and whether the pass draws with the depth buffer.
#[derive(Clone, Copy)]
pub(crate) struct ArenaPlacement {
    pub(crate) space: DrawSpace,
    pub(crate) shift: [f32; 2],
    pub(crate) mixed_turns: bool,
    pub(crate) depth: PassDepth,
}
