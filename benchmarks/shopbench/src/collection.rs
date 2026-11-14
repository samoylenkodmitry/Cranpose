// Collection (`spec/design/screens/Collection.dc.html`): the description, the
// sort chips and the products of a collection, a page at a time.

use std::{cell::Cell, rc::Rc};

use coroflow::MainScope;
use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, GridCells, LaunchedEffect, LazyGridState,
    LazyItems, LazyListScope, LazyVerticalGrid, LazyVerticalGridSpec, LinearArrangement, Modifier,
    MutableState, PaddingValues, RoundedCornerShape, Row, RowSpec, Text, TextWithOptions,
    composable, rememberScrollState, text::FontWeight,
};
use cranpose_core::{OwnedMutableState, ownedMutableStateOf};
use cranpose_coroflow::{Handle, viewModel};
use cranpose_navigation::rememberSaveableLazyGridState;

use crate::{
    api::Api,
    load::Load,
    model::{CollectionProductsData, ProductCard},
    nav_header::NavHeader,
    near_end::LoadMoreNearEnd,
    product_card::ProductCardView,
    sort::Sort,
    states::{ErrorState, Loading, LoadingMore},
    theme::{
        BACKGROUND, BORDER, GRID_COLUMN_GAP, GRID_ROW_GAP, PADDING, TEXT, TEXT_MUTED, WHITE, lines,
        paragraph, style,
    },
};

pub(crate) struct CollectionModel {
    api: Api,
    handle: String,
    scope: MainScope,
    title: OwnedMutableState<String>,
    description: OwnedMutableState<String>,
    sort: OwnedMutableState<Sort>,
    first_page: OwnedMutableState<Load<()>>,
    products: OwnedMutableState<Rc<[ProductCard]>>,
    next: OwnedMutableState<Option<String>>,
    loading_more: OwnedMutableState<bool>,
    // Answers for an older sort are dropped.
    generation: Rc<Cell<u64>>,
    // A new sort shows its first products from the top.
    scroll_to_top: Cell<bool>,
}

impl CollectionModel {
    fn new(scope: MainScope, api: Api, handle: String, title: String) -> Self {
        let model = Self {
            api,
            handle,
            scope,
            title: ownedMutableStateOf(title),
            description: ownedMutableStateOf(String::new()),
            sort: ownedMutableStateOf(Sort::default()),
            first_page: ownedMutableStateOf(Load::Loading),
            products: ownedMutableStateOf(Rc::from([])),
            next: ownedMutableStateOf(None),
            loading_more: ownedMutableStateOf(false),
            generation: Rc::default(),
            scroll_to_top: Cell::new(false),
        };
        model.reload();
        model
    }

    fn select(&self, sort: Sort) {
        if self.sort.handle().get_non_reactive() == sort {
            return;
        }
        self.sort.handle().set(sort);
        self.scroll_to_top.set(true);
        self.reload();
    }

    fn reload(&self) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.products.handle().set(Rc::from([]));
        self.next.handle().set(None);
        self.loading_more.handle().set(false);
        self.first_page.handle().set(Load::Loading);
        let page = self.page_handles();
        let (api, handle, sort) = (
            self.api.clone(),
            self.handle.clone(),
            self.sort.handle().get_non_reactive(),
        );
        let (title, description, first_page) = (
            self.title.handle(),
            self.description.handle(),
            self.first_page.handle(),
        );
        self.scope.launch(async move {
            let answer = api.collection_products(&handle, sort, None).await;
            if page.generation.get() != generation {
                return;
            }
            match answer.ok().and_then(|data| data.collection) {
                Some(collection) => {
                    title.set(collection.title.clone());
                    description.set(collection.description.clone());
                    page.append(CollectionProductsData {
                        collection: Some(collection),
                    });
                    first_page.set(Load::Loaded(()));
                }
                None => first_page.set(Load::Failed),
            }
        });
    }

    fn load_more(&self) {
        let loading_more = self.loading_more.handle();
        let first_loaded = matches!(
            self.first_page.handle().get_non_reactive(),
            Load::Loaded(())
        );
        let Some(cursor) = self.next.handle().get_non_reactive() else {
            return;
        };
        if !first_loaded || loading_more.get_non_reactive() {
            return;
        }
        loading_more.set(true);
        let generation = self.generation.get();
        let page = self.page_handles();
        let (api, handle, sort) = (
            self.api.clone(),
            self.handle.clone(),
            self.sort.handle().get_non_reactive(),
        );
        self.scope.launch(async move {
            let answer = api.collection_products(&handle, sort, Some(&cursor)).await;
            if page.generation.get() != generation {
                return;
            }
            if let Ok(data) = answer {
                page.append(data);
            }
            loading_more.set(false);
        });
    }

    fn page_handles(&self) -> PageHandles {
        PageHandles {
            products: self.products.handle(),
            next: self.next.handle(),
            generation: Rc::clone(&self.generation),
        }
    }
}

struct PageHandles {
    products: MutableState<Rc<[ProductCard]>>,
    next: MutableState<Option<String>>,
    generation: Rc<Cell<u64>>,
}

impl PageHandles {
    fn append(&self, data: CollectionProductsData) {
        let Some(collection) = data.collection else {
            return;
        };
        let page = collection.products;
        self.next.set(page.page_info.next().map(str::to_owned));
        let products = self.products.with(|products| {
            products
                .iter()
                .cloned()
                .chain(page.nodes)
                .collect::<Rc<[_]>>()
        });
        self.products.set(products);
    }
}

#[composable]
pub(crate) fn CollectionScreen(
    api: Api,
    handle: String,
    title: String,
    back: impl Fn() + Clone + 'static,
    open_product: impl Fn(ProductCard) + Clone + 'static,
) {
    let shared = viewModel((), move |scope| {
        CollectionModel::new(scope, api, handle, title)
    });
    let model = shared.get();
    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            NavHeader(model.title.handle().get(), back.clone());
            let description = model.description.handle().get();
            if !description.is_empty() {
                Text(
                    description,
                    Modifier::empty()
                        .fill_max_width()
                        .padding_each(PADDING, 4.0, PADDING, 0.0),
                    paragraph(14.0, 20.0, FontWeight::NORMAL, TEXT_MUTED),
                );
            }
            SortChips(model.sort.handle().get(), {
                let model = model.clone();
                move |sort| model.select(sort)
            });
            match model.first_page.handle().get() {
                Load::Failed => {
                    let model = model.clone();
                    ErrorState(move || model.reload());
                }
                Load::Loading => Loading(),
                Load::Loaded(()) => ProductGrid(shared, open_product.clone()),
            }
        },
    );
}

#[composable]
fn SortChips(selected: Sort, select: impl Fn(Sort) + Clone + 'static) {
    let scroll = rememberScrollState!();
    Row(
        Modifier::empty()
            .fill_max_width()
            .horizontal_scroll(scroll, false)
            .padding_symmetric(PADDING, 12.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for sort in Sort::ALL {
                let chosen = sort == selected;
                let select = select.clone();
                let (fill, ink, edge) = if chosen {
                    (TEXT, WHITE, TEXT)
                } else {
                    (BACKGROUND, TEXT, BORDER)
                };
                let weight = if chosen {
                    FontWeight::SEMI_BOLD
                } else {
                    FontWeight::MEDIUM
                };
                Box(
                    Modifier::empty()
                        .height(32.0)
                        .rounded_corners(16.0)
                        .background(fill)
                        .border(1.0, edge, RoundedCornerShape::uniform(16.0))
                        .test_tag(sort.tag())
                        .clickable(move |_| select(sort))
                        .padding_horizontal(12.0),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    move || {
                        TextWithOptions(
                            sort.label(),
                            Modifier::empty(),
                            style(13.0, weight, ink),
                            lines(1),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn ProductGrid(
    shared: Handle<CollectionModel>,
    open_product: impl Fn(ProductCard) + Clone + 'static,
) {
    let model = shared.get();
    let grid: LazyGridState = rememberSaveableLazyGridState("collection-grid");
    LaunchedEffect((), {
        let grid = grid.clone();
        async move |_| {
            if shared.get().scroll_to_top.replace(false) {
                grid.scroll_to_item(0, 0.0);
            }
        }
    });
    let products = model.products.handle().get();
    let loading_more = model.loading_more.handle().get();
    LazyVerticalGrid(
        Modifier::empty()
            .fill_max_width()
            .weight(1.0)
            .test_tag("product-grid"),
        grid.clone(),
        LazyVerticalGridSpec {
            vertical_arrangement: LinearArrangement::SpacedBy(GRID_ROW_GAP),
            horizontal_spacing: GRID_COLUMN_GAP,
            content_padding: PaddingValues::new(PADDING, 0.0, PADDING, PADDING),
            ..LazyVerticalGridSpec::new(GridCells::Fixed(2))
        },
        move |scope| {
            let products = products.clone();
            let open_product = open_product.clone();
            scope.items(products.len(), move |index| {
                let product = products[index].clone();
                let open = open_product.clone();
                let target = product.clone();
                ProductCardView(
                    product,
                    format!("product-card-{index}"),
                    Modifier::empty(),
                    move || open(target.clone()),
                );
            });
            if loading_more {
                scope.items(LazyItems::new(1).span(|line, _| line.max_line_span), |_| {
                    LoadingMore()
                });
            }
        },
    );
    LoadMoreNearEnd(grid.lazy_list_state(), move || shared.get().load_more());
}
