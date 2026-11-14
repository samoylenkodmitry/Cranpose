// The Storefront API's answers, as `spec/storefront.graphql` asks for them.

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(crate) struct Nodes<T> {
    pub(crate) nodes: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct Money {
    amount: String,
}

impl Money {
    pub(crate) fn value(&self) -> f64 {
        self.amount.parse().unwrap_or(0.0)
    }

    // The prompt shows `"75.0"` as `$75.00`.
    pub(crate) fn formatted(&self) -> String {
        format!("${:.2}", self.value())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ImageRef {
    pub(crate) url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PriceRange {
    pub(crate) min_variant_price: Money,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInfo {
    pub(crate) has_next_page: bool,
    pub(crate) end_cursor: Option<String>,
}

impl PageInfo {
    pub(crate) fn next(&self) -> Option<&str> {
        self.end_cursor.as_deref().filter(|_| self.has_next_page)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductCard {
    pub(crate) handle: String,
    pub(crate) title: String,
    pub(crate) featured_image: Option<ImageRef>,
    pub(crate) price_range: PriceRange,
    pub(crate) compare_at_price_range: Option<PriceRange>,
}

impl ProductCard {
    pub(crate) fn price(&self) -> &Money {
        &self.price_range.min_variant_price
    }

    pub(crate) fn compare_at(&self) -> Option<&Money> {
        self.compare_at_price_range
            .as_ref()
            .map(|range| &range.min_variant_price)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct CollectionSummary {
    pub(crate) handle: String,
    pub(crate) title: String,
    pub(crate) image: Option<ImageRef>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct HomeData {
    pub(crate) collections: Nodes<CollectionSummary>,
    pub(crate) products: Nodes<ProductCard>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductPage {
    pub(crate) page_info: PageInfo,
    pub(crate) nodes: Vec<ProductCard>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct CollectionDetail {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) products: ProductPage,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct CollectionProductsData {
    pub(crate) collection: Option<CollectionDetail>,
}

// The query asks only for products, and anything else arrives as an empty
// object.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum SearchNode {
    Product(ProductCard),
    Other(serde_json::Value),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchPage {
    pub(crate) total_count: usize,
    pub(crate) page_info: PageInfo,
    pub(crate) nodes: Vec<SearchNode>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct SearchData {
    pub(crate) search: SearchPage,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductOption {
    pub(crate) name: String,
    pub(crate) option_values: Vec<OptionValue>,
}

impl ProductOption {
    // The prompt hides an option named `Title` whose one value is `Default Title`.
    pub(crate) fn is_shown(&self) -> bool {
        !(self.name == "Title"
            && matches!(self.option_values.as_slice(), [only] if only.name == "Default Title"))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct OptionValue {
    pub(crate) name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct SelectedOption {
    pub(crate) name: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Variant {
    pub(crate) id: String,
    pub(crate) available_for_sale: bool,
    pub(crate) selected_options: Vec<SelectedOption>,
    pub(crate) price: Money,
    pub(crate) compare_at_price: Option<Money>,
}

impl Variant {
    pub(crate) fn value_of(&self, option: &str) -> Option<&str> {
        self.selected_options
            .iter()
            .find(|selected| selected.name == option)
            .map(|selected| selected.value.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct ProductDetail {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) images: Nodes<ImageRef>,
    pub(crate) options: Vec<ProductOption>,
    pub(crate) variants: Nodes<Variant>,
}

impl ProductDetail {
    // The prompt shows the first variant for sale first, else the first one.
    pub(crate) fn initial_variant(&self) -> Option<usize> {
        let variants = &self.variants.nodes;
        variants
            .iter()
            .position(|variant| variant.available_for_sale)
            .or((!variants.is_empty()).then_some(0))
    }

    // The variant whose options match `current`'s with `option` set to
    // `value`, else the first variant with that value.
    pub(crate) fn variant_with(
        &self,
        current: Option<usize>,
        option: &str,
        value: &str,
    ) -> Option<usize> {
        let variants = &self.variants.nodes;
        let wanted = |name: &str| -> Option<&str> {
            if name == option {
                return Some(value);
            }
            current
                .and_then(|index| variants.get(index))
                .and_then(|variant| variant.value_of(name))
        };
        variants
            .iter()
            .position(|variant| {
                variant
                    .selected_options
                    .iter()
                    .all(|selected| wanted(&selected.name) == Some(selected.value.as_str()))
            })
            .or_else(|| {
                variants
                    .iter()
                    .position(|variant| variant.value_of(option) == Some(value))
            })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductData {
    pub(crate) product: Option<ProductDetail>,
    pub(crate) product_recommendations: Option<Vec<ProductCard>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CartProduct {
    pub(crate) title: String,
    pub(crate) featured_image: Option<ImageRef>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct Merchandise {
    pub(crate) title: String,
    pub(crate) image: Option<ImageRef>,
    pub(crate) product: CartProduct,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LineCost {
    pub(crate) total_amount: Money,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct CartLine {
    pub(crate) id: String,
    pub(crate) quantity: u32,
    pub(crate) cost: LineCost,
    pub(crate) merchandise: Merchandise,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CartCost {
    pub(crate) subtotal_amount: Money,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Cart {
    pub(crate) id: String,
    pub(crate) checkout_url: String,
    pub(crate) total_quantity: u32,
    pub(crate) cost: CartCost,
    pub(crate) lines: Nodes<CartLine>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(crate) struct UserError {
    pub(crate) message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CartPayload {
    pub(crate) cart: Option<Cart>,
    pub(crate) user_errors: Vec<UserError>,
}
