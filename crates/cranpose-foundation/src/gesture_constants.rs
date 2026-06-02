//! Shared gesture constants for consistent touch/pointer handling.
//!
//! These thresholds are intentionally matched between scroll and clickable
//! modifiers to avoid "dead zones" where gestures behave inconsistently.
//!
//! # DPI Considerations
//!
//! These values are in logical pixels. For very high-density touch screens,
//! consider scaling by the device's DPI factor. Current implementation uses
//! fixed values that work well for typical desktop/mobile displays.

/// Drag threshold in logical pixels.
///
/// If pointer moves more than this distance from the initial press position:
/// - Scroll gestures begin (visual scrolling starts)
/// - Click gestures are cancelled (tap won't fire on release)
///
/// Using a single consistent threshold eliminates the "dead zone" issue where
/// you could be visually scrolling but still trigger a click on release.
///
/// Value of 8.0 was chosen as a reasonable touch slop that:
/// - Is large enough to ignore minor finger jitter on touch screens
/// - Is small enough to feel responsive for intentional drags
/// - Matches common platform conventions (Android uses ~8dp for ViewConfiguration.TOUCH_SLOP)
pub const DRAG_THRESHOLD: f32 = 8.0;

/// Maximum fling velocity in logical pixels per second.
///
/// Matches Android's default maximum fling velocity (ViewConfiguration) at
/// baseline density. Platform-specific configuration should feed this value
/// through the input runtime when host APIs expose it.
pub const MAX_FLING_VELOCITY: f32 = 8_000.0;
