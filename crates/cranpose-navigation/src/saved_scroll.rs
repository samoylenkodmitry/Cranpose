//! Scroll positions kept like [`rememberSaveable`] values, so a list, grid,
//! pager or scrolling column comes back where the user left it: after a
//! screen that covered it is popped, and after the process restarts.
//!
//! Each state saves under its `key` in the [`SaveableScope`] where it is
//! composed; a navigation entry has its own scope, so two visits of one
//! screen keep their positions apart. A position is saved at each save
//! boundary and when the scroller leaves the composition, and restored when
//! it is composed again.
//!
//! [`rememberSaveable`]: cranpose_services::rememberSaveable

use cranpose_core::remember;
use cranpose_services::{SaveableRegistration, SaveableScope};
use cranpose_ui::{
    LazyGridState, LazyListState, LazyStaggeredGridState, PagerState, ScrollState, composable,
};

/// The stored form of a first visible item and its offset: `"index offset"`.
fn format_position((index, offset): (usize, f32)) -> String {
    format!("{index} {offset}")
}

fn parse_position(stored: &str) -> Option<(usize, f32)> {
    let (index, offset) = stored.split_once(' ')?;
    Some((index.parse().ok()?, offset.parse().ok()?))
}

/// Remembers the state `create` makes from the position saved under `key`,
/// and saves the state's position under `key`. The state and its
/// registration share one slot, so the scroller's last position is read
/// before its states are released.
fn remember_saved<S, P>(
    key: &'static str,
    parse: fn(&str) -> Option<P>,
    format: fn(P) -> String,
    create: impl FnOnce(Option<P>) -> S,
    read: fn(&S) -> Option<P>,
) -> S
where
    S: Clone + 'static,
    P: 'static,
{
    remember(move || {
        let scope = SaveableScope::current();
        let state = create(scope.restore(key).as_deref().and_then(parse));
        let saved = state.clone();
        let registration: SaveableRegistration =
            scope.register(key, move || read(&saved).map(format));
        (state, registration)
    })
    .with(|(state, _)| state.clone())
}

/// A [`LazyListState`] for a `LazyColumn` or `LazyRow` whose position is
/// saved under `key`.
#[composable]
pub fn rememberSaveableLazyListState(key: &'static str) -> LazyListState {
    remember_saved(
        key,
        parse_position,
        format_position,
        |restored| {
            let (index, offset) = restored.unwrap_or_default();
            LazyListState::new(index, offset)
        },
        LazyListState::try_position,
    )
}

/// A [`LazyGridState`] whose first visible item is saved under `key`. It
/// comes back to the same item when the grid has another number of columns,
/// as after a rotation.
#[composable]
pub fn rememberSaveableLazyGridState(key: &'static str) -> LazyGridState {
    remember_saved(
        key,
        parse_position,
        format_position,
        |restored| {
            let state = LazyGridState::new();
            if let Some((index, offset)) = restored {
                state.scroll_to_item(index, offset);
            }
            state
        },
        LazyGridState::try_position,
    )
}

/// A [`LazyStaggeredGridState`] whose position is saved under `key`.
#[composable]
pub fn rememberSaveableLazyStaggeredGridState(key: &'static str) -> LazyStaggeredGridState {
    remember_saved(
        key,
        parse_position,
        format_position,
        |restored| {
            let (index, offset) = restored.unwrap_or_default();
            LazyStaggeredGridState::new(index, offset)
        },
        LazyStaggeredGridState::try_position,
    )
}

/// A [`PagerState`] whose page is saved under `key`; it starts on
/// `initial_page` when nothing was saved.
#[composable]
pub fn rememberSaveablePagerState(key: &'static str, initial_page: usize) -> PagerState {
    remember_saved(
        key,
        |stored| stored.parse().ok(),
        |page: usize| page.to_string(),
        |restored| PagerState::new(restored.unwrap_or(initial_page)),
        PagerState::try_first_page,
    )
}

/// A [`ScrollState`] for `vertical_scroll` or `horizontal_scroll` whose
/// position, in logical pixels, is saved under `key`.
#[composable]
pub fn rememberSaveableScrollState(key: &'static str) -> ScrollState {
    remember_saved(
        key,
        |stored| stored.parse().ok(),
        |value: f32| value.to_string(),
        |restored| ScrollState::new(restored.unwrap_or_default()),
        ScrollState::try_value,
    )
}
