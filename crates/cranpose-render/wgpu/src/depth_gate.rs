//! When passes lay their opaque interiors down first.

use crate::lazy_resource::Pending;

/// Whether passes lay their opaque interiors down first, in a depth
/// pre-pass (see [`crate::draw_pass::lays_interiors_first`]). The pre-pass
/// takes pipelines of its own: where pipelines build on the compiler's
/// threads, a launch's first frames draw without it while its pipelines
/// build behind the frames' own, and it starts once the pipelines a frame
/// drawn without it would have taken have landed. On a Mate 20 X the
/// gauntlet's first frame took 12 pipelines without the pre-pass and 17
/// with it, and its content showed 383-399 ms after the first frame
/// against 499-516.
pub(crate) struct DepthGate {
    state: GateState,
    /// The depth pipelines the frame's passes drawn without their pre-pass
    /// would have taken and that were still to land.
    awaited: Vec<Pending<wgpu::RenderPipeline>>,
    /// Whether a pass of the frame drew without the pre-pass it takes.
    waited: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GateState {
    /// The backend never lays interiors down first.
    Never,
    /// Passes draw without the pre-pass until its pipelines land.
    Closed,
    Open,
}

impl DepthGate {
    /// A gate for a backend that `may` lay interiors down first, with
    /// pipelines built on compiler threads when `compiling_ahead`, and else
    /// where a draw first needs them, the pre-pass's among them.
    pub(crate) fn new(may: bool, compiling_ahead: bool) -> Self {
        let state = match (may, compiling_ahead) {
            (false, _) => GateState::Never,
            (true, true) => GateState::Closed,
            (true, false) => GateState::Open,
        };
        Self {
            state,
            awaited: Vec::new(),
            waited: false,
        }
    }

    /// Whether a pass may lay interiors down first at all.
    pub(crate) fn may(&self) -> bool {
        self.state != GateState::Never
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state == GateState::Open
    }

    /// The pipelines awaited so far this frame, for a pass drawn without its
    /// pre-pass to add those it would have taken; handed back with
    /// [`Self::waited_for`].
    pub(crate) fn take_awaited(&mut self) -> Vec<Pending<wgpu::RenderPipeline>> {
        std::mem::take(&mut self.awaited)
    }

    pub(crate) fn waited_for(&mut self, awaited: Vec<Pending<wgpu::RenderPipeline>>) {
        self.awaited = awaited;
        self.waited = true;
    }

    /// Ends a frame: the pre-pass starts with the next one once every
    /// pipeline the frame's passes drawn without it would have taken has
    /// landed.
    pub(crate) fn end_frame(&mut self) {
        if self.waited && self.awaited.iter().all(Pending::landed) {
            self.state = GateState::Open;
        }
        self.awaited.clear();
        self.waited = false;
    }
}
