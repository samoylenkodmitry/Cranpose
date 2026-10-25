use std::rc::Rc;

use cranpose_render_common::software_text_raster::{
    SoftwareTextFontSet, SoftwareTextMeasurer, software_text_font_set_from_fonts_or_default,
};
use cranpose_ui::TextMeasurer;

use crate::pipeline::TextLayoutResolver;

pub(crate) struct TextSystemState {
    measurer: SoftwareTextMeasurer,
}

impl TextSystemState {
    pub(crate) fn from_font_set(fonts: SoftwareTextFontSet) -> Self {
        Self {
            measurer: SoftwareTextMeasurer::from_font_set(fonts, 8192),
        }
    }
}

impl TextLayoutResolver for TextSystemState {
    fn layout_text(
        &mut self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::text::TextStyle,
    ) -> cranpose_ui::text_layout_result::TextLayoutResult {
        if cranpose_ui::has_current_app_context() {
            cranpose_ui::text::layout_text(text, style)
        } else {
            self.measurer.layout(text, style)
        }
    }
}

/// The fonts a [`WgpuRenderer`](crate::WgpuRenderer) measures and draws text with.
#[derive(Clone)]
pub struct WgpuTextSystem {
    software_fonts: SoftwareTextFontSet,
}

impl WgpuTextSystem {
    /// Parses `fonts`, font file bytes ordered by priority with the first highest, into a text
    /// system. Files that do not parse are skipped. When none parses, the text system holds the
    /// embedded default font if the build has it, and no font otherwise.
    pub fn from_fonts(fonts: &[&'static [u8]]) -> Self {
        Self {
            software_fonts: software_text_font_set_from_fonts_or_default(fonts),
        }
    }

    /// Adopt a font set an app already built — the path app-supplied families
    /// take, where faces were parsed once at startup rather than from static
    /// byte slices here.
    pub fn from_font_set(software_fonts: SoftwareTextFontSet) -> Self {
        Self { software_fonts }
    }

    pub(crate) fn render_state(&self) -> TextSystemState {
        TextSystemState::from_font_set(self.software_fonts.clone())
    }

    pub(crate) fn software_fonts(&self) -> SoftwareTextFontSet {
        self.software_fonts.clone()
    }
}

/// Create an accurate WGPU text measurer for headless tests without launching a window.
pub fn headless_text_measurer() -> Rc<dyn TextMeasurer> {
    headless_text_measurer_with_fonts(&[])
}

/// Create an accurate WGPU text measurer for headless tests with explicit fonts.
pub fn headless_text_measurer_with_fonts(fonts: &[&'static [u8]]) -> Rc<dyn TextMeasurer> {
    Rc::new(SoftwareTextMeasurer::from_fonts_or_default(fonts, 8192))
}
