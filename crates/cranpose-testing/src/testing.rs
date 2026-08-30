#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::rc::Rc;
use std::rc::Rc as StdRc;

use cranpose_core::{
    ApplierGuard, Composition, Key, MemoryApplier, NodeError, NodeId, ROOT_RENDER_REPLAY_LIMIT,
    RuntimeHandle, location_key,
};
#[cfg(test)]
use cranpose_core::{
    MutableState, Node, pop_parent, push_parent, with_current_composer, with_node_mut,
};
use cranpose_ui::{AppContext, request_render_invalidation, reset_render_state_for_tests};

/// Headless harness for exercising compositions in tests.
///
/// `ComposeTestRule` mirrors the ergonomics of the Jetpack Compose testing APIs
/// while remaining lightweight and allocation-friendly for unit tests. It owns
/// an in-memory applier and exposes helpers for driving recomposition and
/// draining frame callbacks without requiring a windowing backend.
pub struct ComposeTestRule {
    _scope: cranpose_ui::AppContextScope,
    app_context: StdRc<AppContext>,
    composition: Composition<MemoryApplier>,
    content: Option<Box<dyn FnMut()>>,
    initial_root_key: Key,
}

impl ComposeTestRule {
    /// Create a new test rule backed by the default in-memory applier.
    pub fn new() -> Self {
        let app_context = AppContext::new();
        app_context.enter(reset_render_state_for_tests);
        let scope = app_context.enter_scope();
        Self {
            _scope: scope,
            app_context,
            composition: Composition::new(MemoryApplier::new()),
            content: None,
            initial_root_key: location_key(file!(), line!(), column!()),
        }
    }

    fn root_key(&self) -> Key {
        self.composition.root_key().unwrap_or(self.initial_root_key)
    }

    /// Install the provided content into the composition and perform an
    /// initial render.
    pub fn set_content(&mut self, content: impl FnMut() + 'static) -> Result<(), NodeError> {
        self.content = Some(Box::new(content));
        self.render()
    }

    /// Force a recomposition using the currently installed content.
    pub fn recomposition(&mut self) -> Result<(), NodeError> {
        self.render()
    }

    /// Drain scheduled frame callbacks at the supplied timestamp and process
    /// any resulting work until the composition becomes idle.
    pub fn advance_frame(&mut self, frame_time_nanos: u64) -> Result<(), NodeError> {
        let app_context = StdRc::clone(&self.app_context);
        app_context.enter(|| {
            let handle = self.composition.runtime_handle();
            handle.drain_frame_callbacks(frame_time_nanos);
            self.pump_until_idle_in_context()
        })
    }

    /// Drive the composition until there are no pending renders, invalidated
    /// scopes, or enqueued node mutations remaining.
    pub fn pump_until_idle(&mut self) -> Result<(), NodeError> {
        let app_context = StdRc::clone(&self.app_context);
        app_context.enter(|| self.pump_until_idle_in_context())
    }

    fn pump_until_idle_in_context(&mut self) -> Result<(), NodeError> {
        let mut i = 0;
        loop {
            let mut progressed = false;
            i += 1;
            if i > ROOT_RENDER_REPLAY_LIMIT {
                return Err(NodeError::RecompositionLimitExceeded {
                    operation: "pump_until_idle",
                    limit: ROOT_RENDER_REPLAY_LIMIT,
                });
            }

            if self.composition.should_render() {
                self.render()?;
                progressed = true;
            }

            let handle = self.composition.runtime_handle();
            if handle.has_updates() {
                self.composition.flush_pending_node_updates()?;
                progressed = true;
            }

            if handle.has_invalid_scopes() {
                let changed = self.composition.process_invalid_scopes()?;
                if changed {
                    request_render_invalidation();
                }
                if self.composition.take_root_render_request() {
                    self.render()?;
                }
                progressed = true;
            }

            if !progressed {
                break;
            }
        }
        Ok(())
    }

    /// Access the runtime driving this rule. Useful for constructing shared
    /// state objects within the composition.
    pub fn runtime_handle(&self) -> RuntimeHandle {
        self.composition.runtime_handle()
    }

    /// Gain mutable access to the underlying in-memory applier for assertions
    /// about the produced node tree.
    pub fn applier_mut(&mut self) -> ApplierGuard<'_, MemoryApplier> {
        self.composition.applier_mut()
    }

    /// Dump the current node tree as text for debugging
    pub fn dump_tree(&mut self) -> String {
        let root = self.composition.root();
        let applier = self.composition.applier_mut();
        applier.dump_tree(root)
    }

    /// Returns whether user content has been installed in this rule.
    pub fn has_content(&self) -> bool {
        self.content.is_some()
    }

    /// Returns the id of the root node produced by the current composition.
    pub fn root_id(&self) -> Option<NodeId> {
        self.composition.root()
    }

    pub fn with_app_context<R>(&self, block: impl FnOnce() -> R) -> R {
        self.app_context.enter(block)
    }

    /// Gain mutable access to the raw composition for advanced scenarios.
    pub fn composition(&mut self) -> &mut Composition<MemoryApplier> {
        &mut self.composition
    }

    /// Lay the installed content out on a `size` display and read back every
    /// semantics node with the geometry it was placed and drawn at.
    ///
    /// This is the headless answer to "how big is that control, and where" —
    /// see [`crate::placed_semantics`] for what the two boxes on a node mean
    /// and why every walk behind this reads the retained tree.
    ///
    /// The runtime handle is installed for the duration and taken back off
    /// afterwards: a subcomposing widget cannot be measured without one, and
    /// leaving it on outlives the borrow the applier hands out.
    pub fn placed_semantics(
        &mut self,
        size: cranpose_ui::Size,
    ) -> Result<Option<crate::placed_semantics::PlacedSemanticsNode>, NodeError> {
        self.pump_until_idle()?;
        let Some(root) = self.composition.root() else {
            return Ok(None);
        };
        let handle = self.composition.runtime_handle();
        let app_context = StdRc::clone(&self.app_context);
        app_context.enter(|| {
            let mut applier = self.composition.applier_mut();
            applier.set_runtime_handle(handle);
            let placed =
                crate::placed_semantics::placed_semantics_from_applier(&mut applier, root, size);
            applier.clear_runtime_handle();
            placed
        })
    }

    fn render(&mut self) -> Result<(), NodeError> {
        let app_context = StdRc::clone(&self.app_context);
        app_context.enter(|| {
            let key = self.root_key();
            if let Some(content) = self.content.as_mut() {
                self.composition.render(key, &mut **content)?;
                self.drain_root_render_requests()?;
                request_render_invalidation();
            }
            Ok(())
        })
    }

    fn drain_root_render_requests(&mut self) -> Result<(), NodeError> {
        let key = self.root_key();
        for _ in 0..ROOT_RENDER_REPLAY_LIMIT {
            if !self.composition.take_root_render_request() {
                return Ok(());
            }
            let content = self
                .content
                .as_mut()
                .ok_or(NodeError::RecompositionLimitExceeded {
                    operation: "root render replay",
                    limit: ROOT_RENDER_REPLAY_LIMIT,
                })?;
            self.composition.render(key, &mut **content)?;
            request_render_invalidation();
        }

        Err(NodeError::RecompositionLimitExceeded {
            operation: "root render replay",
            limit: ROOT_RENDER_REPLAY_LIMIT,
        })
    }
}

impl Default for ComposeTestRule {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience helper for tests that only need temporary access to a
/// `ComposeTestRule`.
pub fn run_test_composition<R>(f: impl FnOnce(&mut ComposeTestRule) -> R) -> R {
    let mut rule = ComposeTestRule::new();
    f(&mut rule)
}

#[cfg(test)]
#[path = "tests/testing_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/recomposition_tests.rs"]
mod recomposition_tests;
