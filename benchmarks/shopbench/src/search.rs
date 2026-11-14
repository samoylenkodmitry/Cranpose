// Search (`spec/design/screens/Search.dc.html`): a field whose query goes out
// 300 ms after the typing stops, the result count and the results, a page at
// a time.

use std::{cell::Cell, rc::Rc, time::Duration};

use coroflow::MainScope;
use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, ImeAction, KeyboardActionHandler,
    KeyboardCapitalization, KeyboardOptions, LaunchedEffect, LazyColumn, LazyColumnSpec,
    LazyListScope, LazyListState, LinearArrangement, Modifier, Row, RowSpec, Text,
    TextFieldLineLimits, TextFieldState, VerticalAlignment, composable, delay, local_focus_manager,
    text::FontWeight,
    widgets::{BasicTextFieldDecorated, BasicTextFieldOptions},
};
use cranpose_core::{OwnedMutableState, ownedMutableStateOf};
use cranpose_coroflow::{Handle, viewModel};

use crate::{
    api::Api,
    icons::{IconKind, StrokeIcon},
    load::Load,
    model::{ProductCard, SearchData, SearchNode},
    near_end::LoadMoreNearEnd,
    price::Price,
    remote_image::RemoteImage,
    states::{ErrorState, Loading, LoadingMore},
    theme::{
        BACKGROUND, ImageWidth, PADDING, SURFACE, TEXT, TEXT_MUTED, centered, image_url, paragraph,
        style,
    },
};

const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq)]
struct Results {
    products: Rc<[ProductCard]>,
    total: usize,
}

pub(crate) struct SearchModel {
    api: Api,
    scope: MainScope,
    field: TextFieldState,
    // The trimmed query the results belong to; empty shows the hint.
    query: OwnedMutableState<String>,
    results: OwnedMutableState<Load<Results>>,
    next: OwnedMutableState<Option<String>>,
    loading_more: OwnedMutableState<bool>,
    list: LazyListState,
    // Answers for an older query are dropped.
    generation: Rc<Cell<u64>>,
}

impl SearchModel {
    fn new(scope: MainScope, api: Api) -> Self {
        Self {
            api,
            scope,
            field: TextFieldState::new(""),
            query: ownedMutableStateOf(String::new()),
            results: ownedMutableStateOf(Load::Loading),
            next: ownedMutableStateOf(None),
            loading_more: ownedMutableStateOf(false),
            list: LazyListState::new(0, 0.0),
            generation: Rc::default(),
        }
    }

    // A new query drops what the last one showed and any answer still on its
    // way.
    fn changed(&self, query: String) {
        self.generation.set(self.generation.get() + 1);
        self.query.handle().set(query);
        self.next.handle().set(None);
        self.loading_more.handle().set(false);
        self.results.handle().set(Load::Loading);
        self.list.scroll_to_item(0, 0.0);
    }

    // Runs once the typing has stopped for `DEBOUNCE`, and on Retry.
    fn search(&self) {
        let query = self.query.handle().get_non_reactive();
        if query.is_empty() {
            return;
        }
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.results.handle().set(Load::Loading);
        let (api, results, next, current) = (
            self.api.clone(),
            self.results.handle(),
            self.next.handle(),
            Rc::clone(&self.generation),
        );
        self.scope.launch(async move {
            let answer = api.search(&query, None).await;
            if current.get() != generation {
                return;
            }
            results.set(match answer {
                Ok(data) => {
                    let (products, total, cursor) = page(data);
                    next.set(cursor);
                    Load::Loaded(Results {
                        products: products.into(),
                        total,
                    })
                }
                Err(_) => Load::Failed,
            });
        });
    }

    fn load_more(&self) {
        let loading_more = self.loading_more.handle();
        let Some(cursor) = self.next.handle().get_non_reactive() else {
            return;
        };
        if loading_more.get_non_reactive() {
            return;
        }
        loading_more.set(true);
        let generation = self.generation.get();
        let query = self.query.handle().get_non_reactive();
        let (api, results, next, current) = (
            self.api.clone(),
            self.results.handle(),
            self.next.handle(),
            Rc::clone(&self.generation),
        );
        self.scope.launch(async move {
            let answer = api.search(&query, Some(&cursor)).await;
            if current.get() != generation {
                return;
            }
            if let Ok(data) = answer {
                let (more, total, cursor) = page(data);
                next.set(cursor);
                let shown = results.with(|shown| match shown {
                    Load::Loaded(shown) => {
                        Some(shown.products.iter().cloned().chain(more).collect())
                    }
                    _ => None,
                });
                if let Some(products) = shown {
                    results.set(Load::Loaded(Results { products, total }));
                }
            }
            loading_more.set(false);
        });
    }
}

fn page(data: SearchData) -> (Vec<ProductCard>, usize, Option<String>) {
    let search = data.search;
    let cursor = search.page_info.next().map(str::to_owned);
    let products = search
        .nodes
        .into_iter()
        .filter_map(|node| match node {
            SearchNode::Product(product) => Some(product),
            SearchNode::Other(_) => None,
        })
        .collect();
    (products, search.total_count, cursor)
}

#[composable]
pub(crate) fn SearchScreen(api: Api, open_product: impl Fn(ProductCard) + Clone + 'static) {
    let shared = viewModel((), move |scope| SearchModel::new(scope, api));
    let model = shared.get();
    let typed = model.field.with_text(|text| text.trim().to_owned());
    LaunchedEffect(typed.clone(), {
        let model = model.clone();
        async move |_| {
            if typed == model.query.handle().get_non_reactive() {
                return;
            }
            model.changed(typed);
            delay(DEBOUNCE).await;
            model.search();
        }
    });
    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            SearchField(model.field);
            if model.query.handle().with(String::is_empty) {
                Text(
                    "Search across 995 products by name.",
                    Modifier::empty()
                        .fill_max_width()
                        .padding_symmetric(32.0, 48.0),
                    centered(paragraph(15.0, 22.0, FontWeight::NORMAL, TEXT_MUTED)),
                );
                return;
            }
            match model.results.handle().get() {
                Load::Failed => {
                    let model = model.clone();
                    ErrorState(move || model.search());
                }
                Load::Loading => Loading(),
                Load::Loaded(results) => {
                    SearchResults(shared, results, open_product.clone());
                }
            }
        },
    );
}

#[composable]
fn SearchField(field: TextFieldState) {
    Row(
        Modifier::empty()
            .padding_each(PADDING, 4.0, PADDING, 12.0)
            .fill_max_width()
            .height(44.0)
            .rounded_corners(12.0)
            .background(SURFACE)
            .padding_horizontal(12.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            StrokeIcon(IconKind::Search, 20.0, 1.75, TEXT_MUTED);
            let focus = local_focus_manager().current();
            BasicTextFieldDecorated(
                field,
                Modifier::empty().weight(1.0).test_tag("search-input"),
                BasicTextFieldOptions {
                    text_style: style(16.0, FontWeight::NORMAL, TEXT),
                    cursor_color: TEXT,
                    line_limits: TextFieldLineLimits::SingleLine,
                    keyboard_options: KeyboardOptions {
                        capitalization: KeyboardCapitalization::None,
                        auto_correct: false,
                        ime_action: ImeAction::Search,
                        ..KeyboardOptions::default()
                    },
                    // The search key puts the keyboard away; the query goes
                    // out as the typing stops.
                    on_keyboard_action: Some(KeyboardActionHandler::new(move |_| {
                        focus.clear_focus();
                    })),
                    ..BasicTextFieldOptions::default()
                },
                move |scope| {
                    Box(
                        Modifier::empty().fill_max_width(),
                        BoxSpec::new().content_alignment(Alignment::CENTER_START),
                        {
                            let scope = scope.clone();
                            move || {
                                if field.with_text(str::is_empty) {
                                    Text(
                                        "Search products",
                                        Modifier::empty().hide_from_accessibility(),
                                        style(16.0, FontWeight::NORMAL, TEXT_MUTED),
                                    );
                                }
                                scope.inner_text_field();
                            }
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn SearchResults(
    shared: Handle<SearchModel>,
    results: Results,
    open_product: impl Fn(ProductCard) + Clone + 'static,
) {
    let model = shared.get();
    Text(
        format!("{} results", results.total),
        Modifier::empty()
            .fill_max_width()
            .padding_each(PADDING, 4.0, PADDING, 8.0),
        style(13.0, FontWeight::NORMAL, TEXT_MUTED),
    );
    let loading_more = model.loading_more.handle().get();
    let list = model.list;
    LazyColumn(
        Modifier::empty()
            .fill_max_width()
            .weight(1.0)
            .test_tag("search-results"),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            let products = results.products.clone();
            let open_product = open_product.clone();
            scope.items(products.len(), move |index| {
                let product = products[index].clone();
                let open = open_product.clone();
                SearchRow(product, index, move |product| open(product));
            });
            if loading_more {
                scope.item(LoadingMore);
            }
        },
    );
    LoadMoreNearEnd(list, move || shared.get().load_more());
}

#[composable]
fn SearchRow(product: ProductCard, index: usize, open: impl Fn(ProductCard) + 'static) {
    let image = image_url(product.featured_image.as_ref(), ImageWidth::Thumbnail);
    let target = product.clone();
    Row(
        Modifier::empty()
            .fill_max_width()
            .test_tag(format!("search-result-{index}"))
            .clickable(move |_| open(target.clone()))
            .padding_symmetric(PADDING, 8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            RemoteImage(
                image.clone(),
                Modifier::empty()
                    .size_points(64.0, 85.0)
                    .rounded_corners(8.0),
            );
            let product = product.clone();
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    Text(
                        product.title.clone(),
                        Modifier::empty(),
                        paragraph(15.0, 20.0, FontWeight::NORMAL, TEXT),
                    );
                    Price(
                        product.price().clone(),
                        product.compare_at().cloned(),
                        15.0,
                        13.0,
                    );
                },
            );
        },
    );
}
