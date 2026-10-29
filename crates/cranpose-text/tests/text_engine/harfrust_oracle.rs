// HarfRust shaping whole items: what the shaper must produce, however it
// splits an item into words or skips shaping.

use cranpose_text::{
    FontFamily, FontFamilyResolver, FontFeature, GlyphOffset, Itemizer, ShapedGlyphs, Shaper,
    ShapingSpan, TextDirection, TextItem,
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

// A glyph as a comparison reads it.
#[derive(Debug, PartialEq)]
pub(crate) struct Placed {
    pub(crate) id: u32,
    cluster: u32,
    pub(crate) advance: i32,
    offset: GlyphOffset,
    unsafe_to_break: bool,
}

// The glyphs the shaper placed, as a comparison reads them.
fn placed(glyphs: &ShapedGlyphs) -> Vec<Placed> {
    glyphs
        .iter()
        .enumerate()
        .map(|(index, glyph)| Placed {
            id: glyph.id().0,
            cluster: glyph.cluster,
            advance: glyph.advance,
            offset: glyphs.offset(index),
            unsafe_to_break: glyph.unsafe_to_break(),
        })
        .collect()
}

// HarfRust shaping the whole item with `features`, with the same context.
fn whole(font: &Font, text: &str, item: &TextItem<'_>, features: &[Feature]) -> Vec<Placed> {
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
        .map(|(info, position)| Placed {
            id: info.glyph_id,
            cluster: start + info.cluster,
            advance: position.x_advance,
            offset: GlyphOffset {
                x: position.x_offset,
                y: position.y_offset,
            },
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
) -> (Vec<Placed>, Vec<Placed>) {
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
    let (mut split, mut oracle) = (ShapedGlyphs::default(), Vec::new());
    Itemizer::default().itemize(text, TextDirection::Content, &spans, &resolver, |item| {
        shaper.shape(text, &item, &spans[item.span], 0..text.len(), &mut split);
        oracle.extend(whole(&font, text, &item, &features));
    });
    (placed(&split), oracle)
}
