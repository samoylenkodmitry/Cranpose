use std::{
    hash::{Hash, Hasher},
    ops::Range,
    sync::Arc,
};

use ab_glyph::{Font as _, FontArc, GlyphId};
use cranpose_core::{collections::bounded_lru::BoundedLruCache, hash::default};
use cranpose_text::font_feature::font_feature_settings;
use cranpose_ui::text::{ResolvedTextDirection, TextShaping, TextStyle};
use harfrust::{
    Buffer, Direction, Feature, GlyphInfo, GlyphPosition, Language, Script, ShapeOptions,
    ShapePlan, ShapePlanKey, ShaperFont, Tag,
    font::{Blob, Font, Variation},
};
use smallvec::SmallVec;

pub(crate) fn required(text: &str, style: &TextStyle) -> bool {
    !text.is_ascii()
        || matches!(
            style.paragraph_style.platform_style.and_then(|p| p.shaping),
            Some(TextShaping::Advanced)
        )
}

pub(crate) struct ShapedGlyph {
    pub id: GlyphId,
    pub x: f32,
    pub y: f32,
    pub cluster: usize,
}

pub(crate) struct ShapedCluster {
    pub range: Range<usize>,
    pub x: f32,
    pub advance: f32,
}

pub(crate) struct ShapedRun {
    text: Box<str>,
    style_hash: u64,
    pub glyphs: Vec<ShapedGlyph>,
    pub clusters: Vec<ShapedCluster>,
    pub advance: f32,
    cursive: bool,
}

impl ShapedRun {
    // The run of `text` as the shaper laid out its glyphs, each cluster
    // spanning the bytes up to the next cluster's start.
    fn new(
        text: &str,
        style_hash: u64,
        cursive: bool,
        (infos, positions): (&[GlyphInfo], &[GlyphPosition]),
    ) -> Self {
        let mut boundaries: Vec<usize> = infos.iter().map(|info| info.cluster as usize).collect();
        boundaries.push(text.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut run = Self {
            text: text.into(),
            style_hash,
            glyphs: Vec::with_capacity(infos.len()),
            clusters: Vec::new(),
            advance: 0.0,
            cursive,
        };
        let mut y = 0.0;
        for (info, position) in infos.iter().zip(positions) {
            let start = info.cluster as usize;
            if run
                .clusters
                .last()
                .is_none_or(|cluster| cluster.range.start != start)
            {
                let end = boundaries[boundaries.partition_point(|offset| *offset <= start)];
                run.clusters.push(ShapedCluster {
                    range: start..end,
                    x: run.advance,
                    advance: 0.0,
                });
            }
            let cluster = run.clusters.len() - 1;
            run.glyphs.push(ShapedGlyph {
                id: GlyphId(u16::try_from(info.glyph_id).unwrap_or_default()),
                x: run.advance + position.x_offset as f32,
                y: y + position.y_offset as f32,
                cluster,
            });
            run.clusters[cluster].advance += position.x_advance as f32;
            run.advance += position.x_advance as f32;
            y += position.y_advance as f32;
        }
        run
    }

    pub(crate) fn horizontal_scale(&self, natural: f32, synthesized: f32) -> f32 {
        if self.cursive { natural } else { synthesized }
    }

    pub(crate) fn width(&self, scale: f32, spacing: f32) -> f32 {
        (self.advance * scale + self.clusters.len() as f32 * self.spacing(spacing)).max(0.0)
    }

    pub(crate) fn spacing(&self, requested: f32) -> f32 {
        if self.cursive { 0.0 } else { requested }
    }

    pub(crate) fn visit(
        &self,
        scale_x: f32,
        scale_y: f32,
        spacing: f32,
        mut visit: impl FnMut(GlyphId, f32, f32),
    ) {
        let spacing = self.spacing(spacing);
        for glyph in &self.glyphs {
            visit(
                glyph.id,
                glyph.x * scale_x + (glyph.cluster as f32 + 0.5) * spacing,
                -glyph.y * scale_y,
            );
        }
    }
}

// The bytes of a font as `read-fonts` borrows them, shared with the font
// that owns them rather than copied.
struct FontBytes(FontArc);

impl AsRef<[u8]> for FontBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.font_data()
    }
}

// The shape plans a font keeps, the newest last: text uses few scripts and
// feature settings, and a plan takes as long to make as the shaping.
const KEPT_PLANS: usize = 8;

pub(crate) struct ShapingCache {
    entries: BoundedLruCache<u64, Arc<ShapedRun>>,
    buffer: Buffer,
    variations: Vec<Variation>,
    // Made when the font first shapes. It keeps the lookup accelerators the
    // shaper builds on first use.
    font: Option<Font>,
    plans: Vec<ShapePlan>,
}

impl ShapingCache {
    pub(crate) fn new(variations: &[([u8; 4], f32)]) -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(256),
            buffer: Buffer::new(),
            font: None,
            plans: Vec::new(),
            variations: variations
                .iter()
                .map(|(tag, value)| Variation::new(Tag::new(tag), *value))
                .collect(),
        }
    }

    pub(crate) fn shape(
        &mut self,
        font: &FontArc,
        text: &str,
        style: &TextStyle,
    ) -> Option<Arc<ShapedRun>> {
        let style_hash = style.measurement_hash();
        let mut hash = default::new();
        text.hash(&mut hash);
        style_hash.hash(&mut hash);
        let key = hash.finish();
        if let Some(run) = self.entries.get(&key)
            && run.text.as_ref() == text
            && run.style_hash == style_hash
        {
            return Some(Arc::clone(run));
        }
        if self.font.is_none() {
            let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(FontBytes(font.clone()));
            let base = Font::new(Blob::from(bytes), 0)?;
            self.font = Some(if self.variations.is_empty() {
                base
            } else {
                base.instance_builder()
                    .variations(self.variations.iter().copied())
                    .build()
            });
        }
        self.segment(text, style);
        let font = self.font.as_ref()?;
        let (direction, script) = (self.buffer.direction(), self.buffer.script());
        let cursive = matches!(
            script,
            Some(
                Script::ARABIC
                    | Script::SYRIAC
                    | Script::MONGOLIAN
                    | Script::NKO
                    | Script::MANDAIC
                    | Script::ADLAM
            )
        );
        let features: SmallVec<[Feature; 4]> = style
            .span_style
            .font_feature_settings
            .as_deref()
            .into_iter()
            .flat_map(font_feature_settings)
            .map(|feature| Feature::new(Tag::new(&feature.tag), feature.value, ..))
            .collect();
        let language = self.buffer.language().cloned();
        let wanted = ShapePlanKey::new(font, script, direction)
            .language(language.as_ref())
            .features(&features);
        let plan = match self.plans.iter().position(|plan| wanted.matches(plan)) {
            Some(at) => &self.plans[at],
            None => {
                if self.plans.len() == KEPT_PLANS {
                    self.plans.remove(0);
                }
                self.plans.push(ShapePlan::new(
                    font,
                    direction,
                    script,
                    language.as_ref(),
                    &features,
                ));
                &self.plans[self.plans.len() - 1]
            }
        };
        harfrust::shape(
            &ShaperFont::new(font),
            &mut self.buffer,
            ShapeOptions::new().plan(Some(plan)).features(&features),
        )
        .ok()?;
        let run = Arc::new(ShapedRun::new(
            text,
            style_hash,
            cursive,
            (self.buffer.glyph_infos(), self.buffer.glyph_positions()),
        ));
        self.entries.push(key, Arc::clone(&run));
        Some(run)
    }

    // Fills the scratch buffer with `text`, the direction and language
    // `style` asks for and the script the text is in.
    fn segment(&mut self, text: &str, style: &TextStyle) {
        let rtl = style.paragraph_style.text_direction.resolve(text) == ResolvedTextDirection::Rtl;
        let buffer = &mut self.buffer;
        buffer.clear();
        buffer.push_str(text);
        buffer.set_direction(if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        if let Some(language) = style
            .span_style
            .locale_list
            .as_ref()
            .and_then(|locales| locales.locales().first())
            .and_then(|tag| tag.parse::<Language>().ok())
        {
            buffer.set_language(Some(language));
        }
        buffer.guess_segment_properties();
    }
}
