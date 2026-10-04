//! # State
//!
//! [`rememberMutableStateOf`] retains observable state across recomposition.
//! A composable subscribes to state through a read. A later write invalidates
//! the subscribed scope. This counter updates after a button click:
//!
//! ```no_run
//! use cranpose::*;
//!
//! #[composable]
//! fn Counter() {
//!     let count = rememberMutableStateOf(|| 0i32);
//!
//!     Button(
//!         Modifier::empty().padding(10.0),
//!         ButtonSpec::default(),
//!         move || count.set(count.get() + 1),
//!         move || {
//!             Text(
//!                 format!("Count: {}", count.get()),
//!                 Modifier::empty(),
//!                 TextStyle::default(),
//!             );
//!         },
//!     );
//! }
//!
//! fn main() {}
//! ```
//!
//! State handles are `Copy`. Move a handle into each callback to share the value.
//!
//! | Call | Purpose |
//! | --- | --- |
//! | [`remember`] | A value retained for the composition lifetime. |
//! | [`rememberKeyed`] | A value recomputed only when its key changes. |
//! | [`rememberMutableStateOf`] | Observable state. |
//! | [`rememberUpdatedState`] | A current callback or value for a long-lived effect. |
//! | [`rememberCoroutineScope`] | A scope for work started from an event handler. |
//! | [`mutableStateOf`] | State owned outside the composition. |
//!
//! A state read inside `draw_behind` or the lazy `graphics_layer` closure
//! subscribes the node's visual phase. Updates then invalidate the draw phase.
//! A layout read subscribes the layout phase; a composition read subscribes the
//! composition scope. Place each read in the phase which needs the value.
//!
//! [`remember`]: crate::prelude::remember
//! [`rememberKeyed`]: crate::prelude::rememberKeyed
//! [`rememberMutableStateOf`]: crate::prelude::rememberMutableStateOf
//! [`rememberUpdatedState`]: crate::prelude::rememberUpdatedState
//! [`rememberCoroutineScope`]: crate::prelude::rememberCoroutineScope
//! [`mutableStateOf`]: crate::prelude::mutableStateOf
