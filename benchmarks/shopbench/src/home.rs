// Home (`spec/design/screens/Main.dc.html`): the collection chips, the
// featured collection, the collections carousel and the best sellers.

use coroflow::MainScope;
use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Spacer,
    Text, TextWithOptions, VerticalAlignment, composable, rememberScrollState, text::FontWeight,
};
use cranpose_core::{OwnedMutableState, ownedMutableStateOf};
use cranpose_coroflow::viewModel;
use cranpose_navigation::rememberSaveableScrollState;

use crate::{
    api::Api,
    home_ready::ReportHomeReady,
    load::Load,
    model::{CollectionSummary, HomeData, ProductCard},
    product_card::ProductCardView,
    remote_image::RemoteImage,
    skeleton::HomeSkeleton,
    states::ErrorState,
    theme::{
        BAR_HEIGHT, CARD_WIDTH, GRID_COLUMN_GAP, GRID_ROW_GAP, ImageWidth, PADDING, SCRIM, SURFACE,
        TEXT, WHITE, image_url, letter_spaced, lines, paragraph, style,
    },
};

pub(crate) struct HomeModel {
    api: Api,
    scope: MainScope,
    data: OwnedMutableState<Load<HomeData>>,
}

impl HomeModel {
    fn new(scope: MainScope, api: Api) -> Self {
        let model = Self {
            api,
            scope,
            data: ownedMutableStateOf(Load::Loading),
        };
        model.load();
        model
    }

    fn load(&self) {
        let data = self.data.handle();
        let api = self.api.clone();
        data.set(Load::Loading);
        self.scope.launch(async move {
            data.set(Load::from_result(api.home().await));
        });
    }
}

#[composable]
pub(crate) fn HomeScreen(
    api: Api,
    open_collection: impl Fn(CollectionSummary) + Clone + 'static,
    open_product: impl Fn(ProductCard) + Clone + 'static,
) {
    let model = viewModel((), move |scope| HomeModel::new(scope, api)).get();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            TextWithOptions(
                "Everbloom",
                Modifier::empty()
                    .fill_max_width()
                    .height(BAR_HEIGHT)
                    .padding_horizontal(PADDING)
                    .padding_each(0.0, 8.0, 0.0, 0.0),
                letter_spaced(style(22.0, FontWeight::BOLD, TEXT), -0.4),
                lines(1),
            );
            match model.data.handle().get() {
                Load::Loading => HomeSkeleton(),
                Load::Failed => {
                    let model = model.clone();
                    ErrorState(move || model.load());
                }
                Load::Loaded(data) => {
                    HomeContent(data, open_collection.clone(), open_product.clone());
                    ReportHomeReady();
                }
            }
        },
    );
}

#[composable]
fn HomeContent(
    data: HomeData,
    open_collection: impl Fn(CollectionSummary) + Clone + 'static,
    open_product: impl Fn(ProductCard) + Clone + 'static,
) {
    let scroll = rememberSaveableScrollState("home");
    Column(
        Modifier::empty()
            .fill_max_size()
            .test_tag("home-scroll")
            .vertical_scroll(scroll, false),
        ColumnSpec::default(),
        move || {
            let collections = &data.collections.nodes;
            CollectionChips(collections.clone(), open_collection.clone());
            if let Some(featured) = collections.first() {
                FeaturedCollection(featured.clone(), open_collection.clone());
            }
            SectionHeading("Collections");
            CollectionsCarousel(collections.clone(), open_collection.clone());
            SectionHeading("Best sellers");
            BestSellers(data.products.nodes.clone(), open_product.clone());
        },
    );
}

#[composable]
fn CollectionChips(
    collections: Vec<CollectionSummary>,
    open_collection: impl Fn(CollectionSummary) + Clone + 'static,
) {
    let scroll = rememberScrollState!();
    Row(
        Modifier::empty()
            .fill_max_width()
            .horizontal_scroll(scroll, false)
            .padding_each(PADDING, 8.0, PADDING, 12.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for (index, collection) in collections.iter().enumerate() {
                let open = open_collection.clone();
                let target = collection.clone();
                Box(
                    Modifier::empty()
                        .height(36.0)
                        .rounded_corners(18.0)
                        .background(SURFACE)
                        .test_tag(format!("collection-chip-{index}"))
                        .clickable(move |_| open(target.clone()))
                        .padding_horizontal(14.0),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    {
                        let title = collection.title.clone();
                        move || {
                            TextWithOptions(
                                title.clone(),
                                Modifier::empty(),
                                style(14.0, FontWeight::MEDIUM, TEXT),
                                lines(1),
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn FeaturedCollection(
    featured: CollectionSummary,
    open_collection: impl Fn(CollectionSummary) + Clone + 'static,
) {
    let image = image_url(featured.image.as_ref(), ImageWidth::Banner);
    let title = featured.title.clone();
    Box(
        Modifier::empty()
            .padding_each(PADDING, 4.0, PADDING, 0.0)
            .fill_max_width()
            .height(240.0)
            .rounded_corners(12.0)
            .clip_to_bounds()
            .test_tag("featured-collection")
            .clickable(move |_| open_collection(featured.clone())),
        BoxSpec::new().content_alignment(Alignment::BOTTOM_START),
        move || {
            RemoteImage(image.clone(), Modifier::empty().fill_max_size());
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .background(SCRIM)
                    .padding(PADDING),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(2.0)),
                {
                    let title = title.clone();
                    move || {
                        Text(
                            "FEATURED COLLECTION",
                            Modifier::empty(),
                            letter_spaced(style(12.0, FontWeight::SEMI_BOLD, WHITE), 0.6),
                        );
                        Text(
                            title.clone(),
                            Modifier::empty(),
                            style(20.0, FontWeight::BOLD, WHITE),
                        );
                        Text(
                            "Shop now",
                            Modifier::empty(),
                            style(14.0, FontWeight::NORMAL, WHITE),
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn SectionHeading(title: &'static str) {
    Text(
        title,
        Modifier::empty().padding_each(PADDING, 28.0, PADDING, 12.0),
        style(17.0, FontWeight::BOLD, TEXT),
    );
}

#[composable]
fn CollectionsCarousel(
    collections: Vec<CollectionSummary>,
    open_collection: impl Fn(CollectionSummary) + Clone + 'static,
) {
    let scroll = rememberScrollState!();
    Row(
        Modifier::empty()
            .fill_max_width()
            .horizontal_scroll(scroll, false)
            .padding_horizontal(PADDING),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::Top),
        move || {
            for (index, collection) in collections.iter().enumerate() {
                let open = open_collection.clone();
                let target = collection.clone();
                let image = image_url(collection.image.as_ref(), ImageWidth::Card);
                let title = collection.title.clone();
                Column(
                    Modifier::empty()
                        .width(CARD_WIDTH)
                        .test_tag(format!("collection-card-{index}"))
                        .clickable(move |_| open(target.clone())),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        RemoteImage(
                            image.clone(),
                            Modifier::empty()
                                .size_points(CARD_WIDTH, 175.0)
                                .rounded_corners(8.0),
                        );
                        TextWithOptions(
                            title.clone(),
                            Modifier::empty(),
                            paragraph(14.0, 18.0, FontWeight::SEMI_BOLD, TEXT),
                            lines(2),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn BestSellers(products: Vec<ProductCard>, open_product: impl Fn(ProductCard) + Clone + 'static) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding_each(PADDING, 0.0, PADDING, 24.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(GRID_ROW_GAP)),
        move || {
            for (row, pair) in products.chunks(2).enumerate() {
                let pair = pair.to_vec();
                let open_product = open_product.clone();
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(GRID_COLUMN_GAP))
                        .vertical_alignment(VerticalAlignment::Top),
                    move || {
                        for (column, product) in pair.iter().enumerate() {
                            let open = open_product.clone();
                            let target = product.clone();
                            ProductCardView(
                                product.clone(),
                                format!("best-seller-{}", row * 2 + column),
                                Modifier::empty().weight(1.0),
                                move || open(target.clone()),
                            );
                        }
                        if pair.len() == 1 {
                            Spacer(Modifier::empty().weight(1.0));
                        }
                    },
                );
            }
        },
    );
}
