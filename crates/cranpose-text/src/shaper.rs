use std::{
    cell::OnceCell,
    hash::{BuildHasher, Hash, Hasher},
    ops::Range,
};

use foldhash::fast::FixedState;
use harfrust::{
    Buffer, BufferFlags, Direction, Feature, Language, Script as ShapingScript, ShapeOptions,
    ShapePlan, ShaperFont, Tag,
    font::{Font, Source, Variation},
};
use read_fonts::FontRef;
use smallvec::SmallVec;

use crate::{
    font::{FontStyle, FontWeight},
    glyph_id::GlyphId,
    itemizer::TextItem,
    plain_glyphs::PlainGlyphs,
    shaped_glyphs::{BreakBefore, GlyphOffset, ShapedGlyph, ShapedGlyphs},
    shaping_span::ShapingSpan,
    typeface::Typeface,
    word_cache::WordCache,
    word_split::{SpaceCut, WordSplit, word_split},
};

/// Shapes items with HarfRust. Keeps, for each typeface instance it met, the
/// shaping font and its shape plans, the words it shaped where the font lets
/// words shape apart, and one glyph buffer for every call.
pub struct Shaper {
    faces: Vec<ShapingFace>,
    buffer: Buffer,
    features: Vec<Feature>,
    pub(crate) words: WordCache,
    // The ends of an item's words, and its pieces shaped apart: their text
    // and their glyphs, reused from item to item.
    word_ends: Vec<usize>,
    pieces: Vec<(Range<usize>, Range<usize>)>,
}

struct ShapingFace {
    content_hash: u64,
    positions: SmallVec<[([u8; 4], f32); 3]>,
    font: Font,
    // The newest last: text uses few scripts and feature settings, and a
    // plan takes as long to make as the shaping.
    plans: Vec<KeptPlan>,
    // The plain glyphs of the plans, one for the plans whose lookups leave
    // the same glyphs plain, as the plans of numbers and of words do.
    plain: Vec<PlainGlyphs>,
    // The plan the last item with no language or features of its own took,
    // and what it asked for: an item that asks the same takes it without
    // matching the plans kept.
    last: Option<(PlanAsk, usize)>,
}

// What chooses the plan of an item with no language or features of its own.
#[derive(Clone, Copy, PartialEq)]
struct PlanAsk {
    rtl: bool,
    script: [u8; 4],
}

struct KeptPlan {
    // What the plan shapes runs for.
    direction: Direction,
    script: Option<ShapingScript>,
    language: Option<Language>,
    features: SmallVec<[Feature; 4]>,
    // Made when HarfRust first shapes with the plan, as a run of plain
    // glyphs needs neither its lookups nor its words.
    harfrust: OnceCell<HarfRustPlan>,
    // The index in its face of the glyphs the plan leaves as they are;
    // `None` when it cannot tell.
    plain: Option<usize>,
    // The face and plan as one number, which keys the plan's words.
    key: u64,
}

// What HarfRust shapes the runs of a plan with.
struct HarfRustPlan {
    plan: ShapePlan,
    // Where runs cut into words; `None` when they shape whole.
    split: Option<WordSplit>,
}

// What one shaping call shapes with.
struct Setting<'a> {
    font: ShaperFont<'a, 'a>,
    // The font `font` shapes with, which plain glyphs read.
    face: &'a Font,
    plan: &'a ShapePlan,
    features: &'a [Feature],
    direction: Direction,
    script: Option<ShapingScript>,
    language: Option<&'a Language>,
}

const KEPT_FACES: usize = 16;
const KEPT_PLANS: usize = 8;
const HASH_STATE: FixedState = FixedState::with_seed(0);

impl Default for Shaper {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            buffer: Buffer::new(),
            features: Vec::new(),
            words: WordCache::default(),
            word_ends: Vec::new(),
            pieces: Vec::new(),
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
        glyphs: &mut ShapedGlyphs,
    ) {
        let face = self.face(item.typeface, span.font.weight, span.font.style);
        let asked = (span.language.is_none() && span.features.is_empty()).then_some(PlanAsk {
            rtl: item.level.is_rtl(),
            script: item.script.tag(),
        });
        // An item that searched the plans has its settings made, and the
        // shaper's features set for it.
        let (plan, settings) = match self.faces[face].last.filter(|last| Some(last.0) == asked) {
            Some((_, plan)) => (plan, None),
            None => {
                let (plan, settings) = self.asked_plan(face, (item, span), asked);
                (plan, Some(settings))
            }
        };
        let ShapingFace {
            font, plans, plain, ..
        } = &mut self.faces[face];
        // A run of plain glyphs needs neither shaping nor words.
        if let Some(index) = plans[plan].plain
            && plain[index].append(font, text, item.range.clone(), glyphs)
        {
            return;
        }
        // Made here, the HarfRust plan does not take its stack under the
        // shaping font's.
        plans[plan].harfrust(font);
        self.shape_with_plan((face, plan), text, (item, span, settings), context, glyphs);
    }

    // The plan of face `face` for `item` in `span`, which did not take the
    // face's last plan as `asked`, and the settings it searched with. Out of
    // line: most items take the plan the item before them took, and need
    // neither the settings nor the search.
    #[inline(never)]
    fn asked_plan(
        &mut self,
        face: usize,
        (item, span): (&TextItem<'_>, &ShapingSpan<'_>),
        asked: Option<PlanAsk>,
    ) -> (usize, ShapingSettings) {
        let settings = shaping_settings(&mut self.features, item, span);
        let (direction, script, language) = &settings;
        let plan = self.plan(face, *direction, *script, language.as_ref());
        self.faces[face].last = asked.map(|asked| (asked, plan));
        (plan, settings)
    }

    // Shapes `item` as `shape` does, with plan `plan` of face `face`. Out of
    // line, so the shaping font stays off the stack of the plain glyphs and
    // of a new plan, which run at the bottom of a layout.
    #[inline(never)]
    fn shape_with_plan(
        &mut self,
        (face, plan): (usize, usize),
        text: &str,
        (item, span, settings): (&TextItem<'_>, &ShapingSpan<'_>, Option<ShapingSettings>),
        context: Range<usize>,
        glyphs: &mut ShapedGlyphs,
    ) {
        let Self {
            faces,
            buffer,
            features,
            words,
            word_ends,
            pieces,
        } = self;
        let (direction, script, language) =
            settings.unwrap_or_else(|| shaping_settings(features, item, span));
        let ShapingFace {
            font, plans, plain, ..
        } = &mut faces[face];
        let font = &*font;
        let kept = &plans[plan];
        let harfrust = kept.harfrust(font);
        let plain = kept.plain.map(|index| &mut plain[index]);
        let setting = Setting {
            font: ShaperFont::new(font),
            face: font,
            plan: &harfrust.plan,
            features,
            direction,
            script,
            language: language.as_ref(),
        };
        let Some(split) = &harfrust.split else {
            shape_range(buffer, &setting, text, item.range.clone(), context, glyphs);
            return;
        };
        WordShaping {
            buffer,
            setting,
            plain,
            words,
            key: kept.key,
            text,
            context,
        }
        .shape_words(item.range.clone(), split, word_ends, pieces, glyphs);
    }

    // The index in its face of the plan for `direction`, `script`,
    // `language` and the features just set, made when first asked for.
    fn plan(
        &mut self,
        face: usize,
        direction: Direction,
        script: Option<ShapingScript>,
        language: Option<&Language>,
    ) -> usize {
        let features = &self.features;
        if let Some(plan) = self.faces[face]
            .plans
            .iter()
            .position(|kept| kept.is_for(direction, script, language, features))
        {
            return plan;
        }
        self.new_plan(face, direction, script, language)
    }

    // Makes the plan `plan` found none of. It runs once a plan, at the bottom
    // of a layout, so it and each of its parts stay out of line: the stack
    // holds one part's frame at a time, which a microcontroller's main task
    // has room for.
    #[cold]
    #[inline(never)]
    fn new_plan(
        &mut self,
        face: usize,
        direction: Direction,
        script: Option<ShapingScript>,
        language: Option<&Language>,
    ) -> usize {
        let features = &self.features;
        let face = &mut self.faces[face];
        if face.plans.len() == KEPT_PLANS {
            face.forget_oldest_plan();
        }
        let key = face.plan_key(direction, script, language, features);
        let plain = face.plan_glyphs(direction, script, features);
        face.push_plan(direction, script, language, features, (plain, key));
        face.plans.len() - 1
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
        self.new_face(typeface, positions, content_hash)
    }

    // Makes the face `face` found none of. Out of line: the instance of a
    // variable font takes a deeper frame than shaping does.
    #[cold]
    #[inline(never)]
    fn new_face(
        &mut self,
        typeface: &Typeface,
        positions: SmallVec<[([u8; 4], f32); 3]>,
        content_hash: u64,
    ) -> usize {
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
            plain: Vec::new(),
            last: None,
        });
        self.faces.len() - 1
    }
}

// The direction, script and language an item shapes in.
type ShapingSettings = (Direction, Option<ShapingScript>, Option<Language>);

// The settings of `item`, with the features of `span` set in `features`.
// The layouter spaces letters by the font's `trak` table itself, at each
// span's size, so shaped words keep to every size. Out of line: only a plan
// search and HarfRust need them, and an item makes them once.
#[inline(never)]
fn shaping_settings(
    features: &mut Vec<Feature>,
    item: &TextItem<'_>,
    span: &ShapingSpan<'_>,
) -> ShapingSettings {
    let direction = if item.level.is_rtl() {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    };
    let script = ShapingScript::from_iso15924_tag(Tag::new(&item.script.tag()));
    let language = span.language.and_then(|tag| tag.parse::<Language>().ok());
    features.clear();
    features.push(Feature::new(Tag::new(b"trak"), 0, ..));
    features.extend(
        span.features
            .iter()
            .map(|feature| Feature::new(Tag::new(&feature.tag), feature.value, ..)),
    );
    (direction, script, language)
}

impl ShapingFace {
    // The index of `plain` among the face's plain glyphs: of the ones equal
    // to it, or of itself, kept.
    fn keep_plain(&mut self, plain: PlainGlyphs) -> usize {
        if let Some(index) = self.plain.iter().position(|kept| kept.reads_as(&plain)) {
            return index;
        }
        self.plain.reserve_exact(1);
        self.plain.push(plain);
        self.plain.len() - 1
    }

    // The number that keys the words of the plan for `direction`, `script`,
    // `language` and `features`.
    #[inline(never)]
    fn plan_key(
        &self,
        direction: Direction,
        script: Option<ShapingScript>,
        language: Option<&Language>,
        features: &[Feature],
    ) -> u64 {
        let mut key = HASH_STATE.build_hasher();
        self.content_hash.hash(&mut key);
        for (tag, value) in &self.positions {
            tag.hash(&mut key);
            value.to_bits().hash(&mut key);
        }
        script
            .map(|script| script.tag().to_be_bytes())
            .hash(&mut key);
        (direction == Direction::RightToLeft).hash(&mut key);
        language.map(Language::as_str).hash(&mut key);
        for feature in features {
            (
                feature.tag.to_be_bytes(),
                feature.value,
                feature.start,
                feature.end,
            )
                .hash(&mut key);
        }
        key.finish()
    }

    // The index of the plain glyphs of a plan for `direction`, `script` and
    // `features`.
    #[inline(never)]
    fn plan_glyphs(
        &mut self,
        direction: Direction,
        script: Option<ShapingScript>,
        features: &[Feature],
    ) -> Option<usize> {
        let plain = layout_tables(&self.font)
            .and_then(|tables| PlainGlyphs::of(&tables, script, direction, features));
        plain.map(|plain| self.keep_plain(plain))
    }

    // Keeps a new plan with its plain glyphs and key.
    #[inline(never)]
    fn push_plan(
        &mut self,
        direction: Direction,
        script: Option<ShapingScript>,
        language: Option<&Language>,
        features: &[Feature],
        (plain, key): (Option<usize>, u64),
    ) {
        // A face keeps a plan or two, each a few hundred bytes: room for
        // more goes unused.
        self.plans.reserve_exact(1);
        self.plans.push(KeptPlan {
            direction,
            script,
            language: language.cloned(),
            features: SmallVec::from_slice(features),
            harfrust: OnceCell::new(),
            plain,
            key,
        });
    }

    // Drops the oldest plan, and its plain glyphs when no other plan has
    // them.
    #[inline(never)]
    fn forget_oldest_plan(&mut self) {
        let Some(index) = self.plans.remove(0).plain else {
            return;
        };
        if self.plans.iter().any(|kept| kept.plain == Some(index)) {
            return;
        }
        self.plain.remove(index);
        for other in self.plans.iter_mut().filter_map(|kept| kept.plain.as_mut()) {
            if *other > index {
                *other -= 1;
            }
        }
    }
}

impl KeptPlan {
    // Whether the plan shapes runs for `direction`, `script`, `language` and
    // `features`. HarfRust matches its plans by these and by the font's
    // variations, which a face fixes.
    fn is_for(
        &self,
        direction: Direction,
        script: Option<ShapingScript>,
        language: Option<&Language>,
        features: &[Feature],
    ) -> bool {
        self.direction == direction
            && self.script == script
            && self.language.as_ref() == language
            && self.features.as_slice() == features
    }

    // The HarfRust plan of `font` for the runs of this plan, made on first
    // use. Out of line, so the plan in the making stays off the frame of
    // the shaping that asks for it.
    #[inline(never)]
    fn harfrust(&self, font: &Font) -> &HarfRustPlan {
        self.harfrust.get_or_init(|| HarfRustPlan::new(font, self))
    }
}

impl HarfRustPlan {
    #[cold]
    #[inline(never)]
    fn new(font: &Font, kept: &KeptPlan) -> Self {
        Self {
            plan: ShapePlan::new(
                font,
                kept.direction,
                kept.script,
                kept.language.as_ref(),
                &kept.features,
            ),
            split: face_word_split(font, kept.script),
        }
    }
}

// One item's words being shaped: the buffer, the setting, the plan's plain
// glyphs, the kept words and their key, and the text with its context.
struct WordShaping<'a> {
    buffer: &'a mut Buffer,
    setting: Setting<'a>,
    plain: Option<&'a mut PlainGlyphs>,
    words: &'a mut WordCache,
    key: u64,
    text: &'a str,
    context: Range<usize>,
}

impl WordShaping<'_> {
    // Shapes `range` word by word, cut where `split` says, and appends the
    // glyphs in drawing order. A word whose glyph meets the space at a cut
    // and is one of the split's exceptions, which a lookup reads with that
    // space, shapes together with its neighbor there.
    fn shape_words(
        mut self,
        range: Range<usize>,
        split: &WordSplit,
        word_ends: &mut Vec<usize>,
        pieces: &mut Vec<(Range<usize>, Range<usize>)>,
        glyphs: &mut ShapedGlyphs,
    ) {
        let after_spaces = split.cut == SpaceCut::After;
        word_ends.clear();
        word_ends.extend(word_cuts(self.text, range.clone(), split.cut));
        word_ends.push(range.end);
        let rtl = self.setting.direction == Direction::RightToLeft;
        let item_start = glyphs.len();
        pieces.clear();
        let mut start = range.start;
        for &end in word_ends.iter() {
            let mut word = start..end;
            start = end;
            self.word(word.clone(), glyphs);
            // The glyph of a word that meets the space between it and its
            // neighbor: the last in text order, or the first.
            let meets = |glyphs: &[ShapedGlyph], last: bool| {
                let glyph = if last == rtl {
                    glyphs.first()
                } else {
                    glyphs.last()
                };
                glyph.is_some_and(|glyph| split.exceptions.contains(glyph.id().0))
            };
            let joins = pieces.last().is_some_and(|(_, previous)| {
                if after_spaces {
                    meets(&glyphs[previous.end..], false)
                } else {
                    meets(&glyphs[previous.clone()], true)
                }
            });
            if joins && let Some((text_range, glyph_range)) = pieces.pop() {
                glyphs.truncate(glyph_range.start);
                word = text_range.start..word.end;
                self.word(word.clone(), glyphs);
            }
            let first = pieces
                .last()
                .map_or(item_start, |(_, glyph_range)| glyph_range.end);
            pieces.push((word, first..glyphs.len()));
        }
        if rtl {
            // Words were shaped in text order; drawn right to left, the last
            // comes first.
            let end = glyphs.len();
            glyphs.reverse(item_start..end);
            for (_, piece) in pieces.iter() {
                glyphs.reverse(item_start + end - piece.end..item_start + end - piece.start);
            }
        }
    }
    // Appends the glyphs of `word`: its plain glyphs, kept ones, or shaped
    // and kept when the word shapes the same wherever it stands.
    fn word(&mut self, word: Range<usize>, glyphs: &mut ShapedGlyphs) {
        let Some(word_text) = self.text.get(word.clone()) else {
            return;
        };
        let face = self.setting.face;
        if self
            .plain
            .as_deref_mut()
            .is_some_and(|plain| plain.append(face, self.text, word.clone(), glyphs))
        {
            return;
        }
        let keeps = stands_alone(self.text, &word, &self.context);
        if keeps && let Some((cached, offsets)) = self.words.get(self.key, word_text) {
            glyphs.extend_from_word(cached, offsets, offset(word.start));
            return;
        }
        let first = glyphs.len();
        shape_range(
            self.buffer,
            &self.setting,
            self.text,
            word.clone(),
            self.context.clone(),
            glyphs,
        );
        if keeps && self.words.admits(self.key, word_text) {
            let shaped: SmallVec<[ShapedGlyph; 16]> = glyphs[first..]
                .iter()
                .map(|glyph| glyph.in_cluster(glyph.cluster - offset(word.start)))
                .collect();
            let offsets = glyphs.offsets().get(first..).unwrap_or_default();
            self.words.put(self.key, word_text, &shaped, offsets);
        }
    }
}

// Shapes `range` of `text` with `setting`, the text of `context` around it as
// context, and appends the glyphs.
fn shape_range(
    buffer: &mut Buffer,
    setting: &Setting<'_>,
    text: &str,
    range: Range<usize>,
    context: Range<usize>,
    glyphs: &mut ShapedGlyphs,
) {
    let (Ok(start), Some(before), Some(content), Some(after)) = (
        u32::try_from(range.start),
        text.get(context.start..range.start),
        text.get(range.clone()),
        text.get(range.end..context.end),
    ) else {
        return;
    };
    buffer.clear();
    let mut flags = BufferFlags::empty();
    flags.set(BufferFlags::BEGINNING_OF_TEXT, range.start == context.start);
    flags.set(BufferFlags::END_OF_TEXT, range.end == context.end);
    buffer.set_flags(flags);
    buffer.set_pre_context(before);
    buffer.set_post_context(after);
    buffer.push_str(content);
    buffer.set_direction(setting.direction);
    buffer.set_script(setting.script);
    buffer.set_language(setting.language.cloned());
    let shaped = harfrust::shape(
        &setting.font,
        buffer,
        ShapeOptions::new()
            .plan(Some(setting.plan))
            .features(setting.features),
    );
    if shaped.is_ok() {
        glyphs.reserve(buffer.len());
        for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
            glyphs.push(
                ShapedGlyph::new(
                    GlyphId(info.glyph_id),
                    start + info.cluster,
                    position.x_advance,
                    if info.unsafe_to_break() {
                        BreakBefore::Unsafe
                    } else {
                        BreakBefore::Safe
                    },
                ),
                GlyphOffset {
                    x: position.x_offset,
                    y: position.y_offset,
                },
            );
        }
    }
}

// The offsets inside `range` of `text` where `cut` cuts it into words:
// before each space, or after each run of spaces.
fn word_cuts(text: &str, range: Range<usize>, cut: SpaceCut) -> impl Iterator<Item = usize> + '_ {
    let bytes = text.as_bytes();
    (range.start + 1..range.end).filter(move |&at| match cut {
        SpaceCut::Before => bytes[at] == b' ',
        SpaceCut::After => bytes[at - 1] == b' ' && bytes[at] != b' ',
    })
}

// Whether `word` shapes the same wherever it stands: each of its edges is the
// context's edge or meets a space, so no joining or contextual form reaches
// across it.
fn stands_alone(text: &str, word: &Range<usize>, context: &Range<usize>) -> bool {
    let bytes = text.as_bytes();
    let space = |at: usize| bytes.get(at) == Some(&b' ');
    (word.start == context.start || space(word.start) || space(word.start - 1))
        && (word.end == context.end || space(word.end) || space(word.end - 1))
}

fn offset(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

// The layout tables of `font`.
fn layout_tables(font: &Font) -> Option<FontRef<'_>> {
    FontRef::from_index(font_bytes(font), 0).ok()
}

// The bytes of `font`, empty when it is not read from bytes.
pub(crate) fn font_bytes(font: &Font) -> &[u8] {
    match font.source() {
        Source::Blob(bytes) => bytes.as_ref(),
        _ => &[],
    }
}

// The word split of `font` for runs in `script`; `None` when they shape
// whole.
#[inline(never)]
fn face_word_split(font: &Font, script: Option<ShapingScript>) -> Option<WordSplit> {
    let tables = layout_tables(font)?;
    let space = font.charmap().map_unicode(' ')?;
    let tags = script.map(ShapingScript::tags);
    word_split(
        &tables,
        space,
        tags.as_ref().map_or(&[], |tags| tags.as_slice()),
    )
}
