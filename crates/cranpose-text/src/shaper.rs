use std::ops::Range;

use harfrust::{
    Buffer, BufferFlags, Direction, Feature, Language, Script as ShapingScript, ShapeOptions,
    ShapePlan, ShapePlanKey, ShaperFont, Tag,
    font::{Font, Variation},
};
use smallvec::SmallVec;

use crate::{
    font::{FontStyle, FontWeight},
    glyph_id::GlyphId,
    itemizer::TextItem,
    shaping_span::ShapingSpan,
    typeface::Typeface,
};

/// One glyph a shaper placed, in font design units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// The glyph.
    pub id: GlyphId,
    /// The byte offset in the text of the cluster the glyph belongs to.
    pub cluster: u32,
    /// How far the glyph moves the pen.
    pub advance: i32,
    /// How far right of the pen the glyph draws.
    pub x_offset: i32,
    /// How far up from the baseline the glyph draws.
    pub y_offset: i32,
    /// Whether a line break before this glyph's cluster changes the shapes
    /// on either side of it.
    pub unsafe_to_break: bool,
    /// Whether shaping the text on either side of this glyph's cluster apart
    /// can change the shapes on the other side.
    pub unsafe_to_concat: bool,
}

/// Shapes items with HarfRust. Keeps, for each typeface instance it met, the
/// shaping font and its shape plans, and one glyph buffer for every call.
pub struct Shaper {
    faces: Vec<ShapingFace>,
    buffer: Buffer,
    features: Vec<Feature>,
}

struct ShapingFace {
    content_hash: u64,
    positions: SmallVec<[([u8; 4], f32); 3]>,
    font: Font,
    // The newest last: text uses few scripts and feature settings, and a
    // plan takes as long to make as the shaping.
    plans: Vec<ShapePlan>,
}

const KEPT_FACES: usize = 16;
const KEPT_PLANS: usize = 8;

impl Default for Shaper {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            buffer: Buffer::new(),
            features: Vec::new(),
        }
    }
}

impl Shaper {
    /// Shapes `item` of `text` in `span` and appends its glyphs to `glyphs`,
    /// in the order they are drawn from left to right. The text of `context`
    /// around the item is the shaping context, so joining and contextual
    /// forms carry across item edges inside it.
    pub fn shape(
        &mut self,
        text: &str,
        item: &TextItem<'_>,
        span: &ShapingSpan<'_>,
        context: Range<usize>,
        glyphs: &mut Vec<ShapedGlyph>,
    ) {
        let Ok(start) = u32::try_from(item.range.start) else {
            return;
        };
        let face = self.face(item.typeface, span.font.weight, span.font.style);
        let face = &mut self.faces[face];
        let buffer = &mut self.buffer;
        buffer.clear();
        let mut flags = BufferFlags::PRODUCE_UNSAFE_TO_CONCAT;
        flags.set(
            BufferFlags::BEGINNING_OF_TEXT,
            item.range.start == context.start,
        );
        flags.set(BufferFlags::END_OF_TEXT, item.range.end == context.end);
        buffer.set_flags(flags);
        buffer.set_pre_context(&text[context.start..item.range.start]);
        buffer.set_post_context(&text[item.range.end..context.end]);
        buffer.push_str(&text[item.range.clone()]);
        let direction = if item.level.is_rtl() {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };
        let script = ShapingScript::from_iso15924_tag(Tag::new(&item.script.tag()));
        let language = span.language.and_then(|tag| tag.parse::<Language>().ok());
        buffer.set_direction(direction);
        buffer.set_script(script);
        buffer.set_language(language.clone());
        self.features.clear();
        self.features.extend(
            span.features
                .iter()
                .map(|feature| Feature::new(Tag::new(&feature.tag), feature.value, ..)),
        );
        let wanted = ShapePlanKey::new(&face.font, script, direction)
            .language(language.as_ref())
            .features(&self.features);
        let plan = match face.plans.iter().position(|plan| wanted.matches(plan)) {
            Some(plan) => plan,
            None => {
                if face.plans.len() == KEPT_PLANS {
                    face.plans.remove(0);
                }
                face.plans.push(ShapePlan::new(
                    &face.font,
                    direction,
                    script,
                    language.as_ref(),
                    &self.features,
                ));
                face.plans.len() - 1
            }
        };
        let shaped = harfrust::shape(
            &ShaperFont::new(&face.font),
            buffer,
            ShapeOptions::new()
                .plan(Some(&face.plans[plan]))
                .features(&self.features),
        );
        if shaped.is_ok() {
            glyphs.extend(
                buffer
                    .glyph_infos()
                    .iter()
                    .zip(buffer.glyph_positions())
                    .map(|(info, position)| ShapedGlyph {
                        id: GlyphId(info.glyph_id),
                        cluster: start + info.cluster,
                        advance: position.x_advance,
                        x_offset: position.x_offset,
                        y_offset: position.y_offset,
                        unsafe_to_break: info.unsafe_to_break(),
                        unsafe_to_concat: info.unsafe_to_concat(),
                    }),
            );
        }
    }

    // The index of the shaping face for `typeface` at `weight` and `style`,
    // made when first asked for.
    fn face(&mut self, typeface: &Typeface, weight: FontWeight, style: FontStyle) -> usize {
        let positions = typeface.axis_positions(weight, style);
        let content_hash = typeface.content_hash();
        if let Some(face) = self
            .faces
            .iter()
            .position(|face| face.content_hash == content_hash && face.positions == positions)
        {
            return face;
        }
        if self.faces.len() == KEPT_FACES {
            self.faces.remove(0);
        }
        let font = if positions.is_empty() {
            typeface.font().clone()
        } else {
            typeface
                .font()
                .instance_builder()
                .variations(
                    positions
                        .iter()
                        .map(|(tag, value)| Variation::new(Tag::new(tag), *value)),
                )
                .build()
        };
        self.faces.push(ShapingFace {
            content_hash,
            positions,
            font,
            plans: Vec::new(),
        });
        self.faces.len() - 1
    }
}
