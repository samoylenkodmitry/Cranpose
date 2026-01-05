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
//! use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
//! use compose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
//!
//! let state = remember_lazy_list_state();
//! LazyColumn(Modifier::empty(), state, LazyColumnSpec::default(), |scope| {
//!     scope.items(100, None::<fn(usize)->u64>, None::<fn(usize)->u64>, |i| {
//!         Text(format!("Item {}", i), Modifier::empty());
//!     });
//! });
//! ```
//!
//! For convenience with slices, use the [`LazyListScopeExt`] extension methods:
//!
//! ```rust,ignore
//! use compose_ui::widgets::{LazyColumn, LazyColumnSpec};
//! use compose_foundation::lazy::{remember_lazy_list_state, LazyListScopeExt};
//!
//! let state = remember_lazy_list_state();
//! LazyColumn(Modifier::empty(), state, LazyColumnSpec::default(), |scope| {
//!     scope.items_slice(&my_data, |item| {
//!         Text(item.name.clone(), Modifier::empty());
//!     });
//! });
//! ```

mod bounds_adjuster;
mod item_measurer;
mod item_provider;
mod lazy_list_layout_info;
mod lazy_list_measure;
mod lazy_list_measured_item;
mod lazy_list_scope;
mod lazy_list_state;
mod nearest_range;
mod prefetch;
mod scroll_position_resolver;
mod viewport;

pub use item_provider::*;
pub use lazy_list_measure::*;
pub use lazy_list_measured_item::*;
pub use lazy_list_scope::*;
pub use lazy_list_state::*;
pub use nearest_range::*;
pub use prefetch::*;
