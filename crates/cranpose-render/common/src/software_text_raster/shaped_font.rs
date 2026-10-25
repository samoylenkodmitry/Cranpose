use std::ops::Deref;
#[cfg(feature = "text-shaping")]
use std::sync::Arc;

use cranpose_ui::text::TextStyle;

use super::SoftwareTextFont;

// A face as a style's font features shape it. The copy a style with other
// features takes is boxed: a face takes kilobytes, and each function on the
// text measure and raster paths that held one kept room for it on the stack.
pub(crate) enum ShapedFont<'a> {
    Borrowed(&'a SoftwareTextFont),
    Owned(Box<SoftwareTextFont>),
}

impl Deref for ShapedFont<'_> {
    type Target = SoftwareTextFont;

    fn deref(&self) -> &SoftwareTextFont {
        match self {
            Self::Borrowed(font) => font,
            Self::Owned(font) => font,
        }
    }
}

impl<'a> ShapedFont<'a> {
    pub(crate) fn new(font: &'a SoftwareTextFont, style: &TextStyle) -> Self {
        let settings = style.span_style.font_feature_settings.as_deref();
        if settings.is_none() && !font.font.has_substitutions() {
            return Self::Borrowed(font);
        }
        Self::with_settings(font, settings)
    }

    // Out of line, so the face it may build takes room on the stack only
    // while it builds one.
    #[inline(never)]
    fn with_settings(font: &'a SoftwareTextFont, settings: Option<&str>) -> Self {
        match font.font.with_feature_settings(settings) {
            None => Self::Borrowed(font),
            Some(shaped) => Self::Owned(Box::new(SoftwareTextFont {
                glyph_map_hash: font.content_hash ^ shaped.feature_key(),
                font: shaped,
                metadata: font.metadata.clone(),
                score: font.score,
                content_hash: font.content_hash,
                #[cfg(feature = "text-shaping")]
                shaping: Arc::clone(&font.shaping),
            })),
        }
    }
}
