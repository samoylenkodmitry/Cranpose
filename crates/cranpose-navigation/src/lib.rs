#![doc = include_str!("../README.md")]
#![expect(non_snake_case)]

mod controller;
mod host;

pub use controller::{NavController, NavOptions, rememberNavController};
pub use host::{NavHost, NavHostWith};
