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

impl SoftwareTextFont {
    pub(crate) fn shaped_for(&self, style: &TextStyle) -> ShapedFont<'_> {
        let settings = style.span_style.font_feature_settings.as_deref();
        if settings.is_none() && !self.font.has_substitutions() {
            return ShapedFont::Borrowed(self);
        }
        self.shaped_with(settings)
    }

    // Out of line, so the face it may build takes room on the stack only
    // while it builds one.
    #[inline(never)]
    fn shaped_with(&self, settings: Option<&str>) -> ShapedFont<'_> {
        match self.font.with_feature_settings(settings) {
            None => ShapedFont::Borrowed(self),
            Some(font) => ShapedFont::Owned(Box::new(Self {
                glyph_map_hash: self.content_hash ^ font.feature_key(),
                font,
                metadata: self.metadata.clone(),
                score: self.score,
                content_hash: self.content_hash,
                #[cfg(feature = "text-shaping")]
                shaping: Arc::clone(&self.shaping),
            })),
        }
    }
}
