//! Brief messages at the bottom of the screen, one at a time from a queue,
//! as Compose's `SnackbarHost` and `SnackbarHostState` show them.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use cranpose_core::{LaunchedEffectAsync, MutableState, delay, mutableStateOf, remember};
use cranpose_foundation::LiveRegionMode;
use cranpose_macros::composable;
use cranpose_services::local_accessibility_state;
use cranpose_ui::{
    Alignment, AnimatedVisibility, Modifier, Row, RowSpec, SemanticsWidgetRole, SpanStyle, Text,
    TextStyle, VerticalAlignment, fade_in, fade_out, slide_in_vertically, slide_out_vertically,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{Brush, Color, CornerRadii};

use crate::theme::{liquid_colors, liquid_typography};

/// How long a snackbar stays before it dismisses itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnackbarDuration {
    /// Four seconds.
    Short,
    /// Ten seconds.
    Long,
    /// Until its action or dismiss action is used, or the app dismisses it.
    Indefinite,
    Exactly(Duration),
}

impl SnackbarDuration {
    /// When a screen reader is on, a snackbar with an action stays until it
    /// is used, and one without stays the long time: a reader needs the time
    /// to reach it.
    fn timeout(self, reader_on: bool, has_action: bool) -> Option<Duration> {
        if reader_on && has_action {
            return None;
        }
        let duration = match self {
            Self::Short => Duration::from_secs(4),
            Self::Long => Duration::from_secs(10),
            Self::Indefinite => return None,
            Self::Exactly(duration) => duration,
        };
        Some(if reader_on {
            duration.max(Duration::from_secs(10))
        } else {
            duration
        })
    }
}

/// How a snackbar ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnackbarResult {
    /// It timed out, or its dismiss action or the app dismissed it.
    Dismissed,
    /// Its action was used.
    ActionPerformed,
}

/// What a snackbar shows.
#[derive(Clone, Debug, PartialEq)]
pub struct SnackbarVisuals {
    pub message: String,
    pub action_label: Option<String>,
    /// The label of a dismiss button, in the app's language: "Dismiss".
    pub dismiss_label: Option<String>,
    pub duration: SnackbarDuration,
}

impl SnackbarVisuals {
    /// A `message` that stays [`SnackbarDuration::Short`].
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            action_label: None,
            dismiss_label: None,
            duration: SnackbarDuration::Short,
        }
    }

    /// Adds an action, such as "Undo", which ends the snackbar with
    /// [`SnackbarResult::ActionPerformed`].
    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action_label = Some(label.into());
        self
    }

    /// Adds a dismiss button labelled `label`, such as "Dismiss" in the
    /// app's language.
    pub fn dismiss_action(mut self, label: impl Into<String>) -> Self {
        self.dismiss_label = Some(label.into());
        self
    }

    pub fn duration(mut self, duration: SnackbarDuration) -> Self {
        self.duration = duration;
        self
    }
}

/// The queue of snackbars a [`LiquidSnackbarHost`] shows. A clone is the same
/// queue. Make one with [`rememberLiquidSnackbarHostState`].
#[derive(Clone)]
pub struct LiquidSnackbarHostState {
    inner: Rc<HostQueue>,
}

impl PartialEq for LiquidSnackbarHostState {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

struct HostQueue {
    shown: MutableState<Option<Shown>>,
    waiting: RefCell<VecDeque<Shown>>,
    next_id: Cell<u64>,
}

#[derive(Clone)]
struct Shown {
    id: u64,
    visuals: Rc<SnackbarVisuals>,
    completion: Rc<Completion>,
}

impl PartialEq for Shown {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Default)]
struct Completion {
    result: Cell<Option<SnackbarResult>>,
    waker: RefCell<Option<Waker>>,
}

impl Completion {
    fn complete(&self, result: SnackbarResult) {
        self.result.set(Some(result));
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

/// The future [`LiquidSnackbarHostState::show`] returns. Dropping it leaves
/// the snackbar queued.
pub struct SnackbarShown {
    completion: Rc<Completion>,
}

impl Future for SnackbarShown {
    type Output = SnackbarResult;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<SnackbarResult> {
        match self.completion.result.get() {
            Some(result) => Poll::Ready(result),
            None => {
                *self.completion.waker.borrow_mut() = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl LiquidSnackbarHostState {
    fn new() -> Self {
        Self {
            inner: Rc::new(HostQueue {
                shown: mutableStateOf(None),
                waiting: RefCell::new(VecDeque::new()),
                next_id: Cell::new(0),
            }),
        }
    }

    /// Queues `visuals` behind the snackbars already shown or waiting, and
    /// resolves with how it ended.
    pub fn show(&self, visuals: SnackbarVisuals) -> SnackbarShown {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        let completion = Rc::new(Completion::default());
        self.inner.waiting.borrow_mut().push_back(Shown {
            id,
            visuals: Rc::new(visuals),
            completion: Rc::clone(&completion),
        });
        if self.inner.shown.get_non_reactive().is_none() {
            self.advance();
        }
        SnackbarShown { completion }
    }

    /// What the snackbar on screen shows, if one is.
    pub fn current(&self) -> Option<SnackbarVisuals> {
        self.inner.shown.get().map(|shown| (*shown.visuals).clone())
    }

    /// Dismisses the snackbar on screen, which ends with
    /// [`SnackbarResult::Dismissed`], and shows the next one.
    pub fn dismiss(&self) {
        if let Some(shown) = self.inner.shown.get_non_reactive() {
            self.finish(shown.id, SnackbarResult::Dismissed);
        }
    }

    fn finish(&self, id: u64, result: SnackbarResult) {
        let Some(shown) = self.inner.shown.get_non_reactive() else {
            return;
        };
        if shown.id != id {
            return;
        }
        shown.completion.complete(result);
        self.advance();
    }

    fn advance(&self) {
        let next = self.inner.waiting.borrow_mut().pop_front();
        self.inner.shown.set(next);
    }
}

/// A snackbar queue that lives as long as the caller stays composed.
#[composable]
pub fn rememberLiquidSnackbarHostState() -> LiquidSnackbarHostState {
    remember(LiquidSnackbarHostState::new).with(Clone::clone)
}

/// Shows the snackbars of `state` one at a time, sliding in from the bottom.
/// Each times out after its duration, longer while a screen reader is on,
/// and readers hear the message as it appears. Place it at the bottom of the
/// screen, over the content.
#[composable]
pub fn LiquidSnackbarHost(state: LiquidSnackbarHostState, modifier: Modifier) {
    let shown = state.inner.shown.get();
    let reader_on = local_accessibility_state().current().screen_reader_on;
    // Kept through the exit animation, after the queue moved on.
    let last = remember(|| mutableStateOf(None::<Shown>)).with(|last| *last);
    if let Some(shown) = &shown {
        if last.get_non_reactive().as_ref() != Some(shown) {
            last.set(Some(shown.clone()));
        }
        let timeout = shown
            .visuals
            .duration
            .timeout(reader_on, shown.visuals.action_label.is_some());
        let (host, id) = (state.clone(), shown.id);
        LaunchedEffectAsync((id, timeout), move |_| {
            Box::pin(async move {
                if let Some(timeout) = timeout {
                    delay(timeout).await;
                    host.finish(id, SnackbarResult::Dismissed);
                }
            })
        });
    }
    AnimatedVisibility(
        shown.is_some(),
        fade_in() + slide_in_vertically(1.0),
        fade_out() + slide_out_vertically(1.0),
        move || {
            if let Some(shown) = last.get() {
                Snackbar(state.clone(), shown, modifier.clone());
            }
        },
    );
}

#[composable]
fn Snackbar(state: LiquidSnackbarHostState, shown: Shown, modifier: Modifier) {
    let colors = liquid_colors();
    let typography = liquid_typography();
    // Inverse colors, as a snackbar stands apart from the screen under it.
    let (background, foreground) = (colors.label, colors.background);
    let message_style = colored(&typography.subheadline, foreground);
    let action_style = colored(&typography.headline, colors.accent);
    let id = shown.id;
    let dismissing = state.clone();
    let container = modifier
        .padding(12.0)
        .draw_behind(move |scope| {
            scope.draw_round_rect(Brush::solid(background), CornerRadii::uniform(14.0));
        })
        .padding_horizontal(16.0)
        .padding_vertical(10.0)
        .live_region(LiveRegionMode::Polite)
        .dismiss(move || {
            dismissing.finish(id, SnackbarResult::Dismissed);
            true
        });
    Row(
        container,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                shown.visuals.message.clone(),
                Modifier::empty().weight(1.0),
                message_style.clone(),
            );
            if let Some(label) = shown.visuals.action_label.clone() {
                let acting = state.clone();
                SnackbarButton(label, action_style.clone(), move || {
                    acting.finish(id, SnackbarResult::ActionPerformed);
                });
            }
            if let Some(label) = shown.visuals.dismiss_label.clone() {
                let dismissing = state.clone();
                SnackbarButton(label, message_style.clone(), move || {
                    dismissing.finish(id, SnackbarResult::Dismissed);
                });
            }
        },
    );
}

fn colored(style: &TextStyle, color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            ..style.span_style.clone()
        },
        ..style.clone()
    }
}

#[composable]
fn SnackbarButton(label: String, style: cranpose_ui::TextStyle, on_click: impl Fn() + 'static) {
    Box(
        Modifier::empty()
            .minimum_interactive_component_size()
            .clickable(move |_| on_click())
            .semantics(|config| config.role = Some(SemanticsWidgetRole::Button)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                label.clone(),
                Modifier::empty().padding_horizontal(8.0),
                style.clone(),
            );
        },
    );
}
