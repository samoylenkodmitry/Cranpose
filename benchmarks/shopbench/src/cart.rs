// Cart (`spec/design/screens/Cart.dc.html`): the lines with their quantity
// steppers, the subtotal and Check out, or the empty cart.

use std::rc::Rc;

use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, HorizontalAlignment, LazyColumn, LazyColumnSpec,
    LazyListScope, LinearArrangement, Modifier, PaddingValues, RoundedCornerShape, Row, RowSpec,
    Spacer, Text, TextWithOptions, VerticalAlignment, composable, local_uri_handler,
    rememberLazyListState, text::FontWeight,
};
use cranpose_coroflow::Handle;

use crate::{
    cart_store::CartStore,
    icons::{IconKind, StrokeIcon},
    model::{Cart, CartLine},
    remote_image::RemoteImage,
    theme::{
        BACKGROUND, BAR_HEIGHT, BORDER, ImageWidth, PADDING, TEXT, TEXT_MUTED, WHITE, centered,
        image_url, lines, paragraph, style,
    },
};

#[composable]
pub(crate) fn CartScreen(store: Handle<CartStore>, start_shopping: impl Fn() + Clone + 'static) {
    let cart = store
        .get()
        .cart()
        .get()
        .filter(|cart| !cart.lines.nodes.is_empty());
    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            Text(
                "Cart",
                Modifier::empty()
                    .fill_max_width()
                    .height(BAR_HEIGHT)
                    .padding_each(PADDING, 8.0, PADDING, 0.0),
                style(22.0, FontWeight::BOLD, TEXT),
            );
            match cart.clone() {
                Some(cart) => {
                    CartLines(cart.clone(), store);
                    Summary(cart);
                }
                None => EmptyCart(start_shopping.clone()),
            }
        },
    );
}

#[composable]
fn CartLines(cart: Cart, store: Handle<CartStore>) {
    let list = rememberLazyListState();
    let busy = store.get().busy().get();
    LazyColumn(
        Modifier::empty().fill_max_width().weight(1.0),
        list,
        LazyColumnSpec::default().content_padding(PaddingValues::symmetric(PADDING, 0.0)),
        move |scope| {
            let lines: Rc<[CartLine]> = cart.lines.nodes.clone().into();
            scope.items(lines.len(), move |index| {
                CartLineRow(lines[index].clone(), index, busy, store);
            });
        },
    );
}

#[composable]
fn CartLineRow(line: CartLine, index: usize, busy: bool, store: Handle<CartStore>) {
    let merchandise = &line.merchandise;
    let image = image_url(
        merchandise
            .image
            .as_ref()
            .or(merchandise.product.featured_image.as_ref()),
        ImageWidth::Thumbnail,
    );
    let title = merchandise.product.title.clone();
    let variant = Some(merchandise.title.clone()).filter(|title| title != "Default Title");
    Column(
        Modifier::empty()
            .fill_max_width()
            .test_tag(format!("cart-line-{index}"))
            .merge_descendants(),
        ColumnSpec::default(),
        move || {
            let (image, title, variant, line) =
                (image.clone(), title.clone(), variant.clone(), line.clone());
            Row(
                Modifier::empty().fill_max_width().padding_vertical(16.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                move || {
                    RemoteImage(
                        image.clone(),
                        Modifier::empty()
                            .size_points(80.0, 107.0)
                            .rounded_corners(8.0),
                    );
                    let (title, variant, line) = (title.clone(), variant.clone(), line.clone());
                    Column(
                        Modifier::empty().weight(1.0).height(107.0),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                        move || {
                            Text(
                                title.clone(),
                                Modifier::empty(),
                                style(15.0, FontWeight::SEMI_BOLD, TEXT),
                            );
                            if let Some(variant) = &variant {
                                Text(
                                    variant.clone(),
                                    Modifier::empty(),
                                    style(13.0, FontWeight::NORMAL, TEXT_MUTED),
                                );
                            }
                            Spacer(Modifier::empty().weight(1.0));
                            let line = line.clone();
                            Row(
                                Modifier::empty().fill_max_width(),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpaceBetween)
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                move || {
                                    Stepper(line.clone(), index, busy, store);
                                    Text(
                                        line.cost.total_amount.formatted(),
                                        Modifier::empty(),
                                        style(15.0, FontWeight::BOLD, TEXT),
                                    );
                                },
                            );
                        },
                    );
                },
            );
            Spacer(
                Modifier::empty()
                    .fill_max_width()
                    .height(1.0)
                    .background(BORDER),
            );
        },
    );
}

#[composable]
fn Stepper(line: CartLine, index: usize, busy: bool, store: Handle<CartStore>) {
    Row(
        Modifier::empty()
            .height(36.0)
            .border(1.0, BORDER, RoundedCornerShape::uniform(12.0)),
        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let quantity = line.quantity;
            StepButton(
                IconKind::Minus,
                "Decrease quantity",
                format!("qty-dec-{index}"),
                busy,
                {
                    let id = line.id.clone();
                    move || store.get().update(id.clone(), quantity.saturating_sub(1))
                },
            );
            Text(
                quantity.to_string(),
                Modifier::empty().width_in(24.0, f32::INFINITY),
                centered(style(15.0, FontWeight::SEMI_BOLD, TEXT)),
            );
            StepButton(
                IconKind::Plus,
                "Increase quantity",
                format!("qty-inc-{index}"),
                busy,
                {
                    let id = line.id.clone();
                    move || store.get().update(id.clone(), quantity + 1)
                },
            );
        },
    );
}

#[composable]
fn StepButton(
    icon: IconKind,
    label: &'static str,
    tag: String,
    busy: bool,
    step: impl Fn() + 'static,
) {
    Box(
        Modifier::empty()
            .size_points(36.0, 34.0)
            .content_description(label)
            .test_tag(tag)
            .clickable(move |_| {
                if !busy {
                    step();
                }
            }),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || StrokeIcon(icon, 16.0, 1.4, TEXT),
    );
}

#[composable]
fn Summary(cart: Cart) {
    let checkout = cart.checkout_url.clone();
    Column(
        Modifier::empty().fill_max_width().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            Spacer(
                Modifier::empty()
                    .fill_max_width()
                    .height(1.0)
                    .background(BORDER),
            );
            let (subtotal, checkout) = (cart.cost.subtotal_amount.formatted(), checkout.clone());
            Column(
                Modifier::empty().fill_max_width().padding(PADDING),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
                move || {
                    let subtotal = subtotal.clone();
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpaceBetween)
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            Text(
                                "Subtotal",
                                Modifier::empty(),
                                style(15.0, FontWeight::NORMAL, TEXT),
                            );
                            Text(
                                subtotal.clone(),
                                Modifier::empty().test_tag("cart-subtotal"),
                                style(17.0, FontWeight::BOLD, TEXT),
                            );
                        },
                    );
                    Text(
                        "Taxes and shipping are calculated at checkout.",
                        Modifier::empty(),
                        style(13.0, FontWeight::NORMAL, TEXT_MUTED),
                    );
                    let uri_handler = local_uri_handler().current();
                    let checkout = checkout.clone();
                    PrimaryButton(
                        "Check out",
                        Modifier::empty().fill_max_width().test_tag("checkout"),
                        52.0,
                        move || {
                            let _ = uri_handler.open_uri(&checkout);
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn EmptyCart(start_shopping: impl Fn() + Clone + 'static) {
    Column(
        Modifier::empty().fill_max_size().padding_horizontal(32.0),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::Center)
            .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        move || {
            Text(
                "Your cart is empty",
                Modifier::empty(),
                style(17.0, FontWeight::BOLD, TEXT),
            );
            Text(
                "Items you add will appear here.",
                Modifier::empty().padding_each(0.0, 12.0, 0.0, 0.0),
                centered(paragraph(15.0, 22.0, FontWeight::NORMAL, TEXT_MUTED)),
            );
            PrimaryButton(
                "Start shopping",
                Modifier::empty().padding_each(0.0, 20.0, 0.0, 0.0),
                48.0,
                start_shopping.clone(),
            );
        },
    );
}

#[composable]
fn PrimaryButton(
    label: &'static str,
    modifier: Modifier,
    height: f32,
    on_click: impl Fn() + 'static,
) {
    Box(
        modifier
            .height(height)
            .rounded_corners(12.0)
            .background(TEXT)
            .clickable(move |_| on_click()),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            TextWithOptions(
                label,
                Modifier::empty().padding_horizontal(24.0),
                style(16.0, FontWeight::SEMI_BOLD, WHITE),
                lines(1),
            );
        },
    );
}
