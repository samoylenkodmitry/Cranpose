use std::sync::Arc;

use crate::{cluster_widths::ClusterWidths, line_breaker::BreakOpportunity};

// What breaking a paragraph into lines reads that its width does not
// change: the widths of its clusters and where its lines may break. A
// layout of the same text in the same spans at another width takes them,
// as Compose's `Paragraph` takes its `ParagraphIntrinsics`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParagraphMeasures {
    pub(crate) widths: ClusterWidths,
    pub(crate) breaks: Vec<BreakOpportunity>,
}

impl ParagraphMeasures {
    // The measures in `kept` to fill anew: their own storage when the kept
    // measures do not hold them.
    pub(crate) fn fresh(kept: &mut Arc<Self>) -> &mut Self {
        if Arc::get_mut(kept).is_none() {
            *kept = Arc::default();
        }
        Arc::make_mut(kept)
    }
}
