use crate::{cluster_widths::ClusterWidths, line_breaker::BreakOpportunity};

// What breaking a paragraph into lines reads that its width does not
// change: the widths of its clusters and where its lines may break.
#[derive(Debug, Default)]
pub(crate) struct ParagraphMeasures {
    pub(crate) widths: ClusterWidths,
    pub(crate) breaks: Vec<BreakOpportunity>,
}
