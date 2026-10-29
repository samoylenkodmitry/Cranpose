use std::sync::Arc;

use crate::{cluster_widths::ClusterWidths, line_breaker::BreakOpportunity};

// What breaking a paragraph into lines reads that its width does not
// change: the widths of its clusters and where its lines may break. A
// layout that wrapped keeps them, so a layout of the same text in the same
// spans at another width takes them, as Compose's `Paragraph` takes its
// `ParagraphIntrinsics`.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParagraphMeasures {
    pub(crate) widths: ClusterWidths,
    pub(crate) breaks: Vec<BreakOpportunity>,
}

impl ParagraphMeasures {
    // The measures in `kept` to fill anew: their own storage when no layout
    // shares them.
    pub(crate) fn fresh(kept: &mut Arc<Self>) -> &mut Self {
        if Arc::get_mut(kept).is_none() {
            *kept = Arc::default();
        }
        Arc::make_mut(kept)
    }
}

// The measures a layout keeps for the layouts of its text at other widths.
// They follow from its text, spans and glyphs, so they leave two layouts
// that draw the same equal.
#[derive(Clone, Debug)]
pub(crate) struct KeptMeasures(pub(crate) Arc<ParagraphMeasures>);

impl PartialEq for KeptMeasures {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
