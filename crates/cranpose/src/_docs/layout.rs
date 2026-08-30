//! # Layout
//!
//! A composable takes its modifier first, its spec second, and its content
//! last. `Text` is the exception: its value comes first.
//!
//! ```no_run
//! #![allow(non_snake_case)]
//! use cranpose::prelude::*;
//! # use cranpose::{
//! #     __branch_group_scope_deferred, branch_location_key,
//! #     cached_branch_location_key, cached_composable_definition_key,
//! #     caller_location_key,
//! #     composable_definition_key, composable_identity_key, debug_label_current_scope,
//! #     location_key,
//! #     with_current_composer, CallbackHolder, Composer, Key, ParamState, ReturnSlot,
//! # };
//!
//! #[composable]
//! fn Card() {
//!     Column(
//!         Modifier::empty()
//!             .fill_max_width()
//!             .padding(16.0)
//!             .background(Color(0.1, 0.12, 0.18, 1.0))
//!             .rounded_corners(12.0),
//!         ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
//!         move || {
//!             Text("Title", Modifier::empty(), TextStyle::default());
//!             Text("Body", Modifier::empty(), TextStyle::default());
//!         },
//!     );
//! }
//!
//! fn main() {}
//! ```
//!
//! A `Modifier` is an ordered chain and the order is the meaning:
//! `.padding(8.0).background(c)` paints the background inside the padding,
//! `.background(c).padding(8.0)` paints it outside.
//!
//! ## Lists
//!
//! A `for` loop composes every item. Anything long belongs in `LazyColumn`,
//! which composes only what is on screen and takes its state positionally:
//!
//! ```no_run
//! #![allow(non_snake_case)]
//! use cranpose::prelude::*;
//! # use cranpose::{
//! #     __branch_group_scope_deferred, branch_location_key,
//! #     cached_branch_location_key, cached_composable_definition_key,
//! #     caller_location_key,
//! #     composable_definition_key, composable_identity_key, debug_label_current_scope,
//! #     location_key,
//! #     with_current_composer, CallbackHolder, Composer, Key, ParamState, ReturnSlot,
//! # };
//!
//! #[composable]
//! fn Rows(count: usize) {
//!     let state = rememberLazyListState();
//!
//!     LazyColumn(
//!         Modifier::empty().fill_max_size(),
//!         state,
//!         LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
//!         move |scope| {
//!             scope.items(LazyItems::new(count), move |index| {
//!                 Text(
//!                     format!("Row {index}"),
//!                     Modifier::empty(),
//!                     TextStyle::default(),
//!                 );
//!             });
//!         },
//!     );
//! }
//!
//! fn main() {}
//! ```
//!
//! Give `LazyItems::content_type` a real grouping when items differ
//! structurally: it is what lets the runtime reuse a subtree between items of
//! the same shape instead of building a new one.
