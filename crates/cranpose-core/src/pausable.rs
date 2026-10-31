//! Content that composes across frames: [`with_pausable_content`].
//!
//! A composable called for the first time inside pausable content runs its
//! body while the frame's composition budget lasts. Past the budget its group
//! stays empty and its body waits for a later frame, where the composition
//! runs it after the invalid scopes, again within that frame's budget. The
//! content is complete once no body waits. Until then the effects it
//! registers and the invalidations of its scopes wait too, so they run on a
//! complete composition, as they do for content composed in one pass; its
//! owner shows the content only once it is complete. Every frame runs at
//! least one waiting body, so content completes however small the budget.

use std::{
    cell::{Cell, OnceCell, RefCell},
    fmt,
    rc::{Rc, Weak},
    time::Duration,
};

use web_time::Instant;

use crate::{
    Composer, FrameCallbackId, RecomposeScope, RecomposeScopeInner, Runtime, RuntimeHandle,
    composer_context,
};

// First bodies between two reads of the clock.
const CLOCK_STRIDE: u8 = 8;

type Effect = Box<dyn FnOnce()>;

/// Content that may compose across frames, for a container that keeps
/// showing what it showed until the content is complete. Compose's
/// `PausableComposition`, for a part of one composition.
#[doc(hidden)]
#[derive(Clone)]
pub struct PausableContent(Rc<Region>);

impl PausableContent {
    /// Content not composed yet. `on_complete` runs when the content
    /// completes in a frame after the one it started in, after its effects:
    /// its owner shows it then.
    pub fn new(on_complete: impl FnOnce() + 'static) -> Self {
        Self(Rc::new(Region {
            stage: Cell::new(Stage::Fresh),
            paused: RefCell::default(),
            scopes: RefCell::default(),
            effects: RefCell::default(),
            on_complete: Cell::new(Some(Box::new(on_complete))),
        }))
    }

    /// Whether the content composed in full, with no body waiting.
    pub fn is_complete(&self) -> bool {
        self.0.stage.get() == Stage::Complete
    }

    // Keeps a waiting body that its pass left for the next one.
    pub(crate) fn keep_waiting(&self, scope: &RecomposeScope) {
        self.0.keep_waiting(scope);
    }
}

impl PartialEq for PausableContent {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for PausableContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PausableContent")
            .field(&self.0.stage.get())
            .finish()
    }
}

/// Composes `compose` as pausable `content`: a composable it calls for the
/// first time may wait for a later frame once this frame's composition budget
/// is spent. Inside other pausable content it composes as part of that
/// content and completes with it.
#[doc(hidden)]
pub fn with_pausable_content(content: &PausableContent, compose: impl FnOnce()) {
    composer_context::with_composer(|composer| composer.with_pausable_content(&content.0, compose));
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Fresh,
    Waiting,
    Complete,
}

// One piece of pausable content.
struct Region {
    stage: Cell<Stage>,
    // The scopes whose first body waits, in composition order.
    paused: RefCell<Vec<RecomposeScope>>,
    // The scopes the content opened before it completed: their invalidations
    // wait until it completes.
    scopes: RefCell<Vec<Weak<RecomposeScopeInner>>>,
    // The effects the content registered before it completed.
    effects: RefCell<Vec<Effect>>,
    on_complete: Cell<Option<Effect>>,
}

impl Region {
    // Keeps a waiting body for the next frame.
    fn keep_waiting(&self, scope: &RecomposeScope) {
        self.paused.borrow_mut().push(scope.clone());
    }

    // Marks the content complete, lets its scopes' invalidations through and
    // gives back the effects that waited.
    fn complete(&self) -> Vec<Effect> {
        self.stage.set(Stage::Complete);
        self.release_scopes();
        std::mem::take(&mut *self.effects.borrow_mut())
    }

    // Whether any of the content's scopes is still in the composition.
    fn is_composed(&self) -> bool {
        self.scopes
            .borrow()
            .iter()
            .filter_map(RecomposeScope::upgrade)
            .any(|scope| scope.is_effectively_active())
    }

    // Content that left the composition before it completed: its effects
    // never run, and it composes from the start if it comes back.
    fn abandon(&self) {
        self.stage.set(Stage::Fresh);
        self.effects.borrow_mut().clear();
        self.release_scopes();
    }

    fn release_scopes(&self) {
        let scopes = std::mem::take(&mut *self.scopes.borrow_mut());
        for scope in scopes.iter().filter_map(RecomposeScope::upgrade) {
            scope.set_pending(false);
        }
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // Content dropped before it completed leaves its scopes as any other.
        self.release_scopes();
    }
}

// The budget of the composition pass that runs now.
#[derive(Clone, Copy, Default)]
enum PassBudget {
    // No pass of the composition's own loop runs: nothing waits.
    #[default]
    Closed,
    // A pass in which no body waited when it began: its clock starts at its
    // first pausable content, so a pass with none never reads it.
    Open,
    // A pass that composes pausable content in full.
    Unlimited,
    // A pass whose first bodies in pausable content wait from this instant.
    Until(Instant),
}

// What the composers of one composition share about pausable content.
#[derive(Default)]
pub(crate) struct Pausing {
    budget: Cell<Option<Duration>>,
    // The content composing now, if any.
    current: RefCell<Option<Rc<Region>>>,
    holding: Cell<bool>,
    deadline: Cell<PassBudget>,
    over: Cell<bool>,
    countdown: Cell<u8>,
    // Whether a waiting body ran in this pass.
    resumed: Cell<bool>,
    // Content with waiting bodies, in the order it first paused.
    waiting: RefCell<Vec<Weak<Region>>>,
    // Whether content waited when a pass last ended or started waiting
    // since: a pass ends with no other work while it is not set.
    engaged: Cell<bool>,
    // Whether a frame started since waiting bodies last ran. The runtime
    // counts a due frame as an invalid scope. Set up once content waits.
    due: OnceCell<Rc<Cell<bool>>>,
    next_frame: Cell<Option<FrameCallbackId>>,
    // The runtime of `next_frame`'s callback, once content waited.
    runtime: RefCell<Option<RuntimeHandle>>,
}

impl Drop for Pausing {
    fn drop(&mut self) {
        // A composition that leaves while content waits gives back its frame.
        let Some(runtime) = self.runtime.get_mut().take() else {
            return;
        };
        if let Some(id) = self.next_frame.take() {
            runtime.cancel_frame_callback(id);
        }
        if self.due.get().is_some_and(|due| due.get()) {
            runtime.scope_runtime().mark_scope_recomposed();
        }
    }
}

impl Pausing {
    pub(crate) fn set_budget(&self, budget: Option<Duration>) {
        self.budget.set(budget);
    }

    // Starts the budget of a frame's composition. The checks of a pass with
    // nothing pausable take no call.
    #[inline]
    pub(crate) fn start_budget(&self) {
        if self.is_due() {
            self.start_resuming();
        } else {
            self.deadline.set(PassBudget::Open);
        }
    }

    // Starts a pass in which waiting bodies run: its clock starts now.
    fn start_resuming(&self) {
        self.start_clock();
        self.resumed.set(false);
    }

    // Starts the pass's clock: first bodies in pausable content wait once
    // the budget from now is spent.
    fn start_clock(&self) {
        self.deadline.set(match self.budget.get() {
            Some(budget) => PassBudget::Until(Instant::now() + budget),
            None => PassBudget::Unlimited,
        });
        self.over.set(false);
        self.countdown.set(0);
    }

    // Ends the frame's composition: waiting bodies run on the next frame.
    #[inline]
    pub(crate) fn end_budget(&self, runtime: &Runtime) {
        self.deadline.set(PassBudget::Closed);
        if self.engaged.get() {
            self.end_engaged(runtime.handle());
        }
    }

    // Opens the budget of the frame's layout pass: a first body that a
    // measure composes in pausable content waits once it is spent.
    #[inline]
    pub(crate) fn open_layout(&self) {
        self.deadline.set(PassBudget::Open);
    }

    // Ends the frame's layout pass: content that waited in it asks for the
    // next frame, as content that waited in the composition does.
    #[inline]
    pub(crate) fn close_layout(&self, runtime: &RuntimeHandle) {
        self.deadline.set(PassBudget::Closed);
        if self.engaged.get() {
            self.end_engaged(runtime.clone());
        }
    }

    // Ends a pass after which content may wait: asks for the next frame
    // while any does.
    fn end_engaged(&self, runtime: RuntimeHandle) {
        let due = self.due.get_or_init(Rc::default);
        if due.replace(false) {
            runtime.scope_runtime().mark_scope_recomposed();
        }
        let mut waiting = self.waiting.borrow_mut();
        waiting.retain(|region| region.strong_count() > 0);
        // A callback that ran keeps no frame coming: register anew.
        if let Some(id) = self.next_frame.take() {
            runtime.cancel_frame_callback(id);
        }
        self.engaged.set(!waiting.is_empty());
        if !waiting.is_empty() {
            let due = Rc::clone(due);
            let counted = runtime.scope_runtime();
            self.next_frame
                .set(runtime.register_frame_callback(move |_| {
                    if !due.replace(true) {
                        counted.hold_due_bodies();
                    }
                }));
            self.runtime.replace(Some(runtime));
        }
    }

    // Whether bodies wait for this frame's composition.
    #[inline]
    fn is_due(&self) -> bool {
        // Content is engaged whenever a frame can be due: a pass with none
        // reads no shared cell.
        self.engaged.get() && self.due.get().is_some_and(|due| due.get())
    }

    // Whether the content composing now holds its effects and scopes.
    #[inline]
    pub(crate) fn is_holding(&self) -> bool {
        self.holding.get()
    }

    // Whether the frame's budget is spent, reading the clock every
    // `CLOCK_STRIDE` calls.
    fn spent(&self) -> bool {
        if let PassBudget::Open = self.deadline.get() {
            self.start_clock();
        }
        let PassBudget::Until(deadline) = self.deadline.get() else {
            return false;
        };
        if self.over.get() {
            return true;
        }
        let left = self.countdown.get();
        if left > 0 {
            self.countdown.set(left - 1);
            return false;
        }
        self.countdown.set(CLOCK_STRIDE - 1);
        self.spent_now(deadline)
    }

    fn spent_now(&self, deadline: Instant) -> bool {
        let over = Instant::now() >= deadline;
        self.over.set(over);
        over
    }

    // Whether a waiting body waits for the next frame: once one ran in this
    // pass, when the budget is spent.
    fn must_wait(&self) -> bool {
        self.resumed.get()
            && matches!(self.deadline.get(), PassBudget::Until(at) if self.spent_now(at))
    }

    fn enter(&self, region: Rc<Region>) -> Option<Rc<Region>> {
        self.holding.set(true);
        self.current.replace(Some(region))
    }

    fn leave(&self, previous: Option<Rc<Region>>) {
        self.holding.set(previous.is_some());
        *self.current.borrow_mut() = previous;
    }

    fn current(&self) -> Option<Rc<Region>> {
        self.current.borrow().clone()
    }

    fn wait(&self, region: &Rc<Region>) {
        if region.stage.replace(Stage::Waiting) != Stage::Waiting {
            self.waiting.borrow_mut().push(Rc::downgrade(region));
            self.engaged.set(true);
        }
    }

    // The waiting bodies of the first content that has any, when this
    // frame's budget allows them to run. Content left with none completes
    // here and runs its effects.
    #[inline]
    pub(crate) fn take_resumable(&self) -> Option<(PausableContent, Vec<RecomposeScope>)> {
        if self.is_due() {
            self.take_due_resumable()
        } else {
            None
        }
    }

    fn take_due_resumable(&self) -> Option<(PausableContent, Vec<RecomposeScope>)> {
        if matches!(self.deadline.get(), PassBudget::Closed) || self.must_wait() {
            return None;
        }
        loop {
            let region = {
                let mut waiting = self.waiting.borrow_mut();
                if waiting.is_empty() {
                    return None;
                }
                waiting.remove(0).upgrade()
            };
            let Some(region) = region else {
                continue;
            };
            let mut scopes = std::mem::take(&mut *region.paused.borrow_mut());
            // A body that its parent ran meanwhile, or whose group left,
            // waits no longer.
            scopes.retain(|scope| {
                let waits = scope.is_effectively_active() && !scope.has_composed_once();
                if !waits {
                    scope.replace_paused(false);
                }
                waits
            });
            if scopes.is_empty() {
                if region.is_composed() {
                    complete_resumed(&region);
                } else {
                    region.abandon();
                }
                continue;
            }
            return Some((PausableContent(region), scopes));
        }
    }

    // After the pass of [`Self::take_resumable`]'s bodies of `content`: the
    // content completes when none waits any more, or waits for the next
    // frame.
    pub(crate) fn finish_resumed(&self, content: PausableContent) {
        let region = content.0;
        if region.paused.borrow().is_empty() {
            complete_resumed(&region);
        } else {
            self.waiting.borrow_mut().insert(0, Rc::downgrade(&region));
        }
    }
}

// Completes content whose last waiting body ran in a pass of its own: its
// effects run now, then its owner learns it can show it.
fn complete_resumed(region: &Region) {
    for effect in region.complete() {
        effect();
    }
    if let Some(on_complete) = region.on_complete.take() {
        on_complete();
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Composer {
    fn pausing(&self) -> &Pausing {
        &self.core.shared_state.pausing
    }

    fn with_pausable_content(&self, region: &Rc<Region>, compose: impl FnOnce()) {
        let pausing = self.pausing();
        if region.stage.get() == Stage::Complete || pausing.is_holding() {
            region.stage.set(Stage::Complete);
            compose();
            return;
        }
        let previous = pausing.enter(Rc::clone(region));
        compose();
        pausing.leave(previous);
        if region.paused.borrow().is_empty() {
            let effects = region.complete();
            self.side_effects_mut().extend(effects);
        } else {
            pausing.wait(region);
        }
    }

    // Whether a group opened in pausable content, whose body has not run
    // yet, waits for a later frame.
    #[inline(never)]
    pub(crate) fn pauses_first_body(&self, scope: &RecomposeScope) -> bool {
        !scope.has_composed_once() && self.pausing().spent()
    }

    // Notes a group opened in pausable content.
    pub(crate) fn hold_scope(&self, scope: &RecomposeScope) {
        if let Some(region) = self.pausing().current()
            && !scope.is_pending()
        {
            scope.set_pending(true);
            region.scopes.borrow_mut().push(scope.downgrade());
        }
    }

    // Records a group whose first body waits: it stays invalid until the
    // body runs.
    pub(crate) fn hold_paused(&self, scope: &RecomposeScope) {
        let Some(region) = self.pausing().current() else {
            return;
        };
        scope.invalidate();
        if !scope.replace_paused(true) {
            region.paused.borrow_mut().push(scope.clone());
        }
    }

    // Queues `effect` to run after the pass applied its node changes, or,
    // in pausable content, once the content completes.
    pub(crate) fn queue_side_effect(&self, effect: Effect) {
        let region = self
            .pausing()
            .is_holding()
            .then(|| self.pausing().current())
            .flatten();
        match region {
            Some(region) => region.effects.borrow_mut().push(effect),
            None => self.side_effects_mut().push(effect),
        }
    }

    // Runs the waiting bodies that [`Pausing::take_resumable`] gave for
    // `content`, in it, while the frame's budget lasts: the rest wait for
    // the next frame.
    pub(crate) fn resume_waiting_bodies(
        &self,
        content: &PausableContent,
        scopes: &[RecomposeScope],
    ) {
        for scope in scopes {
            debug_assert!(scope.is_paused(), "only bodies that waited resume");
            if self.begin_resume(&content.0, scope) {
                self.recompose_group(scope);
                self.end_resume(scope);
            }
        }
    }

    // Enters `region`, in which the first body of `scope` waited, for the
    // body to run, or keeps it waiting for the next frame once the budget is
    // spent. Says whether the body runs.
    fn begin_resume(&self, region: &Rc<Region>, scope: &RecomposeScope) -> bool {
        let pausing = self.pausing();
        if pausing.must_wait() {
            region.keep_waiting(scope);
            return false;
        }
        pausing.resumed.set(true);
        // Waiting bodies run at the top of a pass, outside other content.
        let outside = pausing.enter(Rc::clone(region));
        debug_assert!(outside.is_none(), "a waiting body runs outside content");
        true
    }

    // Leaves the content after the first body of `scope` ran.
    fn end_resume(&self, scope: &RecomposeScope) {
        self.pausing().leave(None);
        scope.replace_paused(false);
        scope.mark_composed_once();
    }
}
