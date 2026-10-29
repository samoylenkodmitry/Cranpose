// HarfRust shaping whole items: what the shaper must produce, however it
// splits an item into words or skips shaping.

use cranpose_text::{
    FontFamily, FontFamilyResolver, FontFeature, Itemizer, ShapedGlyph, Shaper, ShapingSpan,
    TextDirection, TextItem,
};
use harfrust::{
    Buffer, BufferFlags, Direction, Feature, Script, ShapeOptions, ShaperFont, Tag,
    font::{Blob, Font},
};

use crate::fonts::{bytes, span, typeface};

// How the text of a comparison is styled.
#[derive(Clone, Copy)]
pub(crate) struct Style<'a> {
    pub(crate) weight: u16,
    pub(crate) features: &'a [FontFeature],
}

pub(crate) const REGULAR: Style<'static> = Style {
    weight: 400,
    features: &[],
};

// HarfRust shaping the whole item with `features`, with the same context.
fn whole(font: &Font, text: &str, item: &TextItem<'_>, features: &[Feature]) -> Vec<ShapedGlyph> {
    let mut buffer = Buffer::new();
    let mut flags = BufferFlags::empty();
    flags.set(BufferFlags::BEGINNING_OF_TEXT, item.range.start == 0);
    flags.set(BufferFlags::END_OF_TEXT, item.range.end == text.len());
    buffer.set_flags(flags);
    buffer.set_pre_context(
        text.get(..item.range.start)
            .expect("item starts on a boundary"),
    );
    buffer.set_post_context(text.get(item.range.end..).expect("item ends on a boundary"));
    buffer.push_str(text.get(item.range.clone()).expect("item on boundaries"));
    buffer.set_direction(if item.level.is_rtl() {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    buffer.set_script(Script::from_iso15924_tag(Tag::new(&item.script.tag())));
    harfrust::shape(
        &ShaperFont::new(font),
        &mut buffer,
        ShapeOptions::new().features(features),
    )
    .expect("HarfRust shapes the item");
    let start = u32::try_from(item.range.start).expect("short text");
    buffer
        .glyph_infos()
        .iter()
        .zip(buffer.glyph_positions())
        .map(|(info, position)| ShapedGlyph {
            id: cranpose_text::GlyphId(info.glyph_id),
            cluster: start + info.cluster,
            advance: position.x_advance,
            x_offset: position.x_offset,
            y_offset: position.y_offset,
            unsafe_to_break: info.unsafe_to_break(),
        })
        .collect()
}

// The shapes of `text` in the font at `path` in `style`, item by item, from
// `shaper` and from HarfRust on each whole item.
pub(crate) fn compare(
    shaper: &mut Shaper,
    path: &str,
    text: &str,
    style: Style<'_>,
) -> (Vec<ShapedGlyph>, Vec<ShapedGlyph>) {
    let resolver = FontFamilyResolver::new([typeface(path)]);
    let font = Font::new(Blob::from(bytes(path)), 0).expect("the font reads");
    let has_weight = font
        .axes()
        .iter()
        .any(|axis| axis.tag() == Tag::new(b"wght"));
    let font = if has_weight {
        font.instance_builder()
            .variations([(Tag::new(b"wght"), f32::from(style.weight))])
            .build()
    } else {
        font
    };
    let features: Vec<Feature> = style
        .features
        .iter()
        .map(|feature| Feature::new(Tag::new(&feature.tag), feature.value, ..))
        .collect();
    let family = FontFamily::Default;
    let spans = [ShapingSpan {
        features: style.features,
        ..span(0..text.len(), &family, style.weight)
    }];
    let (mut split, mut oracle) = (Vec::new(), Vec::new());
    Itemizer::default().itemize(text, TextDirection::Content, &spans, &resolver, |item| {
        shaper.shape(text, &item, &spans[item.span], 0..text.len(), &mut split);
        oracle.extend(whole(&font, text, &item, &features));
    });
    (split, oracle)
}
