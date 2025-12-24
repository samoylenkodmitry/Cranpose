//! Lazy layout system for Compose-RS.
//!
//! This module provides virtualized layout components that only compose
//! and render currently visible items, enabling efficient handling of
//! large datasets.
//!
//! # Architecture
//!
//! Based on Jetpack Compose's lazy layout system:
//! - [`LazyListState`] - State holder for scroll position (JC: `LazyListState`)
//! - [`LazyLayoutItemProvider`] - Item factory trait (JC: `LazyLayoutItemProvider`)
//! - [`LazyListScope`] - DSL builder (JC: `LazyListScope`)
//! - [`measure_lazy_list`] - Virtualized measurement (JC: `measureLazyList`)
//!
//! # Example
//!
//! ```rust,ignore
//! lazy_column(modifier, state, |scope| {
//!     scope.items(&my_data, |item| {
//!         Text::new(item.name.clone())
//!     });
//! });
//! ```

mod item_provider;
mod lazy_list_layout_info;
mod lazy_list_measure;
mod lazy_list_measured_item;
mod lazy_list_scope;
mod lazy_list_state;
mod nearest_range;
mod prefetch;

pub use item_provider::*;
pub use lazy_list_measure::*;
pub use lazy_list_measured_item::*;
pub use lazy_list_scope::*;
pub use lazy_list_state::*;
pub use nearest_range::*;
pub use prefetch::*;
