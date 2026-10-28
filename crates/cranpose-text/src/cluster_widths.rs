// The widths of a paragraph's clusters in text order, as running sums, so the
// width of any range between cluster boundaries is one subtraction.
#[derive(Default)]
pub(crate) struct ClusterWidths {
    // The start of each cluster with the width of the text before it; the
    // last entry is the end of the text with the width of all of it.
    clusters: Vec<(usize, f32)>,
    total: f32,
}

impl ClusterWidths {
    pub(crate) fn clear(&mut self) {
        self.clusters.clear();
        self.total = 0.0;
    }

    // Adds a glyph of the cluster at `start`, in text order, `advance` wide,
    // in a run whose clusters each take `spacing` more.
    pub(crate) fn push(&mut self, start: usize, advance: f32, spacing: f32) {
        if self.clusters.last().is_none_or(|&(last, _)| last != start) {
            self.clusters.push((start, self.total));
            self.total += spacing;
        }
        self.total += advance;
    }

    // Ends the clusters at `end`, the end of the text.
    pub(crate) fn finish(&mut self, end: usize) {
        self.clusters.push((end, self.total));
    }

    // The width of the text from `start` to `end`.
    pub(crate) fn width(&self, start: usize, end: usize) -> f32 {
        self.before(end) - self.before(start)
    }

    fn before(&self, offset: usize) -> f32 {
        let at = self.clusters.partition_point(|&(start, _)| start < offset);
        self.clusters
            .get(at)
            .map_or(self.total, |&(_, width)| width)
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
