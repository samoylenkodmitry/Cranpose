// The product card of the Home best sellers and the collection grid, and the
// narrower card of the recommendations carousel.

use cranpose::{
    Column, ColumnSpec, LinearArrangement, Modifier, TextWithOptions, composable, text::FontWeight,
};

use crate::{
    aspect_ratio::AspectRatio,
    model::ProductCard,
    price::Price,
    remote_image::RemoteImage,
    theme::{CARD_WIDTH, ImageWidth, TEXT, image_url, lines, paragraph},
};

#[composable]
pub(crate) fn ProductCardView(
    product: ProductCard,
    tag: String,
    modifier: Modifier,
    on_click: impl Fn() + 'static,
) {
    let image = image_url(product.featured_image.as_ref(), ImageWidth::Card);
    Column(
        modifier.test_tag(tag).clickable(move |_| on_click()),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            AspectRatio(3.0 / 4.0, Modifier::empty().fill_max_width(), {
                let image = image.clone();
                move || {
                    RemoteImage(
                        image.clone(),
                        Modifier::empty().fill_max_size().rounded_corners(8.0),
                    );
                }
            });
            CardText(product.clone(), 15.0);
        },
    );
}

#[composable]
pub(crate) fn RecommendationCard(product: ProductCard, tag: String, on_click: impl Fn() + 'static) {
    let image = image_url(product.featured_image.as_ref(), ImageWidth::Card);
    Column(
        Modifier::empty()
            .width(CARD_WIDTH)
            .test_tag(tag)
            .clickable(move |_| on_click()),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            RemoteImage(
                image.clone(),
                Modifier::empty()
                    .size_points(CARD_WIDTH, 187.0)
                    .rounded_corners(8.0),
            );
            CardText(product.clone(), 14.0);
        },
    );
}

// The title on up to two lines and the price, under the image of a card.
#[composable]
fn CardText(product: ProductCard, price_size: f32) {
    TextWithOptions(
        product.title.clone(),
        Modifier::empty(),
        paragraph(14.0, 18.0, FontWeight::NORMAL, TEXT),
        lines(2),
    );
    Price(
        product.price().clone(),
        product.compare_at().cloned(),
        price_size,
        13.0,
    );
}
