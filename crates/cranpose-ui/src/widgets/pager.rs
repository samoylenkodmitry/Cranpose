//! Pagers: Compose's `HorizontalPager` and `VerticalPager`. The pages fill
//! the pager or take a fixed length, compose lazily as a lazy row or
//! column's items do, and a drag or a fling the user lets go of comes to
//! rest at the start of a page.

use std::rc::Rc;

use cranpose_core::{MutableState, mutableStateOf, remember};
use cranpose_foundation::lazy::{
    LazyItems, LazyListIntervalContent, LazyListScope, LazyListState, LazySettlePolicy,
};
use cranpose_ui_layout::LinearArrangement;

use crate::{
    AnimateScroll, composable,
    modifier::Modifier,
    widgets::{
        Box, BoxSpec, BoxWithConstraints, BoxWithConstraintsScope, LazyColumn, LazyColumnSpec,
        LazyRow, LazyRowSpec, PaddingValues,
    },
};

/// A release faster than this, in logical pixels per second, turns the page
/// toward the fling however little the drag moved it.
const PAGE_FLING_VELOCITY: f32 = 400.0;

/// How long a pager's pages are along its axis — Compose's `PageSize`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageSize {
    /// Each page is as long as the pager.
    Fill,
    /// Each page is this many logical pixels long, so a shorter page shows
    /// the next one beside it.
    Fixed(f32),
}

/// Options for a pager, Compose's pager arguments beside its state and its
/// page count.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagerSpec {
    /// How long each page is along the pager's axis.
    pub page_size: PageSize,
    /// Logical pixels between two pages.
    pub page_spacing: f32,
    /// Compose's `contentPadding`. A page comes to rest after the start
    /// padding, a `PageSize::Fill` page is as long as the pager minus the
    /// padding at both ends, and the neighbouring pages show in the padding.
    pub content_padding: PaddingValues,
    /// Pages composed and laid out beyond the ones in view on each side:
    /// Compose's `beyondViewportPageCount`.
    pub beyond_viewport_page_count: usize,
}

impl Default for PagerSpec {
    fn default() -> Self {
        Self {
            page_size: PageSize::Fill,
            page_spacing: 0.0,
            content_padding: PaddingValues::default(),
            beyond_viewport_page_count: 2,
        }
    }
}

/// Where a pager is — Compose's `PagerState`. It is `Copy`, so it moves into
/// any number of closures.
#[derive(Clone, Copy, PartialEq)]
pub struct PagerState {
    list: LazyListState,
    /// The page the running animation or settle goes to.
    target: MutableState<Option<usize>>,
}

/// A [`PagerState`] that starts on `initial_page`.
#[composable]
pub fn rememberPagerState(initial_page: usize) -> PagerState {
    remember(|| PagerState::new(initial_page)).with(|state| *state)
}

impl PagerState {
    /// A pager's state on `initial_page`, for code that remembers the state
    /// itself. Call it inside a `remember`, as [`LazyListState::new`].
    pub fn new(initial_page: usize) -> Self {
        Self {
            list: LazyListState::new(initial_page, 0.0),
            target: mutableStateOf(None),
        }
    }

    /// The first page in view, or `None` once the slot that held this state
    /// was released.
    pub fn try_first_page(&self) -> Option<usize> {
        self.list.try_position().map(|(page, _)| page)
    }

    /// The page that fills most of the pager. A composable that reads it
    /// recomposes while the pager scrolls.
    pub fn current_page(&self) -> usize {
        self.page_at(
            self.list.first_visible_item_index(),
            self.list.first_visible_item_scroll_offset(),
        )
    }

    /// The page the pager rests on: Compose's `settledPage`. While a drag,
    /// a fling or an animation moves the pager, it stays the page that
    /// scroll began on, so a composable that reads it recomposes when a
    /// scroll starts and when it ends, not on the frames between.
    pub fn settled_page(&self) -> usize {
        let (first, offset) = match self.list.position_before_scroll() {
            Some(start) if self.list.is_scroll_in_progress() => start,
            // At rest the offset changes only with the page, so the page
            // alone subscribes.
            _ => (
                self.list.first_visible_item_index(),
                self.list.first_visible_item_scroll_offset_non_reactive(),
            ),
        };
        self.page_at(first, offset)
    }

    /// Whether a drag, a fling or an animation moves the pager now:
    /// Compose's `isScrollInProgress`.
    pub fn is_scroll_in_progress(&self) -> bool {
        self.list.is_scroll_in_progress()
    }

    /// The page the pager is on its way to: Compose's `targetPage`. While an
    /// animation or the settle after a release moves the pager, it is the
    /// page that motion ends on; during a drag and at rest it is
    /// [`Self::current_page`].
    pub fn target_page(&self) -> usize {
        if self.list.is_scroll_in_progress()
            && !self.list.is_dragged()
            && let Some(page) = self.target.get()
        {
            return page;
        }
        self.current_page()
    }

    /// How far the pager stands from [`Self::current_page`], as a fraction of
    /// a page from -0.5 to 0.5 — Compose's `currentPageOffsetFraction`.
    pub fn current_page_offset_fraction(&self) -> f32 {
        let page = self.page_extent();
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

    /// Scrolls to `page` with a spring — Compose's `animateScrollToPage`. A
    /// press on the pager stops it, and letting go settles on a page.
    pub fn animate_scroll_to_page(&self, page: usize) {
        self.target.set(Some(page));
        self.list.animate_scroll_to_item(page, 0.0);
    }

    /// The state of the lazy row or column the pages sit in, for a scroll
    /// indicator.
    pub fn lazy_list_state(&self) -> LazyListState {
        self.list
    }

    /// The page that fills most of the pager when page `first` is the first
    /// in view, scrolled by `offset`.
    fn page_at(&self, first: usize, offset: f32) -> usize {
        if offset > self.page_extent() / 2.0 {
            first + 1
        } else {
            first
        }
    }

    /// How far the start of a page lies from the start of the next one.
    fn page_extent(&self) -> f32 {
        self.list
            .with_layout_info(|info| {
                info.visible_items_info
                    .first()
                    .map_or(info.viewport_size, |page| {
                        page.size + info.main_axis_item_spacing
                    })
            })
            .unwrap_or_default()
    }
}

/// Where a scroll the user lets go of comes to rest: the start of the first
/// page it shows or of the page after, the way the release goes when it is a
/// fling, else the one closer to where it would stop.
fn page_settle(state: PagerState) -> LazySettlePolicy {
    Rc::new(move |proposed, velocity| {
        let page = state.page_extent();
        if page <= 0.0 {
            return proposed;
        }
        let forward = if velocity.abs() >= PAGE_FLING_VELOCITY {
            velocity > 0.0
        } else {
            proposed > page / 2.0
        };
        let first = state.list.first_visible_item_index_non_reactive();
        state.target.set(Some(first + usize::from(forward)));
        if forward { page } else { 0.0 }
    })
}

/// Pages side by side, `page_count` of them, each as high as the pager
/// and as wide as `spec` says — Compose's `HorizontalPager`. The pager
/// needs a bounded size.
#[composable(no_skip)]
pub fn HorizontalPager<F>(
    modifier: Modifier,
    state: PagerState,
    spec: PagerSpec,
    page_count: usize,
    page_content: F,
) where
    F: Fn(usize) + 'static,
{
    Pager(
        modifier,
        state,
        spec,
        page_count,
        true,
        Rc::new(page_content),
    );
}

/// Pages stacked from top to bottom — Compose's `VerticalPager`.
#[composable(no_skip)]
pub fn VerticalPager<F>(
    modifier: Modifier,
    state: PagerState,
    spec: PagerSpec,
    page_count: usize,
    page_content: F,
) where
    F: Fn(usize) + 'static,
{
    Pager(
        modifier,
        state,
        spec,
        page_count,
        false,
        Rc::new(page_content),
    );
}

#[composable(no_skip)]
fn Pager(
    modifier: Modifier,
    state: PagerState,
    spec: PagerSpec,
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
        let padding = spec.content_padding;
        let width = constraints.max_width - padding.start - padding.end;
        let height = constraints.max_height - padding.top - padding.bottom;
        let (width, height) = match (spec.page_size, horizontal) {
            (PageSize::Fill, _) => (width, height),
            (PageSize::Fixed(length), true) => (length, height),
            (PageSize::Fixed(length), false) => (width, length),
        };
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
        let spacing = LinearArrangement::SpacedBy(spec.page_spacing);
        if horizontal {
            let row = LazyRowSpec {
                horizontal_arrangement: spacing,
                content_padding: padding,
                beyond_bounds_item_count: spec.beyond_viewport_page_count,
                ..LazyRowSpec::default()
            };
            LazyRow(fill, state.list, row, pages);
        } else {
            let column = LazyColumnSpec {
                vertical_arrangement: spacing,
                content_padding: padding,
                beyond_bounds_item_count: spec.beyond_viewport_page_count,
                ..LazyColumnSpec::default()
            };
            LazyColumn(fill, state.list, column, pages);
        }
    });
}
