use crate::{
    layout_input::LayoutInput,
    paragraph_glyphs::{ParagraphGlyphs, WidthUnit},
    shaped_glyphs::ShapedGlyph,
    shaped_item::{ShapedItem, scale},
    text_layout::font_units,
    typeface::Typeface,
};

// The widths of a paragraph's clusters in text order, as running sums, so the
// width of any range between cluster boundaries is one subtraction.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClusterWidths {
    // The start of each cluster with the width of the text before it; the
    // last entry is the end of the text with the width of all of it. Text
    // offsets fit in 32 bits, as a glyph's cluster does.
    clusters: Vec<(u32, f32)>,
    total: ClusterTotal,
    // Whether every byte of the text starts a cluster, as in most ASCII
    // text: each cluster then sits at the index of its start.
    dense: bool,
    // The pixels the widths are kept in: device pixels where advances round
    // to them, so the widths of whole pixel advances add up exactly, and
    // pixels of the layout otherwise.
    unit: WidthUnit,
}

// The width of glyphs added in text order, each cluster `spacing` wider.
#[derive(Clone, Debug, Default)]
struct ClusterTotal {
    last: Option<u32>,
    width: f32,
}

impl ClusterTotal {
    // Adds a glyph of the cluster at `start`, `advance` wide, in a run whose
    // clusters each take `spacing` more; true when it starts a cluster.
    fn add(&mut self, start: u32, advance: f32, spacing: f32) -> bool {
        let starts = self.last != Some(start);
        if starts {
            self.last = Some(start);
            self.width += spacing;
        }
        self.width += advance;
        starts
    }
}

impl ClusterWidths {
    // Measures the clusters of `glyphs` of `items`, in `faces`.
    pub(crate) fn measure(
        &mut self,
        items: &[ShapedItem],
        input: &LayoutInput<'_>,
        faces: &[Typeface],
        glyphs: &ParagraphGlyphs,
    ) {
        self.clusters.clear();
        self.total = ClusterTotal::default();
        self.unit = glyphs.width_unit();
        each_cluster_glyph(
            items,
            input,
            faces,
            (glyphs, self.unit),
            |start, advance, spacing| {
                let before = self.total.width;
                if self.total.add(start, advance, spacing) {
                    self.clusters.push((start, before));
                }
            },
        );
        // Clusters start in text order, each once: as many as the text has
        // bytes start at every byte.
        self.dense = input.text.len() == self.clusters.len();
        self.clusters
            .push((held(input.text.len()), self.total.width));
    }

    // The width of the text from `start` to `end`.
    pub(crate) fn width(&self, start: usize, end: usize) -> f32 {
        self.unit
            .layout(self.kept_before(end) - self.kept_before(start))
    }

    // Whether `width`, a width of this text, fits `limit`.
    pub(crate) fn fits(&self, width: f32, limit: f32) -> bool {
        self.unit.fits(width, limit)
    }

    // The width of the text before `offset`, in the pixels it is kept in.
    fn kept_before(&self, offset: usize) -> f32 {
        if self.dense
            && let Some(&(_, width)) = self.clusters.get(offset)
        {
            return width;
        }
        let offset = held(offset);
        let at = self.clusters.partition_point(|&(start, _)| start < offset);
        self.clusters
            .get(at)
            .map_or(self.total.width, |&(_, width)| width)
    }

    // The last cluster boundary from `start` to `end` up to which the text
    // from `start` fits `width`; `start` when no cluster fits.
    pub(crate) fn last_fitting(&self, start: usize, end: usize, width: f32) -> usize {
        let limit = self.kept_before(start) + self.unit.limit(width);
        let last = held(end);
        let fits = self
            .clusters
            .partition_point(|&(cluster, before)| cluster <= last && before <= limit);
        self.clusters[..fits]
            .last()
            .map_or(start, |&(cluster, _)| offset(cluster).max(start))
    }

    // The first cluster boundary from `start` to `end` from which the text up
    // to `end` fits `width`; `end` when no cluster fits.
    pub(crate) fn first_fitting(&self, start: usize, end: usize, width: f32) -> usize {
        let limit = self.kept_before(end) - self.unit.limit(width);
        let first_start = held(start);
        let first = self
            .clusters
            .partition_point(|&(cluster, before)| cluster < first_start || before < limit);
        self.clusters
            .get(first)
            .map_or(end, |&(cluster, _)| offset(cluster).min(end))
    }

    // The first cluster boundary after `offset`.
    pub(crate) fn next_boundary(&self, offset: usize) -> Option<usize> {
        let after = held(offset);
        let at = self.clusters.partition_point(|&(start, _)| start <= after);
        self.clusters.get(at).map(|&(start, _)| self::offset(start))
    }
}

// `offset` as the clusters hold it.
fn held(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

// A text offset the clusters hold.
fn offset(held: u32) -> usize {
    usize::try_from(held).unwrap_or(usize::MAX)
}

// Calls `add` with the cluster start of each of `glyphs` of `items` in
// text order, its advance and the spacing its item adds per cluster, in
// `unit`s.
fn each_cluster_glyph(
    items: &[ShapedItem],
    input: &LayoutInput<'_>,
    faces: &[Typeface],
    (glyphs, unit): (&ParagraphGlyphs, WidthUnit),
    mut add: impl FnMut(u32, f32, f32),
) {
    for item in items {
        let own = &glyphs[item.glyphs.clone()];
        let pixels = glyphs.pixels(item.glyphs.clone());
        let spacing = unit.pixels(item.spacing);
        if pixels.is_empty() {
            let scale = scale(&input.spans[item.span], &faces[item.face]);
            let mut add_glyph = |glyph: &ShapedGlyph| {
                add(glyph.cluster, font_units(glyph.advance) * scale, spacing);
            };
            if item.level.is_rtl() {
                own.iter().rev().for_each(&mut add_glyph);
            } else {
                own.iter().for_each(&mut add_glyph);
            }
        } else {
            let mut add_glyph = |(glyph, pixels): (&ShapedGlyph, &f32)| {
                add(glyph.cluster, *pixels, spacing);
            };
            if item.level.is_rtl() {
                own.iter().zip(pixels).rev().for_each(&mut add_glyph);
            } else {
                own.iter().zip(pixels).for_each(&mut add_glyph);
            }
        }
    }
}
