//! Demo fonts for the Compose application.
//!
//! These fonts are embedded at compile time and used for text rendering
//! across both desktop and Android platforms.

/// Static array of embedded font data.
///
/// Contains Roboto Light and Regular variants used throughout the demo.
pub static DEMO_FONTS: [&[u8]; 2] = [
    include_bytes!("../assets/Roboto-Light.ttf"),
    include_bytes!("../assets/Roboto-Regular.ttf"),
];
