//! Pull to refresh over a scrolling list, as Compose's `PullToRefreshBox`.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{
    Animatable, AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
    rememberInfiniteTransition,
};
use cranpose_core::{RuntimeHandle, remember, with_current_composer};
use cranpose_foundation::SemanticsCustomAction;
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, Modifier, NestedScroll, NestedScrollConnection, NestedScrollSource, Point,
    ScrollAxis, Size,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Stroke, StrokeCap};

use crate::{motion::LiquidMotion, theme::liquid_colors};

/// How far the indicator travels before a release refreshes, in dp.
const THRESHOLD: f32 = 72.0;
/// The farthest the indicator follows a pull.
const MAX_PULL: f32 = THRESHOLD * 1.6;
/// A pull moves the indicator half as far as the finger.
const RESISTANCE: f32 = 0.5;
const INDICATOR_SIZE: f32 = 36.0;
const RING_RADIUS: f32 = 10.0;
const RING_WIDTH: f32 = 2.5;

/// What the indicator shows and the pull the connection tracks.
struct Pull {
    /// The indicator's travel, drawn from the animatable.
    travel: RefCell<Animatable<f32>>,
    /// The travel the current drag reached, `0.0` between drags.
    pulled: Cell<f32>,
    refreshing: Cell<bool>,
    on_refresh: RefCell<Rc<dyn Fn()>>,
}

impl Pull {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            travel: RefCell::new(Animatable::new(0.0, runtime)),
            pulled: Cell::new(0.0),
            refreshing: Cell::new(false),
            on_refresh: RefCell::new(Rc::new(|| {})),
        }
    }

    fn pull_to(&self, pulled: f32) {
        self.pulled.set(pulled);
        self.travel.borrow_mut().snapTo(pulled);
    }

    fn settle(&self) {
        let rest = if self.refreshing.get() {
            THRESHOLD
        } else {
            0.0
        };
        self.travel
            .borrow_mut()
            .animateTo(rest, LiquidMotion::snappy());
    }

    fn travel(&self) -> f32 {
        self.travel.borrow().state().value()
    }
}

/// A list at its top hands the rest of a downward drag to the pull; an
/// upward drag first takes the pull back; the release refreshes past the
/// threshold.
struct PullConnection(Rc<Pull>);

impl NestedScrollConnection for PullConnection {
    fn on_pre_scroll(&self, axis: ScrollAxis, available: f32, source: NestedScrollSource) -> f32 {
        let pull = &self.0;
        let pulled = pull.pulled.get();
        if axis != ScrollAxis::Vertical || source != NestedScrollSource::Drag || pulled <= 0.0 {
            return 0.0;
        }
        if available >= 0.0 {
            return 0.0;
        }
        let taken = (available * RESISTANCE).max(-pulled);
        pull.pull_to(pulled + taken);
        taken / RESISTANCE
    }

    fn on_post_scroll(
        &self,
        axis: ScrollAxis,
        _consumed: f32,
        available: f32,
        source: NestedScrollSource,
    ) -> f32 {
        let pull = &self.0;
        if axis != ScrollAxis::Vertical
            || source != NestedScrollSource::Drag
            || available <= 0.0
            || pull.refreshing.get()
        {
            return 0.0;
        }
        pull.pull_to((pull.pulled.get() + available * RESISTANCE).min(MAX_PULL));
        available
    }

    fn on_pre_fling(&self, axis: ScrollAxis, velocity: f32) -> f32 {
        let pull = &self.0;
        let pulled = pull.pulled.get();
        if axis != ScrollAxis::Vertical || pulled <= 0.0 {
            return 0.0;
        }
        pull.pulled.set(0.0);
        if pulled >= THRESHOLD && !pull.refreshing.get() {
            pull.refreshing.set(true);
            let on_refresh = Rc::clone(&pull.on_refresh.borrow());
            on_refresh();
        }
        pull.settle();
        velocity
    }
}

/// Wraps a scrolling list that refreshes when pulled down from its top.
/// A release past the threshold calls `on_refresh`; the app sets
/// `refreshing` while it loads and the indicator spins, then clears it.
/// Screen readers cannot pull, so the list offers them an action named
/// `refresh_label` ("Refresh", in the app's language) that refreshes it.
///
/// Compose's `PullToRefreshBox(isRefreshing, onRefresh)`.
#[composable]
pub fn LiquidPullToRefreshBox(
    modifier: Modifier,
    refreshing: bool,
    refresh_label: impl Into<String>,
    on_refresh: impl Fn() + 'static,
    content: impl FnMut() + 'static,
) {
    let pull = with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        composer
            .remember(move || Rc::new(Pull::new(runtime)))
            .with(Rc::clone)
    });
    let on_refresh: Rc<dyn Fn()> = Rc::new(on_refresh);
    *pull.on_refresh.borrow_mut() = Rc::clone(&on_refresh);
    if pull.refreshing.replace(refreshing) != refreshing && pull.pulled.get() <= 0.0 {
        pull.settle();
    }
    let connection = remember({
        let pull = Rc::clone(&pull);
        move || Rc::new(PullConnection(pull)) as Rc<dyn NestedScrollConnection>
    })
    .with(Rc::clone);
    let action = SemanticsCustomAction::new(refresh_label.into(), move || on_refresh());
    let container = modifier
        .clip_to_bounds()
        .semantics(move |config| config.custom_actions.push(action.clone()));
    let mut content = content;
    Box(container, BoxSpec::default(), move || {
        NestedScroll(Rc::clone(&connection), &mut content);
        PullIndicator(PullHandle(Rc::clone(&pull)), refreshing);
    });
}

/// The ring that slides down with the pull, fills as it nears the
/// threshold, and spins while the list refreshes.
/// The pull, compared by identity: one box keeps one pull.
#[derive(Clone)]
struct PullHandle(Rc<Pull>);

impl PartialEq for PullHandle {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[composable]
fn PullIndicator(pull: PullHandle, refreshing: bool) {
    let colors = liquid_colors();
    let (surface, accent) = (colors.surface, colors.accent);
    let spin = rememberInfiniteTransition("pull-refresh").animateFloat(
        0.0,
        360.0,
        infiniteRepeatable(
            AnimationSpec::tween(900, Easing::LinearEasing),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "pull-refresh-spin",
    );
    let modifier = Modifier::empty()
        .size(Size::new(INDICATOR_SIZE, INDICATOR_SIZE))
        .draw_behind(move |scope| {
            let travel = pull.0.travel();
            if travel <= 0.5 {
                return;
            }
            let top = travel - INDICATOR_SIZE;
            let center = Point::new(INDICATOR_SIZE * 0.5, top + INDICATOR_SIZE * 0.5);
            scope.draw_circle(Brush::solid(surface), center, INDICATOR_SIZE * 0.5);
            // Degrees, which draw_arc takes in radians.
            let (start, sweep) = if refreshing {
                (spin.value(), 270.0)
            } else {
                (-90.0, 300.0 * (travel / THRESHOLD).min(1.0))
            };
            scope.draw_arc(
                Brush::solid(accent),
                center,
                RING_RADIUS,
                f32::to_radians(start),
                f32::to_radians(sweep),
                Stroke::new(RING_WIDTH).with_cap(StrokeCap::Round),
            );
        });
    Box(
        Modifier::empty().fill_max_width(),
        BoxSpec::default().content_alignment(Alignment::TOP_CENTER),
        move || {
            Box(modifier.clone(), BoxSpec::default(), || {});
        },
    );
}
