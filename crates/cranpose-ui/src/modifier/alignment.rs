use super::{Alignment, HorizontalAlignment, Modifier, VerticalAlignment, inspector_metadata};
use crate::modifier_nodes::AlignmentElement;

impl Modifier {
    /// Aligns this child's first text baseline with siblings that also request
    /// baseline alignment in a Row. A child without a baseline is placed at the top.
    pub fn align_by_baseline(self) -> Self {
        self.then(
            Self::with_element(AlignmentElement::row_baseline())
                .with_inspector_metadata(inspector_metadata("alignByBaseline", |_| {})),
        )
    }

    pub fn align(self, alignment: Alignment) -> Self {
        self.then(
            Self::with_element(AlignmentElement::box_alignment(alignment)).with_inspector_metadata(
                inspector_metadata("align", move |info| {
                    info.add_alignment("boxAlignment", alignment);
                }),
            ),
        )
    }

    #[expect(non_snake_case)]
    pub fn alignInBox(self, alignment: Alignment) -> Self {
        self.align(alignment)
    }

    #[expect(non_snake_case)]
    pub fn alignInColumn(self, alignment: HorizontalAlignment) -> Self {
        let modifier = Self::with_element(AlignmentElement::column_alignment(alignment))
            .with_inspector_metadata(inspector_metadata("alignInColumn", move |info| {
                info.add_alignment("columnAlignment", alignment);
            }));
        self.then(modifier)
    }

    #[expect(non_snake_case)]
    pub fn alignInRow(self, alignment: VerticalAlignment) -> Self {
        let modifier = Self::with_element(AlignmentElement::row_alignment(alignment))
            .with_inspector_metadata(inspector_metadata("alignInRow", move |info| {
                info.add_alignment("rowAlignment", alignment);
            }));
        self.then(modifier)
    }
}
