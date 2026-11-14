// Product (`spec/design/screens/Product.dc.html`): the image carousel, the
// title and price, the option pills, the description, the recommendations and
// the Add to cart bar.

use std::rc::Rc;

use coroflow::MainScope;
use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, LinearArrangement, Modifier, MutableState,
    RoundedCornerShape, Row, RowSpec, Text, TextWithOptions, VerticalAlignment, composable,
    local_safe_area_insets, rememberScrollState,
    text::FontWeight,
    widgets::{HorizontalPager, PagerSpec, rememberPagerState},
};
use cranpose_core::{OwnedMutableState, ownedMutableStateOf};
use cranpose_coroflow::{Handle, viewModel};
use cranpose_navigation::rememberSaveableScrollState;

use crate::{
    api::Api,
    aspect_ratio::AspectRatio,
    cart_store::CartStore,
    icons::{IconKind, StrokeIcon},
    load::Load,
    model::{ProductCard, ProductDetail, ProductOption, Variant},
    price::Price,
    product_card::RecommendationCard,
    remote_image::RemoteImage,
    states::{ErrorState, Loading},
    theme::{
        BACKGROUND, BORDER, ImageWidth, PADDING, SURFACE, TEXT, TEXT_BODY, TEXT_MUTED, WHITE,
        image_url, lines, paragraph, style,
    },
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProductPage {
    product: ProductDetail,
    recommendations: Vec<ProductCard>,
}

pub(crate) struct ProductModel {
    api: Api,
    handle: String,
    scope: MainScope,
    page: OwnedMutableState<Load<Rc<ProductPage>>>,
    variant: OwnedMutableState<Option<usize>>,
}

impl ProductModel {
    fn new(scope: MainScope, api: Api, handle: String) -> Self {
        let model = Self {
            api,
            handle,
            scope,
            page: ownedMutableStateOf(Load::Loading),
            variant: ownedMutableStateOf(None),
        };
        model.load();
        model
    }

    fn load(&self) {
        let (page, variant) = (self.page.handle(), self.variant.handle());
        let (api, handle) = (self.api.clone(), self.handle.clone());
        page.set(Load::Loading);
        self.scope.launch(async move {
            let answer = api.product(&handle).await;
            match answer
                .ok()
                .and_then(|data| Some((data.product?, data.product_recommendations)))
            {
                Some((product, recommendations)) => {
                    variant.set(product.initial_variant());
                    page.set(Load::Loaded(Rc::new(ProductPage {
                        product,
                        recommendations: recommendations.unwrap_or_default(),
                    })));
                }
                None => page.set(Load::Failed),
            }
        });
    }
}

#[composable]
pub(crate) fn ProductScreen(
    api: Api,
    handle: String,
    cart: Handle<CartStore>,
    back: impl Fn() + Clone + 'static,
    open_product: impl Fn(ProductCard) + Clone + 'static,
    show_cart: impl Fn() + Clone + 'static,
) {
    let model = viewModel((), move |scope| ProductModel::new(scope, api, handle)).get();
    let top = local_safe_area_insets().current().top;
    Box(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        BoxSpec::new().content_alignment(Alignment::TOP_START),
        move || {
            match model.page.handle().get() {
                Load::Loading => Loading(),
                Load::Failed => {
                    let model = model.clone();
                    ErrorState(move || model.load());
                }
                Load::Loaded(page) => ProductContent(
                    page,
                    model.variant.handle(),
                    cart,
                    open_product.clone(),
                    show_cart.clone(),
                ),
            }
            RoundBackButton(top, back.clone());
        },
    );
}

#[composable]
fn RoundBackButton(top: f32, back: impl Fn() + 'static) {
    Box(
        Modifier::empty()
            .padding_each(12.0, top, 0.0, 0.0)
            .size_points(44.0, 44.0)
            .content_description("Back")
            .test_tag("back")
            .clickable(move |_| back()),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        || {
            Box(
                Modifier::empty()
                    .size_points(40.0, 40.0)
                    .rounded_corners(20.0)
                    .background(BACKGROUND),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || StrokeIcon(IconKind::Back, 22.0, 1.6, TEXT),
            );
        },
    );
}

#[composable]
fn ProductContent(
    page: Rc<ProductPage>,
    variant: MutableState<Option<usize>>,
    cart: Handle<CartStore>,
    open_product: impl Fn(ProductCard) + Clone + 'static,
    show_cart: impl Fn() + Clone + 'static,
) {
    let scroll = rememberSaveableScrollState("product");
    let selected = variant.get();
    let chosen: Option<Variant> =
        selected.and_then(|index| page.product.variants.nodes.get(index).cloned());
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let page = page.clone();
            let shown = chosen.clone();
            let open_product = open_product.clone();
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .vertical_scroll(scroll, false),
                ColumnSpec::default(),
                move || {
                    let product = &page.product;
                    let chosen = &shown;
                    Carousel(
                        product
                            .images
                            .nodes
                            .iter()
                            .filter_map(|image| image_url(Some(image), ImageWidth::Banner))
                            .collect(),
                    );
                    TitleAndPrice(product.title.clone(), chosen.clone());
                    for option in product.options.iter().filter(|option| option.is_shown()) {
                        let current = chosen
                            .as_ref()
                            .and_then(|chosen| chosen.value_of(&option.name))
                            .unwrap_or_default()
                            .to_owned();
                        let page = page.clone();
                        OptionPicker(
                            option.clone(),
                            current,
                            move |name: String, value: String| {
                                variant.set(page.product.variant_with(selected, &name, &value));
                            },
                        );
                    }
                    Description(product.description.clone());
                    if !page.recommendations.is_empty() {
                        Recommendations(page.recommendations.clone(), open_product.clone());
                    }
                },
            );
            AddToCartBar(chosen.clone(), cart, show_cart.clone());
        },
    );
}

#[composable]
fn TitleAndPrice(title: String, chosen: Option<Variant>) {
    Column(
        Modifier::empty().padding_each(PADDING, 20.0, PADDING, 0.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            Text(
                title.clone(),
                Modifier::empty(),
                paragraph(22.0, 28.0, FontWeight::BOLD, TEXT),
            );
            if let Some(chosen) = &chosen {
                Price(
                    chosen.price.clone(),
                    chosen.compare_at_price.clone(),
                    20.0,
                    15.0,
                );
            }
        },
    );
}

#[composable]
fn Description(description: String) {
    Column(
        Modifier::empty().padding_each(PADDING, 24.0, PADDING, 0.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                "Description",
                Modifier::empty(),
                style(15.0, FontWeight::SEMI_BOLD, TEXT),
            );
            Text(
                description.clone(),
                Modifier::empty(),
                paragraph(15.0, 22.0, FontWeight::NORMAL, TEXT_BODY),
            );
        },
    );
}

#[composable]
fn Carousel(images: Vec<String>) {
    let pager = rememberPagerState(0);
    AspectRatio(3.0 / 4.0, Modifier::empty().fill_max_width(), move || {
        let images = images.clone();
        Box(
            Modifier::empty().fill_max_size().background(SURFACE),
            BoxSpec::new().content_alignment(Alignment::BOTTOM_CENTER),
            move || {
                let pages = images.clone();
                // The pager's own scroll container is inside it: the name makes
                // the carousel itself an element the runner can find.
                HorizontalPager(
                    Modifier::empty()
                        .fill_max_size()
                        .content_description("Product images")
                        .test_tag("product-carousel"),
                    pager,
                    PagerSpec::default(),
                    pages.len(),
                    move |index| {
                        RemoteImage(pages.get(index).cloned(), Modifier::empty().fill_max_size());
                    },
                );
                if images.len() > 1 {
                    PageDots(images.len(), pager.current_page());
                }
            },
        );
    });
}

#[composable]
fn PageDots(count: usize, current: usize) {
    Row(
        Modifier::empty().padding_each(0.0, 0.0, 0.0, 12.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            for index in 0..count {
                let color = if index == current {
                    TEXT
                } else {
                    TEXT.with_alpha(0.25)
                };
                Box(
                    Modifier::empty()
                        .size_points(8.0, 8.0)
                        .rounded_corners(4.0)
                        .background(color),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

#[composable]
fn OptionPicker(
    option: ProductOption,
    current: String,
    select: impl Fn(String, String) + Clone + 'static,
) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 24.0, 0.0, 0.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Row(
                Modifier::empty().padding_horizontal(PADDING),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(4.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    let name = option.name.clone();
                    let current = current.clone();
                    move || {
                        Text(
                            format!("{name}:"),
                            Modifier::empty(),
                            style(15.0, FontWeight::SEMI_BOLD, TEXT),
                        );
                        Text(
                            current.clone(),
                            Modifier::empty(),
                            style(15.0, FontWeight::NORMAL, TEXT_MUTED),
                        );
                    }
                },
            );
            let scroll = rememberScrollState!();
            let option = option.clone();
            let current = current.clone();
            let select = select.clone();
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .horizontal_scroll(scroll, false)
                    .padding_symmetric(PADDING, 1.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    for value in &option.option_values {
                        let chosen = value.name == current;
                        let select = select.clone();
                        let (name, picked) = (option.name.clone(), value.name.clone());
                        let weight = if chosen {
                            FontWeight::SEMI_BOLD
                        } else {
                            FontWeight::MEDIUM
                        };
                        let (edge, width) = if chosen { (TEXT, 2.0) } else { (BORDER, 1.0) };
                        Box(
                            Modifier::empty()
                                .width_in(56.0, f32::INFINITY)
                                .height_in(44.0, f32::INFINITY)
                                .border(width, edge, RoundedCornerShape::uniform(12.0))
                                .test_tag(format!("option-{}-{}", option.name, value.name))
                                .clickable(move |_| select(name.clone(), picked.clone()))
                                .padding_horizontal(PADDING),
                            BoxSpec::new().content_alignment(Alignment::CENTER),
                            {
                                let label = value.name.clone();
                                move || {
                                    Text(
                                        label.clone(),
                                        Modifier::empty(),
                                        style(15.0, weight, TEXT),
                                    );
                                }
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn Recommendations(
    products: Vec<ProductCard>,
    open_product: impl Fn(ProductCard) + Clone + 'static,
) {
    Text(
        "You may also like",
        Modifier::empty().padding_each(PADDING, 32.0, PADDING, 12.0),
        style(17.0, FontWeight::BOLD, TEXT),
    );
    let scroll = rememberScrollState!();
    Row(
        Modifier::empty()
            .fill_max_width()
            .horizontal_scroll(scroll, false)
            .padding_each(PADDING, 0.0, PADDING, 24.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::Top),
        move || {
            for (index, product) in products.iter().enumerate() {
                let open = open_product.clone();
                let target = product.clone();
                RecommendationCard(
                    product.clone(),
                    format!("recommendation-{index}"),
                    move || open(target.clone()),
                );
            }
        },
    );
}

#[composable]
fn AddToCartBar(
    chosen: Option<Variant>,
    cart: Handle<CartStore>,
    show_cart: impl Fn() + Clone + 'static,
) {
    let bottom = local_safe_area_insets().current().bottom;
    let in_flight = cart.get().busy();
    let busy = in_flight.get();
    let price = chosen
        .as_ref()
        .map(|chosen| chosen.price.formatted())
        .unwrap_or_default();
    Column(
        Modifier::empty().fill_max_width().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(1.0)
                    .background(BORDER),
                BoxSpec::default(),
                || {},
            );
            let chosen = chosen.clone();
            let show_cart = show_cart.clone();
            let fill = if busy { TEXT.with_alpha(0.5) } else { TEXT };
            Box(
                Modifier::empty()
                    .padding_each(PADDING, 12.0, PADDING, 8.0 + bottom)
                    .fill_max_width()
                    .height(52.0)
                    .rounded_corners(12.0)
                    .background(fill)
                    .test_tag("add-to-cart")
                    .clickable(move |_| {
                        let Some(chosen) =
                            chosen.as_ref().filter(|_| !in_flight.get_non_reactive())
                        else {
                            return;
                        };
                        let show_cart = show_cart.clone();
                        cart.get().add(chosen.id.clone(), move |added| {
                            if added {
                                show_cart();
                            }
                        });
                    }),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                {
                    let price = price.clone();
                    move || {
                        TextWithOptions(
                            format!("Add to cart \u{b7} {price}"),
                            Modifier::empty(),
                            style(16.0, FontWeight::SEMI_BOLD, WHITE),
                            lines(1),
                        );
                    }
                },
            );
        },
    );
}
