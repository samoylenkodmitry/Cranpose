// Home's loading skeleton (`spec/design/screens/States.dc.html`), drawn on
// the first frame while the `Home` request runs.

use cranpose::{
    Box, BoxSpec, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, composable,
};

use crate::theme::{CARD_WIDTH, PADDING, SKELETON};

#[composable]
pub(crate) fn HomeSkeleton() {
    Column(
        Modifier::empty().fill_max_size().hide_from_accessibility(),
        ColumnSpec::default(),
        || {
            Row(
                Modifier::empty().padding_each(PADDING, 8.0, PADDING, 12.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                || {
                    for width in [52.0, 110.0, 96.0, 80.0] {
                        Block(Modifier::empty().size_points(width, 36.0), 18.0);
                    }
                },
            );
            Block(
                Modifier::empty()
                    .padding_each(PADDING, 4.0, PADDING, 0.0)
                    .fill_max_width()
                    .height(240.0),
                12.0,
            );
            Block(
                Modifier::empty()
                    .padding_each(PADDING, 28.0, PADDING, 12.0)
                    .size_points(120.0, 18.0),
                4.0,
            );
            Row(
                Modifier::empty()
                    .padding_horizontal(PADDING)
                    .clip_to_bounds(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                || {
                    for _ in 0..3 {
                        Block(Modifier::empty().size_points(CARD_WIDTH, 175.0), 8.0);
                    }
                },
            );
        },
    );
}

#[composable]
fn Block(modifier: Modifier, radius: f32) {
    Box(
        modifier.rounded_corners(radius).background(SKELETON),
        BoxSpec::default(),
        || {},
    );
}
