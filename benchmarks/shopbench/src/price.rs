// A price, in the `sale` color with the compare-at price struck through
// when the compare-at price is higher.

use cranpose::{
    LinearArrangement, Modifier, Row, RowSpec, Text, VerticalAlignment, composable,
    text::FontWeight,
};

use crate::{
    model::Money,
    theme::{SALE, TEXT, TEXT_MUTED, struck, style},
};

#[composable]
pub(crate) fn Price(price: Money, compare_at: Option<Money>, size: f32, compare_size: f32) {
    let on_sale = compare_at
        .as_ref()
        .is_some_and(|compare_at| compare_at.value() > price.value());
    Row(
        Modifier::empty(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(6.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let color = if on_sale { SALE } else { TEXT };
            Text(
                price.formatted(),
                Modifier::empty().align_by_baseline(),
                style(size, FontWeight::BOLD, color),
            );
            if let Some(compare_at) = compare_at.as_ref().filter(|_| on_sale) {
                Text(
                    compare_at.formatted(),
                    Modifier::empty().align_by_baseline(),
                    struck(compare_size, TEXT_MUTED),
                );
            }
        },
    );
}
