/// Android floating overlay window configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AndroidOverlayWindowOptions {
    /// Requested overlay width in logical pixels.
    pub width: u32,
    /// Requested overlay height in logical pixels.
    pub height: u32,
    /// Requested screen X position in logical pixels.
    pub x: i32,
    /// Requested screen Y position in logical pixels.
    pub y: i32,
    /// Whether the overlay can receive keyboard focus.
    pub focusable: bool,
}

impl AndroidOverlayWindowOptions {
    /// Creates an overlay window request with top-left origin and touch-only focus behavior.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            x: 0,
            y: 0,
            focusable: false,
        }
    }

    /// Sets the initial overlay position in logical pixels.
    pub fn with_position(mut self, x: i32, y: i32) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    /// Sets whether the overlay should receive keyboard focus.
    pub fn with_focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Returns whether this request can create a non-empty overlay surface.
    pub fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }
}
