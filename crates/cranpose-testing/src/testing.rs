use cranpose_core::{
    location_key, ApplierGuard, Composition, Key, MemoryApplier, NodeError, NodeId, RuntimeHandle,
};
use cranpose_ui::{request_render_invalidation, reset_render_state_for_tests};

#[cfg(test)]
use cranpose_core::{
    pop_parent, push_parent, with_current_composer, with_node_mut, MutableState, Node,
};
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::rc::Rc;

/// Headless harness for exercising compositions in tests.
///
/// `ComposeTestRule` mirrors the ergonomics of the Jetpack Compose testing APIs
/// while remaining lightweight and allocation-friendly for unit tests. It owns
/// an in-memory applier and exposes helpers for driving recomposition and
/// draining frame callbacks without requiring a windowing backend.
pub struct ComposeTestRule {
    composition: Composition<MemoryApplier>,
    content: Option<Box<dyn FnMut()>>, // Stored user content for reuse across recompositions.
    root_key: Key,
}

impl ComposeTestRule {
    /// Create a new test rule backed by the default in-memory applier.
    pub fn new() -> Self {
        reset_render_state_for_tests();
        Self {
            composition: Composition::new(MemoryApplier::new()),
            content: None,
            root_key: location_key(file!(), line!(), column!()),
        }
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
        let handle = self.composition.runtime_handle();
        handle.drain_frame_callbacks(frame_time_nanos);
        self.pump_until_idle()
    }

    /// Drive the composition until there are no pending renders, invalidated
    /// scopes, or enqueued node mutations remaining.
    pub fn pump_until_idle(&mut self) -> Result<(), NodeError> {
        let mut i = 0;
        loop {
            let mut progressed = false;
            i += 1;
            if i > 100 {
                panic!("pump_until_idle looped too many times!");
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
                    // Request render invalidation so tests can detect composition changes
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

    /// Gain mutable access to the raw composition for advanced scenarios.
    pub fn composition(&mut self) -> &mut Composition<MemoryApplier> {
        &mut self.composition
    }

    fn render(&mut self) -> Result<(), NodeError> {
        if let Some(content) = self.content.as_mut() {
            self.composition.render(self.root_key, &mut **content)?;
            self.drain_root_render_requests()?;
            // After composition runs, request render invalidation
            // so that tests can detect when content has changed
            request_render_invalidation();
        }
        Ok(())
    }

    fn drain_root_render_requests(&mut self) -> Result<(), NodeError> {
        for _ in 0..100 {
            if !self.composition.take_root_render_request() {
                return Ok(());
            }
            let content = self
                .content
                .as_mut()
                .expect("root render replay requires installed content");
            self.composition.render(self.root_key, &mut **content)?;
            request_render_invalidation();
        }

        panic!("root render replay looped too many times");
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
