//! A screen's frame index, as the Rust apps that render on demand keep it:
//! one step a frame from the first, up to the frame it freezes on.

/// The frame a screen shows, and whether it holds still on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameIndex {
    frame: u32,
    freeze: u32,
    frozen: bool,
    first_logged: bool,
}

impl FrameIndex {
    /// Before the first frame; `freeze` 0 runs on.
    pub fn new(freeze: u32) -> Self {
        Self {
            frame: 0,
            freeze,
            frozen: false,
            first_logged: false,
        }
    }

    /// Moves on to the next frame, unless the screen holds still: logs the
    /// first frame and the frame it freezes on. Returns whether the screen
    /// asks for another frame after this one.
    pub fn advance(&mut self) -> bool {
        if !self.first_logged {
            self.first_logged = true;
            log::info!("PERF first_frame");
        }
        if self.frozen {
            return false;
        }
        self.frame += 1;
        if self.freeze > 0 && self.frame >= self.freeze {
            self.frozen = true;
            log::info!("PERF frozen frame={}", self.frame);
            return false;
        }
        true
    }

    /// The frame shown.
    pub fn get(self) -> u32 {
        self.frame
    }

    /// Whether the screen holds still.
    pub fn frozen(self) -> bool {
        self.frozen
    }
}
