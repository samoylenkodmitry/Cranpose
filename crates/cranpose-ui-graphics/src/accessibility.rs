use std::cell::Cell;

/// What the person set in the system's accessibility settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AccessibilityOptions {
    /// The text size setting as a multiplier of the default, 1.0 when the
    /// person left it alone. Dynamic Type on iOS, the font size on Android
    /// and the web, the text scale of the desktop.
    pub font_scale: f32,
    /// Whether the person asked for less motion. Every animation then ends on
    /// its first frame.
    pub reduce_motion: bool,
    /// Whether the person asked for less transparency. Glass then draws as a
    /// flat surface with no blur.
    pub reduce_transparency: bool,
    /// Whether the person asked for more contrast. Secondary text, separators
    /// and fills then draw darker on light and lighter on dark.
    pub increase_contrast: bool,
    /// Whether the person asked for bold text. Every text style then gains a
    /// weight step.
    pub bold_text: bool,
    /// Whether the system inverts colors and leaves this app's window to
    /// invert its own, the way iOS Smart Invert does. The theme then swaps to
    /// its other palette and pictures stay as they are. A system that inverts
    /// the whole screen itself reports false.
    pub invert_colors: bool,
}

impl Default for AccessibilityOptions {
    fn default() -> Self {
        Self {
            font_scale: 1.0,
            reduce_motion: false,
            reduce_transparency: false,
            increase_contrast: false,
            bold_text: false,
            invert_colors: false,
        }
    }
}

impl AccessibilityOptions {
    /// The options with the font scale made a plain number: finite and above
    /// zero, else 1.0.
    pub fn normalized(mut self) -> Self {
        if !(self.font_scale.is_finite() && self.font_scale > 0.0) {
            self.font_scale = 1.0;
        }
        self
    }
}

thread_local! {
    static PLATFORM_ACCESSIBILITY_OPTIONS: Cell<AccessibilityOptions> =
        const { Cell::new(AccessibilityOptions {
            font_scale: 1.0,
            reduce_motion: false,
            reduce_transparency: false,
            increase_contrast: false,
            bold_text: false,
            invert_colors: false,
        }) };
}

/// Installs what the platform reports. A backend calls this at start and on
/// every change, and forces a root render when the answer is true, so the
/// theme, the glass and the animations read the new options.
pub fn set_platform_accessibility_options(options: AccessibilityOptions) -> bool {
    let options = options.normalized();
    PLATFORM_ACCESSIBILITY_OPTIONS.with(|cell| {
        let changed = cell.get() != options;
        cell.set(options);
        changed
    })
}

/// What the platform last reported.
pub fn platform_accessibility_options() -> AccessibilityOptions {
    PLATFORM_ACCESSIBILITY_OPTIONS.with(Cell::get)
}
