// How many pixels of kept shader outputs a frame draws into the layer
// cache. A frame always fills its first output, so each output gets its
// turn on a slow GPU. Frames fill more while the frames that filled keep
// their pace, and less once one runs long.

use web_time::{Duration, Instant};

// A frame that ends later than this after the frame before ran long.
const PACE: Duration = Duration::from_millis(24);
// The most pixels a frame fills past its first output.
const MOST_PIXELS: u64 = 1 << 23;

#[derive(Default)]
pub(crate) struct FillBudget {
    // the pixels a frame fills past its first output
    pixels: u64,
    // the pixels this frame filled
    spent: u64,
    // when the last frame ended, and the pixels it filled
    last: Option<(Instant, u64)>,
}

impl FillBudget {
    // Whether this frame fills an output of `pixels` pixels.
    pub(crate) fn allows(&self, pixels: u64) -> bool {
        self.spent == 0 || self.spent + pixels <= self.pixels
    }

    pub(crate) fn spend(&mut self, pixels: u64) {
        self.spent += pixels;
    }

    // Ends a frame. After a frame that filled, a frame that kept its pace
    // lets the next fill twice that much, and a long one half of it.
    pub(crate) fn end_frame(&mut self) {
        let now = Instant::now();
        if let Some((ended, filled)) = self.last
            && filled > 0
        {
            self.pixels = if now.duration_since(ended) <= PACE {
                (self.pixels.max(filled) * 2).min(MOST_PIXELS)
            } else {
                filled / 2
            };
        }
        self.last = Some((now, self.spent));
        self.spent = 0;
    }
}
