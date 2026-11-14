// The app: three tabs, Home and Search with a stack of screens each, Cart
// with one, and the alert a cart error opens.

use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, Modifier, MutableState, Text, composable,
    local_http_client, local_safe_area_insets, remember, rememberMutableStateOf,
    text::FontWeight,
    widgets::{Dialog, DialogSpec},
};
use cranpose_coroflow::{Handle, viewModel};
use cranpose_navigation::{NavController, NavHost, rememberNavController};

use crate::{
    api::Api,
    cart::CartScreen,
    cart_store::CartStore,
    collection::CollectionScreen,
    home::HomeScreen,
    model::{CollectionSummary, ProductCard},
    product::ProductScreen,
    search::SearchScreen,
    tab_bar::{Tab, TabBar},
    theme::{BACKGROUND, PADDING, TEXT, WHITE, centered, paragraph, style},
};

#[derive(Clone, Debug, PartialEq)]
enum Route {
    Home,
    Search,
    Collection { handle: String, title: String },
    Product { handle: String },
}

#[composable]
pub(crate) fn App() {
    let client = local_http_client().current();
    let api = remember(move || Api::from_environment(client)).with(Api::clone);
    let cart = viewModel((), {
        let api = api.clone();
        move |scope| CartStore::new(scope, api)
    });
    let tab = rememberMutableStateOf(|| Tab::Home);
    let home = rememberNavController(Route::Home);
    let search = rememberNavController(Route::Search);
    let selected = tab.get();
    let stack = match selected {
        Tab::Home => Some(home),
        Tab::Search => Some(search),
        Tab::Cart => None,
    };
    let on_product = stack
        .and_then(|stack| stack.current_route())
        .is_some_and(|route| matches!(route, Route::Product { .. }));
    let top = local_safe_area_insets().current().top;
    let store = cart.get();
    let badge = store
        .cart()
        .with(|cart| cart.as_ref().map_or(0, |cart| cart.total_quantity));
    let alert = store.alert();
    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            let api = api.clone();
            // The product screen draws its images under the status bar.
            let inset = if on_product { 0.0 } else { top };
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .padding_each(0.0, inset, 0.0, 0.0),
                BoxSpec::default(),
                move || match stack {
                    Some(stack) => TabStack(stack, api.clone(), cart, tab),
                    None => CartScreen(cart, move || tab.set(Tab::Home)),
                },
            );
            if !on_product {
                TabBar(selected, badge, move |chosen| tab.set(chosen));
            }
        },
    );
    CartAlert(alert);
}

// The screens of one tab's stack.
#[composable]
fn TabStack(
    stack: NavController<Route>,
    api: Api,
    cart: Handle<CartStore>,
    tab: MutableState<Tab>,
) {
    NavHost(stack, move |route| {
        let back = move || {
            stack.pop_back_stack();
        };
        let open_product = move |product: ProductCard| {
            stack.navigate(Route::Product {
                handle: product.handle,
            });
        };
        match route {
            Route::Home => HomeScreen(
                api.clone(),
                move |collection: CollectionSummary| {
                    stack.navigate(Route::Collection {
                        handle: collection.handle,
                        title: collection.title,
                    });
                },
                open_product,
            ),
            Route::Search => SearchScreen(api.clone(), open_product),
            Route::Collection { handle, title } => {
                CollectionScreen(api.clone(), handle, title, back, open_product)
            }
            Route::Product { handle } => {
                ProductScreen(api.clone(), handle, cart, back, open_product, move || {
                    tab.set(Tab::Cart)
                })
            }
        }
    });
}

#[composable]
fn CartAlert(alert: MutableState<Option<String>>) {
    let Some(message) = alert.get() else {
        return;
    };
    Dialog(
        DialogSpec::default(),
        move |_| alert.set(None),
        move || {
            let message = message.clone();
            Column(
                Modifier::empty()
                    .padding_horizontal(48.0)
                    .rounded_corners(14.0)
                    .background(WHITE)
                    .padding(PADDING),
                ColumnSpec::default(),
                move || {
                    Text(
                        message.clone(),
                        Modifier::empty().fill_max_width(),
                        centered(paragraph(15.0, 22.0, FontWeight::NORMAL, TEXT)),
                    );
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .padding_each(0.0, 12.0, 0.0, 0.0)
                            .height(44.0)
                            .clickable(move |_| alert.set(None)),
                        BoxSpec::new().content_alignment(Alignment::CENTER),
                        || {
                            Text(
                                "OK",
                                Modifier::empty(),
                                style(17.0, FontWeight::SEMI_BOLD, TEXT),
                            );
                        },
                    );
                },
            );
        },
    );
}
