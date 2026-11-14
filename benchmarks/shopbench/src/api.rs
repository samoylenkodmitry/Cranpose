// The Storefront GraphQL operations of `spec/storefront.graphql`, sent as
// plain HTTP requests.

use std::{rc::Rc, sync::Arc};

use anyhow::{Context, Result};
use cranpose_services::{HttpClient, HttpControl, HttpMethod, HttpRequest};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    model::{CartPayload, CollectionProductsData, HomeData, ProductData, SearchData},
    sort::Sort,
};

const DEFAULT_ENDPOINT: &str = "https://apparel-large-catalog.mock.shop/api";
const PAGE_SIZE: u32 = 24;

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
}

#[derive(Clone)]
pub(crate) struct Api {
    endpoint: Rc<str>,
    client: Arc<dyn HttpClient>,
}

impl PartialEq for Api {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.endpoint, &other.endpoint)
    }
}

impl Api {
    // The benchmark points the app at its replay server through
    // `SHOP_API_URL`, set when it launches the app.
    #[expect(
        clippy::disallowed_methods,
        reason = "the benchmark's runner sets SHOP_API_URL in the app's environment"
    )]
    pub(crate) fn from_environment(client: Arc<dyn HttpClient>) -> Self {
        let endpoint = std::env::var("SHOP_API_URL")
            .ok()
            .map(|url| url.trim().to_owned())
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned());
        Self {
            endpoint: endpoint.into(),
            client,
        }
    }

    async fn send<T: DeserializeOwned>(
        &self,
        operation: &str,
        document: &str,
        variables: Value,
    ) -> Result<T> {
        let body = json!({
            "operationName": operation,
            "query": document,
            "variables": variables,
        });
        let request = HttpRequest::get(self.endpoint.as_ref())
            .method(HttpMethod::Post)
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(&body)?);
        let response = self
            .client
            .send(&request, HttpControl::new())
            .await?
            .error_for_status()?;
        let bytes = response.read_all().await?;
        serde_json::from_slice::<Envelope<T>>(&bytes)?
            .data
            .with_context(|| format!("{operation}: the response has no data"))
    }

    pub(crate) async fn home(&self) -> Result<HomeData> {
        self.send("Home", HOME, json!({})).await
    }

    pub(crate) async fn collection_products(
        &self,
        handle: &str,
        sort: Sort,
        after: Option<&str>,
    ) -> Result<CollectionProductsData> {
        let mut variables = json!({
            "handle": handle,
            "first": PAGE_SIZE,
            "sortKey": sort.key(),
            "reverse": sort.reverse(),
        });
        if let Some(after) = after {
            variables["after"] = json!(after);
        }
        self.send("CollectionProducts", COLLECTION_PRODUCTS, variables)
            .await
    }

    pub(crate) async fn search(&self, query: &str, after: Option<&str>) -> Result<SearchData> {
        let mut variables = json!({ "query": query, "first": PAGE_SIZE });
        if let Some(after) = after {
            variables["after"] = json!(after);
        }
        self.send("Search", SEARCH, variables).await
    }

    pub(crate) async fn product(&self, handle: &str) -> Result<ProductData> {
        self.send("Product", PRODUCT, json!({ "handle": handle }))
            .await
    }

    pub(crate) async fn cart_create(&self, variant: &str) -> Result<CartPayload> {
        let lines = json!([{ "merchandiseId": variant, "quantity": 1 }]);
        let data: CartCreateData = self
            .send("CartCreate", CART_CREATE, json!({ "lines": lines }))
            .await?;
        Ok(data.cart_create)
    }

    pub(crate) async fn cart_lines_add(&self, cart: &str, variant: &str) -> Result<CartPayload> {
        let lines = json!([{ "merchandiseId": variant, "quantity": 1 }]);
        let data: CartLinesAddData = self
            .send(
                "CartLinesAdd",
                CART_LINES_ADD,
                json!({ "cartId": cart, "lines": lines }),
            )
            .await?;
        Ok(data.cart_lines_add)
    }

    pub(crate) async fn cart_lines_update(
        &self,
        cart: &str,
        line: &str,
        quantity: u32,
    ) -> Result<CartPayload> {
        let lines = json!([{ "id": line, "quantity": quantity }]);
        let data: CartLinesUpdateData = self
            .send(
                "CartLinesUpdate",
                CART_LINES_UPDATE,
                json!({ "cartId": cart, "lines": lines }),
            )
            .await?;
        Ok(data.cart_lines_update)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CartCreateData {
    cart_create: CartPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CartLinesAddData {
    cart_lines_add: CartPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CartLinesUpdateData {
    cart_lines_update: CartPayload,
}

// Each operation of `spec/storefront.graphql` with the fragments it uses, as
// written there.
macro_rules! product_card {
    () => {
        "fragment ProductCard on Product {
  id
  handle
  title
  featuredImage { url altText }
  priceRange { minVariantPrice { amount currencyCode } }
  compareAtPriceRange { minVariantPrice { amount currencyCode } }
}
"
    };
}

macro_rules! cart_fields {
    () => {
        "fragment CartFields on Cart {
  id
  checkoutUrl
  totalQuantity
  cost { subtotalAmount { amount currencyCode } }
  lines(first: 100) {
    nodes {
      id
      quantity
      cost { totalAmount { amount currencyCode } }
      merchandise {
        ... on ProductVariant {
          id
          title
          image { url altText }
          product { handle title featuredImage { url altText } }
        }
      }
    }
  }
}
"
    };
}

const HOME: &str = concat!(
    "query Home {
  collections(first: 20) {
    nodes { id handle title image { url altText } }
  }
  products(first: 10, sortKey: BEST_SELLING) {
    nodes { ...ProductCard }
  }
}

",
    product_card!()
);

const COLLECTION_PRODUCTS: &str = concat!(
    "query CollectionProducts($handle: String!, $first: Int!, $after: String, $sortKey: ProductCollectionSortKeys!, $reverse: Boolean!) {
  collection(handle: $handle) {
    id
    title
    description
    products(first: $first, after: $after, sortKey: $sortKey, reverse: $reverse) {
      pageInfo { hasNextPage endCursor }
      nodes { ...ProductCard }
    }
  }
}

",
    product_card!()
);

const SEARCH: &str = concat!(
    "query Search($query: String!, $first: Int!, $after: String) {
  search(query: $query, first: $first, after: $after, types: PRODUCT) {
    totalCount
    pageInfo { hasNextPage endCursor }
    nodes { ... on Product { ...ProductCard } }
  }
}

",
    product_card!()
);

const PRODUCT: &str = concat!(
    "query Product($handle: String!) {
  product(handle: $handle) {
    id
    handle
    title
    description
    images(first: 10) { nodes { url altText } }
    options { name optionValues { name } }
    variants(first: 100) {
      nodes {
        id
        title
        availableForSale
        selectedOptions { name value }
        price { amount currencyCode }
        compareAtPrice { amount currencyCode }
      }
    }
  }
  productRecommendations(productHandle: $handle) { ...ProductCard }
}

",
    product_card!()
);

const CART_CREATE: &str = concat!(
    "mutation CartCreate($lines: [CartLineInput!]!) {
  cartCreate(input: { lines: $lines }) {
    cart { ...CartFields }
    userErrors { field message }
  }
}

",
    cart_fields!()
);

const CART_LINES_ADD: &str = concat!(
    "mutation CartLinesAdd($cartId: ID!, $lines: [CartLineInput!]!) {
  cartLinesAdd(cartId: $cartId, lines: $lines) {
    cart { ...CartFields }
    userErrors { field message }
  }
}

",
    cart_fields!()
);

const CART_LINES_UPDATE: &str = concat!(
    "mutation CartLinesUpdate($cartId: ID!, $lines: [CartLineUpdateInput!]!) {
  cartLinesUpdate(cartId: $cartId, lines: $lines) {
    cart { ...CartFields }
    userErrors { field message }
  }
}

",
    cart_fields!()
);
