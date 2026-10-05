use std::{
    hash::{Hash, Hasher},
    ops::Range,
    sync::Arc,
};

use ab_glyph::GlyphId;
use cranpose_core::{collections::bounded_lru::BoundedLruCache, hash::default};
use cranpose_ui::text::{ResolvedTextDirection, TextShaping, TextStyle};
use rustybuzz::{Direction, Face, Feature, UnicodeBuffer, Variation};
use smallvec::SmallVec;

use crate::font_features::font_feature_settings;

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
    pub fn horizontal_scale(&self, natural: f32, synthesized: f32) -> f32 {
        if self.cursive { natural } else { synthesized }
    }

    pub fn width(&self, scale: f32, spacing: f32) -> f32 {
        (self.advance * scale + self.clusters.len() as f32 * self.spacing(spacing)).max(0.0)
    }

    pub fn spacing(&self, requested: f32) -> f32 {
        if self.cursive { 0.0 } else { requested }
    }

    pub fn visit(
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

pub(crate) struct ShapingCache {
    entries: BoundedLruCache<u64, Arc<ShapedRun>>,
    buffer: Option<UnicodeBuffer>,
    variations: Vec<Variation>,
}

impl ShapingCache {
    pub fn new(variations: &[([u8; 4], f32)]) -> Self {
        Self {
            entries: BoundedLruCache::with_capacity_at_least_one(256),
            buffer: None,
            variations: variations
                .iter()
                .map(|(tag, value)| Variation {
                    tag: rustybuzz::ttf_parser::Tag::from_bytes(tag),
                    value: *value,
                })
                .collect(),
        }
    }

    pub fn shape(&mut self, bytes: &[u8], text: &str, style: &TextStyle) -> Option<Arc<ShapedRun>> {
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
        let mut face = Face::from_slice(bytes, 0)?;
        face.set_variations(&self.variations);
        let rtl = style.paragraph_style.text_direction.resolve(text) == ResolvedTextDirection::Rtl;
        let mut buffer = self.buffer.take().unwrap_or_default();
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
            .and_then(|tag| tag.parse().ok())
        {
            buffer.set_language(language);
        }
        buffer.guess_segment_properties();
        let cursive = matches!(
            buffer.script(),
            rustybuzz::script::ARABIC
                | rustybuzz::script::SYRIAC
                | rustybuzz::script::MONGOLIAN
                | rustybuzz::script::NKO
                | rustybuzz::script::MANDAIC
                | rustybuzz::script::ADLAM
        );
        let features: SmallVec<[Feature; 4]> = style
            .span_style
            .font_feature_settings
            .as_deref()
            .into_iter()
            .flat_map(font_feature_settings)
            .map(|feature| {
                Feature::new(
                    rustybuzz::ttf_parser::Tag::from_bytes(&feature.tag),
                    feature.value,
                    ..,
                )
            })
            .collect();
        let shaped = rustybuzz::shape(&face, &features, buffer);
        let mut boundaries: Vec<usize> = shaped
            .glyph_infos()
            .iter()
            .map(|info| info.cluster as usize)
            .collect();
        boundaries.push(text.len());
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut run = ShapedRun {
            text: text.into(),
            style_hash,
            glyphs: Vec::with_capacity(shaped.len()),
            clusters: Vec::new(),
            advance: 0.0,
            cursive,
        };
        let mut y = 0.0;
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
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
                id: GlyphId(info.glyph_id as u16),
                x: run.advance + position.x_offset as f32,
                y: y + position.y_offset as f32,
                cluster,
            });
            run.clusters[cluster].advance += position.x_advance as f32;
            run.advance += position.x_advance as f32;
            y += position.y_advance as f32;
        }
        self.buffer = Some(shaped.clear());
        let run = Arc::new(run);
        self.entries.push(key, Arc::clone(&run));
        Some(run)
    }
}
