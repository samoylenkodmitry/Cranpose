//! How many layers the runtime takes in a frame.
//!
//! A runtime reports the most layers that it takes. A Quest takes fewer: it refuses a frame of 18
//! layers while it reports 32. The specification guarantees a smaller number, which the limit
//! falls to once the runtime refuses a frame.

// The number of layers that every runtime takes: `XR_MIN_COMPOSITION_LAYERS_SUPPORTED`.
const GUARANTEED: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerLimit(usize);

impl LayerLimit {
    // The limit of a runtime that reports `reported` layers. A runtime that reports none gets the
    // guaranteed number.
    pub(crate) fn reported(reported: u32) -> Self {
        Self(
            usize::try_from(reported)
                .ok()
                .filter(|&layers| layers > 0)
                .unwrap_or(GUARANTEED),
        )
    }

    // The most layers that a frame holds, the room's layer included.
    pub(crate) fn layers(self) -> usize {
        self.0
    }

    // Takes note that the runtime refused a frame of `layers`. Returns whether the limit fell, so
    // that the next frame can have fewer layers. A refused frame of the guaranteed number or fewer
    // has nothing to drop, and a limit that sits at the guaranteed number cannot fall.
    pub(crate) fn refused(&mut self, layers: usize) -> bool {
        let falls = layers > GUARANTEED && self.0 > GUARANTEED;
        if falls {
            self.0 = GUARANTEED;
        }
        falls
    }
}

#[cfg(test)]
#[path = "tests/layer_limit_tests.rs"]
mod tests;
