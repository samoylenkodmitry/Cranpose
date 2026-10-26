//! Pagers: Compose's `HorizontalPager` and `VerticalPager`. The pages fill
//! the pager or take a fixed length, compose lazily as a lazy row or
//! column's items do, and a drag or a fling the user lets go of comes to
//! rest at the start of a page, as far as [`PagerFling`] lets it.

use std::rc::Rc;

use cranpose_core::{MutableState, mutableStateOf, remember};
use cranpose_foundation::lazy::{
    LazyItems, LazyListIntervalContent, LazyListScope, LazyListState, LazySettlePolicy,
};
use cranpose_ui_layout::{Axis, LinearArrangement};

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

/// Where in the pager a page comes to rest — Compose's `SnapPosition`. It
/// matters for a page shorter than the space between the content paddings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapPosition {
    /// At the start, after the start padding.
    #[default]
    Start,
    /// In the middle of the space between the paddings, with room for the
    /// neighbouring pages on both sides.
    Center,
    /// At the end, before the end padding.
    End,
}

impl SnapPosition {
    /// How much of the room beside a resting page lies before it.
    fn fraction(self) -> f32 {
        match self {
            Self::Start => 0.0,
            Self::Center => 0.5,
            Self::End => 1.0,
        }
    }
}

/// How a pager comes to rest after the user lets go — the options of
/// Compose's `PagerDefaults.flingBehavior`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagerFling {
    /// The most pages one fling turns, at least one — Compose's
    /// `PagerSnapDistance.atMost(pages)`. A fling turns as many pages as its
    /// speed carries the pager, up to this many.
    pub max_pages: usize,
    /// How far into the next page a slow release must have carried the
    /// pager to turn to it, as a fraction of a page — Compose's
    /// `snapPositionalThreshold`.
    pub positional_threshold: f32,
}

impl Default for PagerFling {
    fn default() -> Self {
        Self {
            max_pages: 1,
            positional_threshold: 0.5,
        }
    }
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
    /// How a release comes to rest: Compose's `flingBehavior`.
    pub fling: PagerFling,
    /// Where a page comes to rest, the first and the last page too:
    /// Compose's `snapPosition`.
    pub snap_position: SnapPosition,
}

impl Default for PagerSpec {
    fn default() -> Self {
        Self {
            page_size: PageSize::Fill,
            page_spacing: 0.0,
            content_padding: PaddingValues::default(),
            beyond_viewport_page_count: 2,
            fling: PagerFling::default(),
            snap_position: SnapPosition::Start,
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
        let pages = self.pages_at(
            self.list.first_visible_item_index(),
            self.list.first_visible_item_scroll_offset(),
        );
        pages - pages.round()
    }

    /// Shows `page` at once.
    pub fn scroll_to_page(&self, page: usize) {
        self.list.scroll_to_item(page, self.rest_offset(page));
    }

    /// Scrolls to `page` with a spring — Compose's `animateScrollToPage`. A
    /// press on the pager stops it, and letting go settles on a page.
    pub fn animate_scroll_to_page(&self, page: usize) {
        self.target.set(Some(page));
        self.list
            .animate_scroll_to_item(page, self.rest_offset(page));
    }

    /// The scroll offset within `page` at which it rests: a page after the
    /// first rests after the start padding, which the list counts as the
    /// first page's.
    fn rest_offset(&self, page: usize) -> f32 {
        match self.geometry() {
            Some(geometry) if page > 0 => -geometry.before,
            _ => 0.0,
        }
    }

    /// Places the pager's first page, which it was created on, after the
    /// `before` padding of its first layout, as a later page rests.
    fn place_initial_page(&self, before: f32) {
        let untouched = self
            .list
            .with_layout_info(|info| info.total_items_count == 0)
            .unwrap_or(false);
        let Some((page, offset)) = self.list.try_position() else {
            return;
        };
        if untouched && page > 0 && offset == 0.0 && before > 0.0 {
            self.list.scroll_to_item(page, -before);
        }
    }

    /// The state of the lazy row or column the pages sit in, for a scroll
    /// indicator.
    pub fn lazy_list_state(&self) -> LazyListState {
        self.list
    }

    /// The page nearest to where it rests when page `first` is the first in
    /// view, scrolled by `offset`.
    fn page_at(&self, first: usize, offset: f32) -> usize {
        self.pages_at(first, offset).round().max(0.0) as usize
    }

    /// Where the pager stands in pages: page `p` rests at `p`, and 2.25 is
    /// a quarter of the way from page 2 to page 3.
    fn pages_at(&self, first: usize, offset: f32) -> f32 {
        self.geometry()
            .map_or(first as f32, |geometry| geometry.pages_at(first, offset))
    }

    /// The pages' geometry from the pager's last layout.
    fn geometry(&self) -> Option<PageGeometry> {
        self.list
            .with_layout_info(|info| {
                let page = info.visible_items_info.first()?;
                let extent = page.size + info.main_axis_item_spacing;
                (extent > 0.0).then_some(PageGeometry {
                    extent,
                    before: info.before_content_padding,
                })
            })
            .flatten()
    }
}

/// Where a pager's pages lie along its axis.
#[derive(Clone, Copy)]
struct PageGeometry {
    /// How far the start of a page lies from the start of the next one.
    extent: f32,
    /// The padding before the first page, which the list counts as part of
    /// the first page.
    before: f32,
}

impl PageGeometry {
    /// How far the list has scrolled when page `first` is the first in view
    /// at `offset`. Page `p` rests at `p` extents.
    fn scrolled(self, first: usize, offset: f32) -> f32 {
        let start = if first == 0 {
            0.0
        } else {
            self.before + first as f32 * self.extent
        };
        start + offset
    }

    fn pages_at(self, first: usize, offset: f32) -> f32 {
        self.scrolled(first, offset) / self.extent
    }
}

/// Where a scroll the user lets go of comes to rest: a fling turns toward
/// its direction as many pages as it would carry the pager, from one to
/// `fling.max_pages`; a slow release goes to the page it would stop in, or
/// the next one past `fling.positional_threshold` of it.
fn page_settle(state: PagerState, fling: PagerFling) -> LazySettlePolicy {
    Rc::new(move |proposed, velocity| {
        let Some(geometry) = state.geometry() else {
            return proposed;
        };
        // In pages: where the pager stands and where the release would stop.
        let first = state.list.first_visible_item_index_non_reactive();
        let now = geometry.pages_at(
            first,
            state.list.first_visible_item_scroll_offset_non_reactive(),
        );
        let rest = geometry.pages_at(first, proposed);
        let target = if velocity.abs() >= PAGE_FLING_VELOCITY {
            let most = fling.max_pages.max(1) as f32;
            if velocity > 0.0 {
                rest.ceil().clamp(now.floor() + 1.0, now.floor() + most)
            } else {
                rest.floor().clamp(now.ceil() - most, now.ceil() - 1.0)
            }
        } else {
            let whole = rest.floor();
            if rest - whole > fling.positional_threshold {
                whole + 1.0
            } else {
                whole
            }
        };
        let last = state
            .list
            .with_layout_info(|info| info.total_items_count.saturating_sub(1))
            .unwrap_or_default() as f32;
        let target = target.clamp(0.0, last);
        state.target.set(Some(target as usize));
        target * geometry.extent - geometry.scrolled(first, 0.0)
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
        Axis::Horizontal,
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
        Axis::Vertical,
        Rc::new(page_content),
    );
}

#[composable(no_skip)]
fn Pager(
    modifier: Modifier,
    state: PagerState,
    spec: PagerSpec,
    page_count: usize,
    axis: Axis,
    page_content: Rc<dyn Fn(usize)>,
) {
    cranpose_core::DisposableEffect((state, spec.fling), move |scope| {
        state
            .list
            .set_settle_policy(Some(page_settle(state, spec.fling)));
        scope.on_dispose(move || state.list.set_settle_policy(None))
    });
    BoxWithConstraints(modifier, move |scope| {
        let constraints = scope.constraints();
        let mut padding = spec.content_padding;
        let width = constraints.max_width - padding.start - padding.end;
        let height = constraints.max_height - padding.top - padding.bottom;
        let (page_width, page_height) = match (spec.page_size, axis) {
            (PageSize::Fill, _) => (width, height),
            (PageSize::Fixed(length), Axis::Horizontal) => (length, height),
            (PageSize::Fixed(length), Axis::Vertical) => (width, length),
        };
        // A resting page sits at the scroll offset of its index, as at the
        // start; padding moves where that is along the axis.
        let room = if axis.is_horizontal() {
            width - page_width
        } else {
            height - page_height
        }
        .max(0.0);
        let before = room * spec.snap_position.fraction();
        if axis.is_horizontal() {
            padding.start += before;
            padding.end += room - before;
        } else {
            padding.top += before;
            padding.bottom += room - before;
        }
        let leading = if axis.is_horizontal() {
            padding.start
        } else {
            padding.top
        };
        remember(|| state.place_initial_page(leading));
        let (width, height) = (page_width, page_height);
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
        if axis.is_horizontal() {
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
