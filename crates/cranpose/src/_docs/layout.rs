//! # Layout
//!
//! Layout containers accept a modifier, a spec and a content closure. `Text`
//! accepts its text value before the modifier and style.
//!
//! ```no_run
//! use cranpose::*;
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
//! Modifier order controls the area each operation affects:
//! `.padding(8.0).background(c)` paints the background inside the padding,
//! `.background(c).padding(8.0)` extends the background across the inner space.
//!
//! ## Lists
//!
//! A `for` loop composes every item. `LazyColumn` composes the visible rows
//! and retains list state through `rememberLazyListState`:
//!
//! ```no_run
//! use cranpose::*;
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
//! Set `LazyItems::content_type` for distinct row shapes. The runtime can reuse
//! a subtree for rows with the same content type. Use stable keys when row
//! identity must survive insertions, removals or a new order.
