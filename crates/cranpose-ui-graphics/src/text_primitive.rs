//! The text a draw scope records: its primitive, the shared copy of its
//! characters and where its block goes inside a rect.

use std::rc::Rc;

use crate::{
    Brush, DrawPrimitive, Point, Rect,
    typography::{
        DrawTextMeasurer, DrawTextStyle, MeasuredText, TextAlign, TextMeasurement,
        TextVerticalAlign, estimate_text_measurement,
    },
};

/// A run of text, positioned and ready to rasterize.
///
/// `rect` is *already resolved*:
/// [`DrawScope::draw_text_at`](crate::DrawScope::draw_text_at) measures the
/// string, applies [`DrawTextStyle::align`] / [`DrawTextStyle::vertical_align`] inside
/// the requested box, and stores the result here. Renderers therefore lay the
/// glyphs out from `rect`'s top-left and never re-align — which is what keeps
/// what [`DrawScope::measure_text`](crate::DrawScope::measure_text) reported
/// and what lands on screen the same geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitive {
    /// Tight block box: origin is the top-left of the first line's slot, size
    /// is the measured size.
    pub rect: Rect,
    /// Shared so redrawing an unchanged string each frame clones a pointer
    /// rather than the characters.
    pub text: Rc<str>,
    /// Font, size, alignment and other styling the glyphs are laid out and drawn with.
    pub style: DrawTextStyle,
    /// Paint for the glyphs: a solid colour, or a gradient laid over the
    /// text block, as Compose's `drawText(brush)` lays it over the layout.
    pub brush: Brush,
    /// The layout the text was measured in, which the renderer draws.
    /// `None` when no text system measured it.
    pub layout: Option<crate::TextLayoutHandle>,
}

/// Returns a shared `Rc<str>` for `text`, reusing the copy made on an earlier
/// frame when the content matches.
///
/// Apps hand `draw_text*` a `&str` every frame, and a score counter or label
/// is the same characters frame after frame — without this pool every call
/// copied them into a fresh `Rc<str>` anyway, defeating the sharing
/// [`TextPrimitive::text`] exists for. Hits are verified by content, so a hash
/// collision costs one fresh copy, never the wrong text. The pool keeps the
/// copies that recordings still hold, as [`crate::ContentPool`] does.
pub(crate) fn shared_text_str(text: &str) -> Rc<str> {
    use std::hash::{Hash, Hasher};

    let mut hasher = crate::FxHasher::default();
    text.hash(&mut hasher);
    crate::thread_state::with(|state| {
        state.shared_texts.borrow_mut().with(
            hasher.finish(),
            |shared| &**shared == text,
            || Rc::from(text),
            Rc::clone,
        )
    })
}

/// Resolves the top-left corner a text block of `measurement` gets when it is
/// aligned inside `rect`.
///
/// Split out so the placement rule is stated once and can be unit-tested
/// against the measurement it is derived from.
pub fn align_text_block(rect: Rect, measurement: TextMeasurement, style: &DrawTextStyle) -> Point {
    let x = match style.align {
        TextAlign::Left => rect.x,
        TextAlign::Center => rect.x + (rect.width - measurement.size.width) * 0.5,
        TextAlign::Right => rect.x + rect.width - measurement.size.width,
    };
    let y = match style.vertical_align {
        TextVerticalAlign::Top => rect.y,
        TextVerticalAlign::Center => rect.y + (rect.height - measurement.size.height) * 0.5,
        TextVerticalAlign::Bottom => rect.y + rect.height - measurement.size.height,
        TextVerticalAlign::Baseline => rect.y - measurement.first_baseline,
    };
    Point::new(x, y)
}

// Where a drawn text's block goes.
#[derive(Clone, Copy)]
pub(crate) enum TextBlockAt {
    // Inside a rect, placed by the style's alignments.
    Aligned(Rect),
    // With its top left corner at a point, the style's alignments set to
    // the top left.
    From(Point),
}

// `text` in `style` measured by `measurer`, or estimated without one.
pub(crate) fn measured_text(
    measurer: Option<&dyn DrawTextMeasurer>,
    text: &str,
    style: &DrawTextStyle,
) -> MeasuredText {
    match measurer {
        Some(measurer) => measurer.measure_text(text, style),
        None => MeasuredText {
            measurement: estimate_text_measurement(text, style),
            layout: None,
        },
    }
}

// The primitive that draws `text` in `style` with `brush`, measured once by
// `measurer`, its block placed `at`. `None` when nothing would draw: the
// text is empty, the brush paints nothing, the block has no area or its
// corner is not finite.
pub(crate) fn text_primitive(
    measurer: Option<&dyn DrawTextMeasurer>,
    at: TextBlockAt,
    brush: Brush,
    text: &str,
    style: &DrawTextStyle,
) -> Option<DrawPrimitive> {
    if text.is_empty() || !brush.paints() {
        return None;
    }
    let measured = measured_text(measurer, text, style);
    let size = measured.measurement.size;
    if !(size.width > 0.0 && size.height > 0.0) {
        return None;
    }
    let (origin, style) = match at {
        TextBlockAt::Aligned(rect) => (
            align_text_block(rect, measured.measurement, style),
            style.clone(),
        ),
        TextBlockAt::From(top_left) => (
            top_left,
            DrawTextStyle {
                align: TextAlign::Left,
                vertical_align: TextVerticalAlign::Top,
                ..style.clone()
            },
        ),
    };
    if !origin.x.is_finite() || !origin.y.is_finite() {
        return None;
    }
    Some(DrawPrimitive::Text(Box::new(TextPrimitive {
        rect: Rect::from_origin_size(origin, size),
        text: shared_text_str(text),
        style,
        brush,
        layout: measured.layout,
    })))
}
