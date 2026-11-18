//! Pull to refresh over a scrolling list, as Compose's `PullToRefreshBox`.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::Animatable;
use cranpose_core::{RuntimeHandle, State, remember, with_current_composer};
use cranpose_foundation::SemanticsCustomAction;
use cranpose_macros::composable;
use cranpose_ui::{
    Axis, DrawScope, Modifier, NestedScroll, NestedScrollConnection, NestedScrollSource, Point,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::Color;

use super::{LiquidActivityIndicatorSize, activity_indicator::rememberSpinnerHead};
use crate::{motion::LiquidMotion, theme::liquid_colors};

/// How far the indicator travels before a release refreshes, in dp.
const THRESHOLD: f32 = 72.0;
/// The farthest the indicator follows a pull.
const MAX_PULL: f32 = THRESHOLD * 1.6;
/// A pull moves the indicator half as far as the finger.
const RESISTANCE: f32 = 0.5;
// UIRefreshControl, measured on the simulator at 3x: while the list
// refreshes it rests 60 points down, and the spinner, the medium activity
// indicator's wheel at 1.45 times its size, turns in the middle of the gap.
const REFRESHING_TRAVEL: f32 = 60.0;
const SPINNER_SCALE: f32 = 1.45;
// Spokes the wheel has; a pull shows them one by one up to the threshold.
const SPOKES: f32 = 8.0;

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
            REFRESHING_TRAVEL
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
    fn on_pre_scroll(&self, axis: Axis, available: f32, source: NestedScrollSource) -> f32 {
        let pull = &self.0;
        let pulled = pull.pulled.get();
        if axis != Axis::Vertical || source != NestedScrollSource::Drag || pulled <= 0.0 {
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
        axis: Axis,
        _consumed: f32,
        available: f32,
        source: NestedScrollSource,
    ) -> f32 {
        let pull = &self.0;
        if axis != Axis::Vertical
            || source != NestedScrollSource::Drag
            || available <= 0.0
            || pull.refreshing.get()
        {
            return 0.0;
        }
        pull.pull_to((pull.pulled.get() + available * RESISTANCE).min(MAX_PULL));
        available
    }

    fn on_pre_fling(&self, axis: Axis, velocity: f32) -> f32 {
        let pull = &self.0;
        let pulled = pull.pulled.get();
        if axis != Axis::Vertical || pulled <= 0.0 {
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
    content: impl Fn() + 'static,
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
    // The spinner draws behind the list, which moves down with the pull and
    // leaves it a gap. Only a refreshing list turns its wheel, so an idle
    // list draws nothing new.
    let head = refreshing.then(rememberSpinnerHead);
    let color = liquid_colors().secondary_label;
    let container = modifier
        .clip_to_bounds()
        .semantics(move |config| config.custom_actions.push(action.clone()))
        .draw_behind(spinner(Rc::clone(&pull), color, head));
    let content: Rc<dyn Fn()> = Rc::new(content);
    Box(container, BoxSpec::default(), move || {
        let moved = Rc::clone(&pull);
        let (connection, content) = (Rc::clone(&connection), Rc::clone(&content));
        Box(
            Modifier::empty().offset_with(move || Point::new(0.0, moved.travel())),
            BoxSpec::default(),
            move || NestedScroll(Rc::clone(&connection), || content()),
        );
    });
}

// The spinner in the gap over the list, centred in it: its spokes come in
// one by one as the pull nears the threshold, and they turn while the list
// refreshes.
fn spinner(
    pull: Rc<Pull>,
    color: Color,
    head: Option<State<f32>>,
) -> impl Fn(&mut dyn DrawScope) + 'static {
    let wheel = LiquidActivityIndicatorSize::Medium
        .wheel()
        .scaled(SPINNER_SCALE);
    move |scope| {
        let travel = pull.travel();
        if travel <= 0.5 {
            return;
        }
        let left = (scope.size().width - wheel.side) * 0.5;
        let top = (travel - wheel.side) * 0.5;
        scope.translate(left, top, &mut |scope| match head {
            Some(head) => wheel.draw(scope, color, head.get(), SPOKES),
            None => {
                let shown = (SPOKES * travel / THRESHOLD).ceil().min(SPOKES);
                // The newest spoke leads.
                wheel.draw(scope, color, shown - 1.0, shown);
            }
        });
    }
}
