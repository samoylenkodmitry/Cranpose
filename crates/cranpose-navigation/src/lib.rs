#![deny(missing_docs)]
#![doc = include_str!("../README.md")]
#![expect(non_snake_case, reason = "names follow Compose's casing")]

mod browser;
mod controller;
mod deep_links;
mod host;
mod saved_scroll;

pub use browser::BindBrowserHistory;
pub use controller::{
    NavBackStackEntry, NavController, NavOptions, rememberNavController,
    rememberSaveableNavController,
};
pub use deep_links::BindDeepLinks;
pub use host::{NavHost, NavHostWith, NavHostWithDialogs};
pub use saved_scroll::{
    rememberSaveableLazyGridState, rememberSaveableLazyListState,
    rememberSaveableLazyStaggeredGridState, rememberSaveablePagerState,
    rememberSaveableScrollState,
};
