use cranpose_render_common::text_fonts::TextFonts;

/// The fonts a [`WgpuRenderer`](crate::WgpuRenderer) hands the app its text
/// lays out and draws with.
#[derive(Clone)]
pub struct WgpuTextSystem {
    fonts: TextFonts,
}

impl WgpuTextSystem {
    /// Parses `fonts`, font file bytes ordered by priority with the first highest, into a text
    /// system. Files that do not parse are skipped. When none parses, the text system holds the
    /// embedded default font if the build has it, and no font otherwise.
    pub fn from_fonts(fonts: &[&'static [u8]]) -> Self {
        Self::from_text_fonts(TextFonts::from_fonts_or_default(fonts))
    }

    /// Adopt fonts an app already built, the path app-supplied families
    /// take, where faces were parsed once at startup rather than from
    /// static byte slices here.
    pub fn from_text_fonts(fonts: TextFonts) -> Self {
        Self { fonts }
    }

    pub(crate) fn fonts(&self) -> &TextFonts {
        &self.fonts
    }
}
