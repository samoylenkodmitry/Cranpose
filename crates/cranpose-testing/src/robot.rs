//! Robot testing framework for end-to-end app testing
//!
//! This module provides a robot-style testing API that allows developers to:
//! - Launch real apps in a testing environment
//! - Perform interactions (clicks, moves, drags)
//! - Find and validate UI elements
//! - Test the full app lifecycle
//!
//! # Example
//!
//! ```
//! use cranpose_testing::robot::create_headless_robot_test;
//!
//! let mut robot = create_headless_robot_test(800, 600, || {
//!     // Your composable app here
//! });
//!
//! // Find and click a button
//! robot.click_at(100.0, 100.0);
//!
//! // Wait for updates
//! robot.wait_for_idle();
//! ```

use std::rc::Rc;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::PointerEvent;
use cranpose_render_common::{HitTestTarget, RenderScene, Renderer};
use cranpose_ui::{LayoutTree, TextMeasurer};
use cranpose_ui_graphics::{Point, Rect, Size};

/// How many `AppShell::update` turns a headless robot pump gives the shell
/// before it treats the composition as wedged.
///
/// This budget is an iteration count rather than a wall clock on purpose, and
/// it is the opposite call from the desktop robot's `wait_for_idle`. There the
/// wait is on a compositor in another process, so loop turns say nothing about
/// whether the frame is coming and only elapsed time can bound it. Here
/// `AppShell::update` is synchronous work against an in-memory renderer --
/// no compositor, no surface to acquire, nothing to block on -- so the number
/// of turns a healthy app needs is a property of the app and is the same on
/// every host. Timing it instead would hand a loaded machine fewer turns than
/// a quiet one and decide headless results by host load, which is exactly the
/// flakiness the desktop side had to remove.
///
/// The limit matches `ROOT_RENDER_REPLAY_LIMIT` and has room to spare: the
/// loop condition is `AppShell::needs_redraw`, which reports stale pixels and
/// renderer warm-up rather than pending composition work, and against the
/// in-memory renderer one update clears it -- every call in the suite settles
/// on the first turn. What the budget guards is a shell that stays dirty
/// however many turns it is given.
const HEADLESS_IDLE_UPDATE_LIMIT: u32 = 100;

/// Main robot testing rule that provides programmatic control over a real app.
///
/// This is similar to Jetpack Compose's `ComposeTestRule` but for full app testing
/// with real rendering and input simulation.
pub struct RobotTestRule<R>
where
    R: Renderer,
{
    shell: AppShell<R>,
    frame_time_nanos: u64,
}

impl<R> RobotTestRule<R>
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    /// Create a new robot test rule with the given viewport size and app content.
    ///
    /// The app will be launched immediately with the provided dimensions.
    pub fn new(width: u32, height: u32, renderer: R, content: impl FnMut() + 'static) -> Self {
        let root_key = location_key(file!(), line!(), column!());
        let mut shell = AppShell::new(renderer, root_key, content);
        shell.set_viewport(width as f32, height as f32);
        shell.set_buffer_size(width, height);

        Self {
            shell,
            frame_time_nanos: 0,
        }
    }

    /// Get the current viewport size.
    pub fn viewport_size(&self) -> (u32, u32) {
        self.shell.buffer_size()
    }

    /// Resize the viewport (simulates window resize).
    pub fn set_viewport(&mut self, width: u32, height: u32) {
        self.shell.set_viewport(width as f32, height as f32);
        self.shell.set_buffer_size(width, height);
    }

    /// Advance frame time by the given duration in nanoseconds.
    ///
    /// This is useful for testing animations and time-based behaviors.
    pub fn advance_time(&mut self, nanos: u64) {
        self.frame_time_nanos = self.frame_time_nanos.saturating_add(nanos);
        self.shell.update_at_frame_time_nanos(self.frame_time_nanos);
    }

    /// Current deterministic frame time used by this robot.
    pub fn frame_time_nanos(&self) -> u64 {
        self.frame_time_nanos
    }

    /// Pump the shell until it stops asking to be redrawn.
    ///
    /// The condition is `AppShell::needs_redraw` -- stale pixels and renderer
    /// warm-up. That is a weaker question than "has every pending composition,
    /// layout and render drained", which is what `FrameSchedule` answers, so a
    /// caller that needs the composition itself settled has to say so.
    ///
    /// # Panics
    ///
    /// Panics if the shell still wants a redraw after
    /// `HEADLESS_IDLE_UPDATE_LIMIT` turns. Returning quietly instead would
    /// hand back a half-settled tree and move the failure to whichever
    /// assertion read it next, which is the harder bug to find.
    pub fn wait_for_idle(&mut self) {
        for _ in 0..HEADLESS_IDLE_UPDATE_LIMIT {
            self.shell.update();
            if !self.shell.needs_redraw() {
                return;
            }
        }
        panic!(
            "wait_for_idle: the shell still wants a redraw after \
             {HEADLESS_IDLE_UPDATE_LIMIT} update turns (animations={}, \
             transient frame callbacks={})",
            self.shell.has_active_animations(),
            self.shell.has_transient_frame_callbacks(),
        );
    }

    /// Perform a click at the given coordinates.
    ///
    /// Returns true if the click hit a UI element, false otherwise.
    pub fn click_at(&mut self, x: f32, y: f32) -> bool {
        self.shell.set_cursor(x, y);
        self.shell.pointer_pressed();
        self.shell.pointer_released();
        self.wait_for_idle();
        true
    }

    /// Move the cursor to the given coordinates.
    ///
    /// Returns true if the move hit a UI element, false otherwise.
    pub fn move_to(&mut self, x: f32, y: f32) -> bool {
        let hit = self.shell.set_cursor(x, y);
        self.wait_for_idle();
        hit
    }

    /// Perform a drag from one point to another.
    ///
    /// This simulates a pointer down, move, and up sequence.
    pub fn drag(&mut self, from_x: f32, from_y: f32, to_x: f32, to_y: f32) {
        // Move to start position
        self.shell.set_cursor(from_x, from_y);

        // Press
        self.shell.pointer_pressed();

        // Move in steps to simulate smooth drag
        let steps = 10;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = from_x + (to_x - from_x) * t;
            let y = from_y + (to_y - from_y) * t;
            self.shell.set_cursor(x, y);
            self.shell.update();
        }

        // Release
        self.shell.pointer_released();
        self.wait_for_idle();
    }

    /// Move the mouse cursor to the given coordinates.
    pub fn mouse_move(&mut self, x: f32, y: f32) {
        self.shell.set_cursor(x, y);
        self.shell.update();
    }

    /// Press the mouse button.
    pub fn mouse_down(&mut self) {
        self.shell.pointer_pressed();
        self.shell.update();
    }

    /// Release the mouse button.
    pub fn mouse_up(&mut self) {
        self.shell.pointer_released();
        self.shell.update();
    }

    /// Find an element by text content.
    ///
    /// Returns a finder that can be used to interact with or assert on the element.
    pub fn find_by_text(&mut self, text: &str) -> ElementFinder<'_, R> {
        self.wait_for_idle();
        ElementFinder {
            robot: self,
            query: FinderQuery::Text(text.to_string()),
        }
    }

    /// Find an element at the given position.
    ///
    /// Returns a finder for the topmost element at that position.
    pub fn find_at_position(&mut self, x: f32, y: f32) -> ElementFinder<'_, R> {
        self.wait_for_idle();
        ElementFinder {
            robot: self,
            query: FinderQuery::Position(x, y),
        }
    }

    /// Get all text content currently visible on screen.
    ///
    /// This is useful for debugging or asserting on overall screen state.
    pub fn get_all_text(&mut self) -> Vec<String> {
        self.wait_for_idle();

        self.shell.with_layout_tree(|layout_tree| {
            layout_tree
                .map(extract_text_from_layout)
                .unwrap_or_default()
        })
    }

    /// Get all rectangles (bounds) of UI elements on screen.
    ///
    /// Returns a list of (bounds, optional_text) tuples.
    pub fn get_all_rects(&mut self) -> Vec<(Rect, Option<String>)> {
        self.wait_for_idle();

        self.shell.with_layout_tree(|layout_tree| {
            layout_tree
                .map(extract_rects_from_layout)
                .unwrap_or_default()
        })
    }

    /// Print debug information about the current screen state.
    ///
    /// This outputs the layout tree and render scene for debugging.
    pub fn dump_screen(&mut self) {
        self.shell.log_debug_info();
    }

    /// Get access to the underlying app shell for advanced scenarios.
    pub fn shell_mut(&mut self) -> &mut AppShell<R> {
        &mut self.shell
    }

    /// Get the render scene for hit testing and queries.
    fn get_scene(&self) -> &R::Scene {
        self.shell.scene()
    }
}

/// A query for finding UI elements.
#[derive(Clone, Debug)]
enum FinderQuery {
    Text(String),
    Position(f32, f32),
}

/// A finder for locating and interacting with UI elements.
///
/// Finders are created by calling methods on `RobotTestRule` like
/// `find_by_text()` or `find_at_position()`.
pub struct ElementFinder<'a, R>
where
    R: Renderer,
{
    robot: &'a mut RobotTestRule<R>,
    query: FinderQuery,
}

impl<'a, R> ElementFinder<'a, R>
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    /// Check if an element matching this query exists.
    pub fn exists(&mut self) -> bool {
        match &self.query {
            FinderQuery::Text(text) => {
                let all_text = self.robot.get_all_text();
                all_text.iter().any(|t| t.contains(text))
            }
            FinderQuery::Position(x, y) => !self.robot.get_scene().hit_test(*x, *y).is_empty(),
        }
    }

    /// Get the bounds of the found element.
    ///
    /// Returns None if the element doesn't exist or doesn't have bounds.
    pub fn bounds(&mut self) -> Option<Rect> {
        match &self.query {
            FinderQuery::Text(text) => {
                let rects = self.robot.get_all_rects();
                rects
                    .into_iter()
                    .find(|(_, txt)| txt.as_ref().is_some_and(|t| t.contains(text)))
                    .map(|(rect, _)| rect)
            }
            FinderQuery::Position(x, y) => {
                let (x, y) = (*x, *y);
                self.robot
                    .get_all_rects()
                    .into_iter()
                    .map(|(rect, _)| rect)
                    .filter(|rect| rect.contains(x, y))
                    // The innermost box under the point is the one a caller
                    // means; an ancestor covers the point too and would answer
                    // with the whole screen.
                    .min_by(|a, b| {
                        (a.width * a.height)
                            .partial_cmp(&(b.width * b.height))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            }
        }
    }

    /// Get the center point of the element.
    pub fn center(&mut self) -> Option<Point> {
        self.bounds().map(|rect| Point {
            x: rect.x + rect.width / 2.0,
            y: rect.y + rect.height / 2.0,
        })
    }

    /// Get the width of the element.
    pub fn width(&mut self) -> Option<f32> {
        self.bounds().map(|rect| rect.width)
    }

    /// Get the height of the element.
    pub fn height(&mut self) -> Option<f32> {
        self.bounds().map(|rect| rect.height)
    }

    /// Click on this element at its center.
    ///
    /// Returns true if the element was found and clicked.
    pub fn click(&mut self) -> bool {
        if let Some(center) = self.center() {
            self.robot.click_at(center.x, center.y);
            true
        } else {
            false
        }
    }

    /// Perform a long press on this element.
    ///
    /// This holds the pointer down for a duration before releasing.
    pub fn long_press(&mut self) -> bool {
        if let Some(center) = self.center() {
            self.robot.shell_mut().set_cursor(center.x, center.y);
            self.robot.shell_mut().pointer_pressed();

            // Hold for 500ms (simulated by multiple updates)
            for _ in 0..50 {
                self.robot.shell_mut().update();
            }

            self.robot.shell_mut().pointer_released();
            self.robot.wait_for_idle();
            true
        } else {
            false
        }
    }

    /// Assert that this element exists.
    ///
    /// Panics if the element is not found.
    pub fn assert_exists(&mut self) {
        assert!(self.exists(), "Element not found: {:?}", self.query);
    }
}

/// Extract all text content from a layout tree.
fn extract_text_from_layout(layout: &LayoutTree) -> Vec<String> {
    fn collect_text(node: &cranpose_ui::LayoutBox, results: &mut Vec<String>) {
        if let Some(text) = node.node_data.modifier_slices().text_content() {
            results.push(text.to_string());
        }
        for child in &node.children {
            collect_text(child, results);
        }
    }

    let mut results = Vec::new();
    collect_text(layout.root(), &mut results);
    results
}

/// Extract all rectangles with optional text from a layout tree.
fn extract_rects_from_layout(layout: &LayoutTree) -> Vec<(Rect, Option<String>)> {
    fn collect_rects(node: &cranpose_ui::LayoutBox, results: &mut Vec<(Rect, Option<String>)>) {
        // Get the text content if present in modifier slices
        let text = node
            .node_data
            .modifier_slices()
            .text_content()
            .map(|s| s.to_string());

        // Get the rect, including content_offset for proper positioning
        let rect = Rect {
            x: node.rect.x,
            y: node.rect.y,
            width: node.rect.width,
            height: node.rect.height,
        };

        results.push((rect, text));

        // Recurse into children
        for child in &node.children {
            collect_rects(child, results);
        }
    }

    let mut results = Vec::new();
    collect_rects(layout.root(), &mut results);
    results
}

/// A simple test renderer for robot tests.
///
/// This renderer doesn't actually render anything, but provides the
/// Renderer trait implementation needed for testing.
#[derive(Default)]
pub struct TestRenderer {
    scene: TestScene,
    text_measurer: Option<Rc<dyn TextMeasurer>>,
}

impl TestRenderer {
    pub fn with_text_measurer(text_measurer: Rc<dyn TextMeasurer>) -> Self {
        Self {
            scene: TestScene,
            text_measurer: Some(text_measurer),
        }
    }
}

impl Renderer for TestRenderer {
    type Scene = TestScene;
    type Error = ();

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        if let Some(text_measurer) = &self.text_measurer {
            app_context.set_text_measurer_rc(Rc::clone(text_measurer));
        }
    }

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// The scene used by TestRenderer.
#[derive(Default)]
pub struct TestScene;

impl RenderScene for TestScene {
    type HitTarget = TestHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        vec![TestHitTarget]
    }

    fn find_target(&self, _node_id: cranpose_core::NodeId) -> Option<Self::HitTarget> {
        None
    }
}

/// A hit target used by TestScene.
#[derive(Default, Clone)]
pub struct TestHitTarget;

impl HitTestTarget for TestHitTarget {
    fn dispatch(&self, _event: PointerEvent) {}

    fn node_id(&self) -> cranpose_core::NodeId {
        0
    }
}

/// Create a headless robot test rule for testing without a real renderer.
///
/// This is useful for fast unit tests that don't need actual rendering.
pub fn create_headless_robot_test<F>(
    width: u32,
    height: u32,
    content: F,
) -> RobotTestRule<TestRenderer>
where
    F: FnMut() + 'static,
{
    RobotTestRule::new(width, height, TestRenderer::default(), content)
}

/// Create a headless robot test rule with an explicit text measurer.
pub fn create_headless_robot_test_with_text_measurer<F>(
    width: u32,
    height: u32,
    text_measurer: Rc<dyn TextMeasurer>,
    content: F,
) -> RobotTestRule<TestRenderer>
where
    F: FnMut() + 'static,
{
    RobotTestRule::new(
        width,
        height,
        TestRenderer::with_text_measurer(text_measurer),
        content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_robot_creation() {
        // Create a simple headless robot test
        let robot = create_headless_robot_test(800, 600, || {
            // Empty app for testing
        });

        assert_eq!(robot.viewport_size(), (800, 600));
    }

    #[test]
    fn test_robot_click() {
        let mut robot = create_headless_robot_test(800, 600, || {
            // Empty app
        });

        // Should not panic
        robot.click_at(100.0, 100.0);
    }

    #[test]
    fn test_robot_drag() {
        let mut robot = create_headless_robot_test(800, 600, || {
            // Empty app
        });

        // Should not panic
        robot.drag(0.0, 0.0, 100.0, 100.0);
    }

    #[test]
    fn robot_advance_time_uses_supplied_frame_delta() {
        let mut robot = create_headless_robot_test(800, 600, || {});

        robot.advance_time(16_000_000);
        assert_eq!(robot.frame_time_nanos(), 16_000_000);

        robot.advance_time(8_000_000);
        assert_eq!(robot.frame_time_nanos(), 24_000_000);
    }

    #[test]
    fn robot_idle_pump_settles_a_healthy_app_far_inside_its_budget() {
        // The budget is an iteration count because every turn here is
        // synchronous work with nothing to block on, so what a healthy app
        // needs does not move with host load. Pin that it is a budget with
        // room rather than one the suite runs up against: an app that settles
        // must not depend on where in the budget it lands.
        let mut robot = create_headless_robot_test(800, 600, || {});

        for _ in 0..HEADLESS_IDLE_UPDATE_LIMIT {
            robot.wait_for_idle();
            assert!(!robot.shell_mut().needs_redraw());
        }
    }

    /// A renderer that never finishes warming up, so `needs_redraw` stays true
    /// however many turns it is given. `needs_frame_warmup` is the hook a real
    /// renderer uses to demand initial frames, which makes this the shape a
    /// wedged shell actually takes.
    #[derive(Default)]
    struct NeverWarmRenderer {
        inner: TestRenderer,
    }

    impl Renderer for NeverWarmRenderer {
        type Scene = TestScene;
        type Error = ();

        fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
            self.inner.attach_app_context_services(app_context);
        }

        fn scene(&self) -> &Self::Scene {
            self.inner.scene()
        }

        fn scene_mut(&mut self) -> &mut Self::Scene {
            self.inner.scene_mut()
        }

        fn rebuild_scene(
            &mut self,
            layout_tree: &LayoutTree,
            viewport: Size,
        ) -> Result<(), Self::Error> {
            self.inner.rebuild_scene(layout_tree, viewport)
        }

        fn rebuild_scene_from_applier(
            &mut self,
            applier: &mut cranpose_core::MemoryApplier,
            root: cranpose_core::NodeId,
            viewport: Size,
        ) -> Result<(), Self::Error> {
            self.inner
                .rebuild_scene_from_applier(applier, root, viewport)
        }

        fn needs_frame_warmup(&self) -> bool {
            true
        }
    }

    #[test]
    #[should_panic(expected = "still wants a redraw after")]
    fn robot_idle_pump_fails_loudly_when_the_shell_never_settles() {
        // Exhausting the budget used to `break` and return, handing the caller
        // a tree that had not settled and moving the failure to whichever
        // assertion read it next -- which is the harder bug to find. It has to
        // fail here instead, where the diagnostic can say what is still dirty.
        let mut robot = RobotTestRule::new(800, 600, NeverWarmRenderer::default(), || {});

        robot.wait_for_idle();
    }
}
