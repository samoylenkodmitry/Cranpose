//! How many layers the runtime takes in a frame.
//!
//! A runtime reports the most layers that it takes. The specification guarantees 16. A Quest 3
//! reports 32 and refuses a frame of 17 or more. So a frame holds the guaranteed number until the
//! runtime takes a frame of the reported number. The first frame that the runtime shows tries it:
//! empty layers fill the frame up to the reported number. A runtime that refuses that frame gets it
//! again in the same frame without them. The limit then stays at the guaranteed number.

// The number of layers that every runtime takes: `XR_MIN_COMPOSITION_LAYERS_SUPPORTED`.
const GUARANTEED: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerLimit {
    reported: usize,
    taken: usize,
    // Whether a frame still has to try the reported number
    trial: bool,
}

impl LayerLimit {
    // The limit of a runtime that reports `reported` layers. A runtime that reports none gets the
    // guaranteed number.
    pub(crate) fn reported(reported: u32) -> Self {
        let reported = usize::try_from(reported)
            .ok()
            .filter(|&layers| layers > 0)
            .unwrap_or(GUARANTEED);
        Self {
            reported,
            taken: reported.min(GUARANTEED),
            trial: reported > GUARANTEED,
        }
    }

    // The most layers that a frame holds, the room's layer included.
    pub(crate) fn layers(self) -> usize {
        self.taken
    }

    // The number of layers that the next shown frame tries, while no frame has tried it.
    pub(crate) fn trial(self) -> Option<usize> {
        self.trial.then_some(self.reported)
    }

    // Takes note of whether the runtime took the frame that tried the reported number.
    pub(crate) fn tried(&mut self, taken: bool) {
        self.trial = false;
        if taken {
            self.taken = self.reported;
        }
    }

    // Takes note that the runtime refused a frame of `layers`. Returns whether the limit fell, so
    // that the next frame can have fewer layers. A refused frame of the guaranteed number or fewer
    // has nothing to drop, and a limit that sits at the guaranteed number cannot fall.
    pub(crate) fn refused(&mut self, layers: usize) -> bool {
        let falls = layers > GUARANTEED && self.taken > GUARANTEED;
        if falls {
            self.taken = GUARANTEED;
        }
        falls
    }
}

#[cfg(test)]
#[path = "tests/layer_limit_tests.rs"]
mod tests;
