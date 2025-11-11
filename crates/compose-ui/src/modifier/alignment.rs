use super::{inspector_metadata, Alignment, HorizontalAlignment, Modifier, VerticalAlignment};
use crate::modifier_nodes::AlignmentElement;

impl Modifier {
    pub fn align(alignment: Alignment) -> Self {
        Self::with_element(AlignmentElement::box_alignment(alignment)).with_inspector_metadata(
            inspector_metadata("align", move |info| {
                info.add_alignment("boxAlignment", alignment);
            }),
        )
    }

    pub fn alignInBox(self, alignment: Alignment) -> Self {
        self.then(Self::align(alignment))
    }

    pub fn alignInColumn(self, alignment: HorizontalAlignment) -> Self {
        let modifier = Self::with_element(AlignmentElement::column_alignment(alignment))
            .with_inspector_metadata(inspector_metadata("alignInColumn", move |info| {
                info.add_alignment("columnAlignment", alignment);
            }));
        self.then(modifier)
    }

    pub fn alignInRow(self, alignment: VerticalAlignment) -> Self {
        let modifier = Self::with_element(AlignmentElement::row_alignment(alignment))
            .with_inspector_metadata(inspector_metadata("alignInRow", move |info| {
                info.add_alignment("rowAlignment", alignment);
            }));
        self.then(modifier)
    }
}
