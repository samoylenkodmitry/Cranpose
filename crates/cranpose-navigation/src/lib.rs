//! Navigation for Cranpose: a typed back stack, a `NavHost` that shows its top
//! screen, and a view model store per screen.
//!
//! | Android | Cranpose |
//! |---|---|
//! | `rememberNavController()` + `startDestination` | [`rememberNavController`], a `Copy` [`NavController`] |
//! | `NavHost(navController) { composable<Route> { } }` | [`NavHost`] with a `match` on the route |
//! | `NavHost(enterTransition, exitTransition)` | [`NavHostWith`] |
//! | `navigate(route)`, `popBackStack()`, `navigateUp()` | [`NavController::navigate`], [`NavController::pop_back_stack`], [`NavController::navigate_up`] |
//! | `navigate(route) { popUpTo(..) { inclusive }; launchSingleTop }` | [`NavController::navigate_with`] and [`NavOptions`] |
//! | `popBackStack(route, inclusive)` | [`NavController::pop_back_stack_to`] |
//! | `currentBackStackEntryAsState()`, `currentBackStack` | [`NavController::current_route`], [`NavController::back_stack`] |
//! | `viewModel()` scoped to a `NavBackStackEntry` | `cranpose_coroflow::viewModel` inside a screen |
//!
//! A route is any `Clone + PartialEq` value, usually an enum whose variants
//! carry the screen's arguments.

#![expect(non_snake_case)]

mod controller;
mod host;

pub use controller::{NavController, NavOptions, rememberNavController};
pub use host::{NavHost, NavHostWith};
