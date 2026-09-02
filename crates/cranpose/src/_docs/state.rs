//! # State
//!
//! [`rememberMutableStateOf`] creates state that survives recomposition.
//! Reading it inside a composable subscribes that composable to it, so a write
//! recomposes exactly the readers and nothing else.
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
//! State handles are `Copy`. Move them into closures directly; cloning them is
//! never necessary.
//!
//! | Call | Use it for |
//! | --- | --- |
//! | [`remember`] | A value computed once and not observed for changes. |
//! | [`rememberKeyed`] | A value recomputed only when its key changes. |
//! | [`rememberMutableStateOf`] | Observable state. |
//! | [`rememberUpdatedState`] | A value a long-lived effect should see fresh without restarting. |
//! | [`rememberCoroutineScope`] | A scope for work started from an event handler. |
//! | [`mutableStateOf`] | State owned outside the composition. |
//!
//! Reading state inside `draw_behind` or the lazy `graphics_layer` closure
//! subscribes only that node's visual phase. Animation values read there redraw
//! without recomposing or relaying out the composable. Reads outside an observed
//! composition, layout, or draw phase subscribe nothing.
//!
//! [`remember`]: crate::prelude::remember
//! [`rememberKeyed`]: crate::prelude::rememberKeyed
//! [`rememberMutableStateOf`]: crate::prelude::rememberMutableStateOf
//! [`rememberUpdatedState`]: crate::prelude::rememberUpdatedState
//! [`rememberCoroutineScope`]: crate::prelude::rememberCoroutineScope
//! [`mutableStateOf`]: crate::prelude::mutableStateOf
