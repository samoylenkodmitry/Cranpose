use crate::{
    layout_input::LayoutInput,
    shaped_item::{ShapedItem, scale},
    shaper::ShapedGlyph,
    text_layout::{TextLayout, font_units},
};

// The widths of a paragraph's clusters in text order, as running sums, so the
// width of any range between cluster boundaries is one subtraction.
#[derive(Default)]
pub(crate) struct ClusterWidths {
    // The start of each cluster with the width of the text before it; the
    // last entry is the end of the text with the width of all of it.
    clusters: Vec<(usize, f32)>,
    total: ClusterTotal,
}

// The width of glyphs added in text order, each cluster `spacing` wider.
#[derive(Default)]
pub(crate) struct ClusterTotal {
    last: Option<usize>,
    width: f32,
}

impl ClusterTotal {
    // The width of the glyphs of `items`, laid out in `layout`.
    pub(crate) fn of(items: &[ShapedItem], input: &LayoutInput<'_>, layout: &TextLayout) -> f32 {
        let mut total = Self::default();
        each_cluster_glyph(items, input, layout, |start, advance, spacing| {
            total.add(start, advance, spacing);
        });
        total.width
    }

    // Adds a glyph of the cluster at `start`, `advance` wide, in a run whose
    // clusters each take `spacing` more; true when it starts a cluster.
    fn add(&mut self, start: usize, advance: f32, spacing: f32) -> bool {
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
    // Measures the clusters of the glyphs of `items`, laid out in `layout`.
    pub(crate) fn measure(
        &mut self,
        items: &[ShapedItem],
        input: &LayoutInput<'_>,
        layout: &TextLayout,
    ) {
        self.clusters.clear();
        self.total = ClusterTotal::default();
        each_cluster_glyph(items, input, layout, |start, advance, spacing| {
            let before = self.total.width;
            if self.total.add(start, advance, spacing) {
                self.clusters.push((start, before));
            }
        });
        self.clusters.push((input.text.len(), self.total.width));
    }

    // The width of the text from `start` to `end`.
    pub(crate) fn width(&self, start: usize, end: usize) -> f32 {
        self.before(end) - self.before(start)
    }

    // The width of the text before `offset`.
    pub(crate) fn before(&self, offset: usize) -> f32 {
        let at = self.clusters.partition_point(|&(start, _)| start < offset);
        self.clusters
            .get(at)
            .map_or(self.total.width, |&(_, width)| width)
    }

    // The last cluster boundary from `start` to `end` up to which the text
    // from `start` fits `width`; `start` when no cluster fits.
    pub(crate) fn last_fitting(&self, start: usize, end: usize, width: f32) -> usize {
        let limit = self.before(start) + width;
        let fits = self
            .clusters
            .partition_point(|&(cluster, before)| cluster <= end && before <= limit);
        self.clusters[..fits]
            .last()
            .map_or(start, |&(cluster, _)| cluster.max(start))
    }

    // The first cluster boundary from `start` to `end` from which the text up
    // to `end` fits `width`; `end` when no cluster fits.
    pub(crate) fn first_fitting(&self, start: usize, end: usize, width: f32) -> usize {
        let limit = self.before(end) - width;
        let first = self
            .clusters
            .partition_point(|&(cluster, before)| cluster < start || before < limit);
        self.clusters
            .get(first)
            .map_or(end, |&(cluster, _)| cluster.min(end))
    }

    // The first cluster boundary after `offset`.
    pub(crate) fn next_boundary(&self, offset: usize) -> Option<usize> {
        let at = self.clusters.partition_point(|&(start, _)| start <= offset);
        self.clusters.get(at).map(|&(start, _)| start)
    }
}

// Calls `add` with the cluster start of each glyph of `items` in text
// order, its advance in pixels and the spacing its item adds per cluster.
fn each_cluster_glyph(
    items: &[ShapedItem],
    input: &LayoutInput<'_>,
    layout: &TextLayout,
    mut add: impl FnMut(usize, f32, f32),
) {
    for item in items {
        let scale = scale(&input.spans[item.span], &layout.faces[item.face]);
        let glyphs = &layout.glyphs[item.glyphs.clone()];
        let mut add_glyph = |glyph: &ShapedGlyph| {
            add(
                glyph.text_offset(),
                font_units(glyph.advance) * scale,
                item.spacing,
            );
        };
        if item.level.is_rtl() {
            glyphs.iter().rev().for_each(&mut add_glyph);
        } else {
            glyphs.iter().for_each(&mut add_glyph);
        }
    }
}
