//! Shared gesture constants for consistent touch/pointer handling.
//!
//! These thresholds are intentionally matched between scroll and clickable
//! modifiers to avoid "dead zones" where gestures behave inconsistently.
//!
//! # DPI Considerations
//!
//! These values are in logical (density-independent) pixels. Every platform
//! backend converts pointer positions to logical coordinates before dispatch
//! (e.g. the Android host divides physical positions by the display density),
//! so gesture handlers never need to scale these thresholds by DPI: 8 logical
//! pixels IS 8dp on a high-density phone.

/// Drag threshold (touch slop) in logical pixels (dp).
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
/// - Matches common platform conventions (Android uses 8dp for ViewConfiguration.TOUCH_SLOP)
pub const DRAG_THRESHOLD: f32 = 8.0;

/// Maximum fling velocity in logical pixels per second.
///
/// Matches Android's default maximum fling velocity (ViewConfiguration) at
/// baseline density. Platform-specific configuration should feed this value
/// through the input runtime when host APIs expose it.
pub const MAX_FLING_VELOCITY: f32 = 8_000.0;
