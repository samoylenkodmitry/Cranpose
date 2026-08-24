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
//! #     debug_label_current_scope, location_key, with_current_composer, CallbackHolder,
//! #     Composer, ParamState, ReturnSlot,
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
//! | [`rememberMutableStateOf`] | Observable state. |
//! | [`rememberUpdatedState`] | A value a long-lived effect should see fresh without restarting. |
//! | [`rememberCoroutineScope`] | A scope for work started from an event handler. |
//! | [`mutableStateOf`] | State owned outside the composition. |
//!
//! Reading state outside a composable subscribes nothing. If a change does not
//! repaint, check that the read happens inside the composable that should react
//! to it.
//!
//! [`remember`]: crate::prelude::remember
//! [`rememberMutableStateOf`]: crate::prelude::rememberMutableStateOf
//! [`rememberUpdatedState`]: crate::prelude::rememberUpdatedState
//! [`rememberCoroutineScope`]: crate::prelude::rememberCoroutineScope
//! [`mutableStateOf`]: crate::prelude::mutableStateOf
