//! Pagers: Compose's `HorizontalPager` and `VerticalPager`. Each page fills
//! the pager, the pages compose lazily as a lazy row or column does, and a
//! drag or a fling the user lets go of comes to rest on a page.

use std::rc::Rc;

use cranpose_foundation::lazy::{
    LazyItems, LazyListIntervalContent, LazyListScope, LazyListState, LazySettlePolicy,
    rememberLazyListStateWithPosition,
};

use crate::{
    composable,
    modifier::Modifier,
    widgets::{
        Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, LazyColumn, LazyColumnSpec,
        LazyRow, LazyRowSpec,
    },
};

/// A release faster than this, in logical pixels per second, turns the page
/// toward the fling however little the drag moved it.
const PAGE_FLING_VELOCITY: f32 = 400.0;

/// Where a pager is — Compose's `PagerState`. It is `Copy`, so it moves into
/// any number of closures.
#[derive(Clone, Copy, PartialEq)]
pub struct PagerState {
    list: LazyListState,
}

/// A [`PagerState`] that starts on `initial_page`.
#[composable]
pub fn rememberPagerState(initial_page: usize) -> PagerState {
    PagerState {
        list: rememberLazyListStateWithPosition(initial_page, 0.0),
    }
}

impl PagerState {
    /// The page that fills most of the pager. A composable that reads it
    /// recomposes while the pager scrolls.
    pub fn current_page(&self) -> usize {
        let first = self.list.first_visible_item_index();
        let offset = self.list.first_visible_item_scroll_offset();
        if offset > self.page_size() / 2.0 {
            first + 1
        } else {
            first
        }
    }

    /// How far the pager stands from [`Self::current_page`], as a fraction of
    /// a page from -0.5 to 0.5 — Compose's `currentPageOffsetFraction`.
    pub fn current_page_offset_fraction(&self) -> f32 {
        let page = self.page_size();
        if page <= 0.0 {
            return 0.0;
        }
        let fraction = self.list.first_visible_item_scroll_offset() / page;
        if fraction > 0.5 {
            fraction - 1.0
        } else {
            fraction
        }
    }

    /// Shows `page` at once.
    pub fn scroll_to_page(&self, page: usize) {
        self.list.scroll_to_item(page, 0.0);
    }

    /// The state of the lazy row or column the pages sit in, for a scroll
    /// indicator.
    pub fn lazy_list_state(&self) -> LazyListState {
        self.list
    }

    fn page_size(&self) -> f32 {
        self.list.layout_info().viewport_size
    }
}

/// Where a scroll the user lets go of comes to rest: the start of the first
/// page it shows or of the page after, the way the release goes when it is a
/// fling, else the one closer to where it would stop.
fn page_settle(state: PagerState) -> LazySettlePolicy {
    Rc::new(move |proposed, velocity| {
        let page = state.page_size();
        if page <= 0.0 {
            return proposed;
        }
        let forward = if velocity.abs() >= PAGE_FLING_VELOCITY {
            velocity > 0.0
        } else {
            proposed > page / 2.0
        };
        if forward { page } else { 0.0 }
    })
}

/// Pages side by side, `page_count` of them, each as wide and high as the
/// pager — Compose's `HorizontalPager`. The pager needs a bounded size.
#[composable(no_skip)]
pub fn HorizontalPager<F>(modifier: Modifier, state: PagerState, page_count: usize, page_content: F)
where
    F: Fn(usize) + 'static,
{
    Pager(modifier, state, page_count, true, Rc::new(page_content));
}

/// Pages stacked from top to bottom — Compose's `VerticalPager`.
#[composable(no_skip)]
pub fn VerticalPager<F>(modifier: Modifier, state: PagerState, page_count: usize, page_content: F)
where
    F: Fn(usize) + 'static,
{
    Pager(modifier, state, page_count, false, Rc::new(page_content));
}

#[composable(no_skip)]
fn Pager(
    modifier: Modifier,
    state: PagerState,
    page_count: usize,
    horizontal: bool,
    page_content: Rc<dyn Fn(usize)>,
) {
    cranpose_core::DisposableEffect(state, move |scope| {
        state.list.set_settle_policy(Some(page_settle(state)));
        scope.on_dispose(move || state.list.set_settle_policy(None))
    });
    BoxWithConstraints(modifier, move |scope| {
        let constraints = scope.constraints();
        let (width, height) = (constraints.max_width, constraints.max_height);
        let content = Rc::clone(&page_content);
        let pages = move |pages: &mut LazyListIntervalContent| {
            pages.items(
                LazyItems::new(page_count).key(|page| page as u64),
                move |page| {
                    let content = Rc::clone(&content);
                    Box(
                        Modifier::empty().size_points(width, height),
                        BoxSpec::default(),
                        move || content(page),
                    );
                },
            );
        };
        let fill = Modifier::empty().fill_max_size();
        if horizontal {
            LazyRow(fill, state.list, LazyRowSpec::default(), pages);
        } else {
            LazyColumn(fill, state.list, LazyColumnSpec::default(), pages);
        }
    });
}
