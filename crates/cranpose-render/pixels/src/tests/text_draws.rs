use std::{rc::Rc, sync::Arc};

use cranpose_render_common::{text_fonts::TextFonts, text_raster::GlyphInk};
use cranpose_ui::{PreparedTextLayout, TextStyle};
use cranpose_ui_graphics::{Color, Rect};

use crate::scene::TextDraw;

// `text` in `style` laid out with the fonts a renderer without app fonts
// hands its app, up to `width`.
pub(crate) fn laid_out(text: &str, style: &TextStyle, width: f32) -> Rc<PreparedTextLayout> {
    cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(TextFonts::from_fonts_or_default(&[]).resolver());
        cranpose_ui::text::prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from(text),
            style,
            Default::default(),
            Some(width),
        )
    })
}

// A draw of `text` in `style` at `font_size` pixels, laid out in `rect`.
pub(crate) fn text_draw(
    rect: Rect,
    text: &str,
    style: TextStyle,
    font_size: f32,
    color: Color,
) -> TextDraw {
    let mut style = style;
    style.span_style.font_size = cranpose_ui::text::TextUnit::Sp(font_size);
    let prepared = laid_out(text, &style, rect.width);
    TextDraw {
        rect,
        snap_anchor: None,
        text: Rc::clone(&prepared.text),
        layout: prepared.layout.clone(),
        ink: GlyphInk::Painted,
        color,
        text_style: Arc::new(style),
        font_size,
        scale: 1.0,
        clip: None,
    }
}
