#![doc = include_str!("../README.md")]
#![expect(non_snake_case)]

mod browser;
mod controller;
mod host;

pub use browser::BindBrowserHistory;
pub use controller::{
    NavController, NavOptions, rememberNavController, rememberSaveableNavController,
};
pub use host::{NavHost, NavHostWith};
