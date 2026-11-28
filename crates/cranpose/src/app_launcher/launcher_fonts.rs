use cranpose_render_common::text_fonts::default_typeface;
use cranpose_text::Typeface;

/// Where an [`AppLauncher`](crate::AppLauncher)'s text faces come from.
///
/// A launcher starts as [`DefaultFont`] and becomes [`AppFonts`] as soon as a
/// font method runs. The state is a type rather than a flag because only a
/// type keeps the embedded face out of the binary: a flag checked at run time
/// still references the bytes, and the linker keeps what is referenced.
pub trait LauncherFonts: sealed::Sealed {
    /// The face text draws in when the app supplied none.
    fn default_face() -> Option<Typeface>;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::DefaultFont {}
    impl Sealed for super::AppFonts {}
}

/// The app has supplied no fonts, so text draws in the framework's embedded
/// face (the `embedded-default-font` feature, on by default).
pub struct DefaultFont;

impl LauncherFonts for DefaultFont {
    fn default_face() -> Option<Typeface> {
        default_typeface()
    }
}

/// The app supplied its own fonts. Text draws only in them, and the embedded
/// face is left out of the binary.
pub struct AppFonts;

impl LauncherFonts for AppFonts {
    fn default_face() -> Option<Typeface> {
        None
    }
}
