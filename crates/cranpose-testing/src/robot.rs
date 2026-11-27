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

use cranpose_app_shell::{AppShell, PointerEventTime};
use cranpose_core::{MemoryApplier, NodeId, location_key};
use cranpose_render_common::{
    RenderScene, Renderer, SceneUpdates, graph_scene::Scene, text_fonts::TextFonts,
};
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::{Point, Rect, Size};

const HEADLESS_IDLE_UPDATE_LIMIT: u32 = 100;
const HEADLESS_MOTION_FRAME_LIMIT: u32 = 120;
const FRAME_NANOS: u64 = 16_666_667;
// The gap between the samples of a drag: a 120 Hz touch panel's, which the gesture velocity also
// takes for a gesture's first samples.
const DRAG_SAMPLE_NANOS: u64 = 8_000_000;
// The frames for which a long press holds the pointer down: longer than a long press's timeout.
const LONG_PRESS_FRAMES: u32 = 50;

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
        // Content composes in the frame that brings it, however slow the
        // machine that runs the test.
        shell.set_pausable_budget(None);
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
    /// Each turn advances the deterministic frame clock by 1/60 second,
    /// including animations that request their next redraw from a draw closure.
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
            self.advance_time(FRAME_NANOS);
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

    /// Pump the shell until its animations and frame callbacks have finished,
    /// then until it stops asking to be redrawn: the screen once its motion
    /// has settled. [`wait_for_idle`](Self::wait_for_idle) returns while an
    /// animation that only moves pixels still runs. Motion that a frame of
    /// that redraw starts, as a focus move's scroll does, is waited for too.
    ///
    /// # Panics
    ///
    /// Panics if motion still runs after two seconds of frames, as an
    /// infinite animation does.
    pub fn wait_for_motion(&mut self) {
        for _ in 0..HEADLESS_MOTION_FRAME_LIMIT {
            if !self.has_motion() {
                self.wait_for_idle();
                if !self.has_motion() {
                    return;
                }
            }
            self.advance_time(FRAME_NANOS);
        }
        panic!("wait_for_motion: animations still run after {HEADLESS_MOTION_FRAME_LIMIT} frames");
    }

    fn has_motion(&self) -> bool {
        self.shell.has_active_animations() || self.shell.has_transient_frame_callbacks()
    }

    // The time of an input sample at the robot's frame clock, in both clocks of the shell. A
    // gesture's velocity and the animations that the input starts read the robot's time and never
    // the machine's, so a busy machine sees the same gesture as an idle one.
    fn sample_time(&self) -> PointerEventTime {
        let time = self
            .shell
            .exact_pointer_event_time(i64::try_from(self.frame_time_nanos / 1_000_000).ok());
        PointerEventTime {
            animation_time_nanos: time.animation_time_nanos.max(self.frame_time_nanos),
            ..time
        }
    }

    // Processes the input at the robot's frame time, which delivers no frame callback.
    fn update_now(&mut self) {
        self.shell.update_at_frame_time_nanos(self.frame_time_nanos);
    }

    fn move_pointer(&mut self, x: f32, y: f32) -> bool {
        let time = self.sample_time();
        self.shell.set_cursor_at_event_time(x, y, time)
    }

    fn press(&mut self) -> bool {
        let time = self.sample_time();
        self.shell.pointer_pressed_at_event_time(time)
    }

    fn release(&mut self) -> bool {
        let time = self.sample_time();
        self.shell.pointer_released_at_event_time(time)
    }

    /// Perform a click at the given coordinates.
    ///
    /// Returns true if the click hit a UI element, false otherwise.
    pub fn click_at(&mut self, x: f32, y: f32) -> bool {
        self.move_pointer(x, y);
        let pressed = self.press();
        let released = self.release();
        self.wait_for_idle();
        pressed || released
    }

    /// Move the cursor to the given coordinates.
    ///
    /// Returns true if the move hit a UI element, false otherwise.
    pub fn move_to(&mut self, x: f32, y: f32) -> bool {
        let hit = self.move_pointer(x, y);
        self.wait_for_idle();
        hit
    }

    /// Perform a drag from one point to another.
    ///
    /// This simulates a pointer down, ten moves and an up, the moves 8 ms apart on the robot's
    /// frame clock, as a 120 Hz touch panel reports them. The drag ends with the velocity of
    /// that pace, however long the machine takes for each move.
    pub fn drag(&mut self, from_x: f32, from_y: f32, to_x: f32, to_y: f32) {
        self.move_pointer(from_x, from_y);
        self.press();

        let steps = 10;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let x = from_x + (to_x - from_x) * t;
            let y = from_y + (to_y - from_y) * t;
            self.frame_time_nanos = self.frame_time_nanos.saturating_add(DRAG_SAMPLE_NANOS);
            self.move_pointer(x, y);
            self.update_now();
        }

        self.release();
        self.wait_for_idle();
    }

    /// Move the mouse cursor to the given coordinates.
    pub fn mouse_move(&mut self, x: f32, y: f32) {
        self.move_pointer(x, y);
        self.update_now();
    }

    /// Press the mouse button.
    pub fn mouse_down(&mut self) {
        self.press();
        self.update_now();
    }

    /// Release the mouse button.
    pub fn mouse_up(&mut self) {
        self.release();
        self.update_now();
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

    /// Find the element that carries `tag` (`Modifier::test_tag`), which keeps
    /// finding it when its text changes.
    pub fn find_by_tag(&mut self, tag: &str) -> ElementFinder<'_, R> {
        self.shell.set_semantics_enabled(true);
        self.wait_for_idle();
        ElementFinder {
            robot: self,
            query: FinderQuery::Tag(tag.to_string()),
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

        self.shell
            .with_layout_tree(|layout_tree| layout_tree.map_or_default(extract_text_from_layout))
    }

    /// Get all rectangles (bounds) of UI elements on screen.
    ///
    /// Returns a list of (bounds, optional_text) tuples.
    pub fn get_all_rects(&mut self) -> Vec<(Rect, Option<String>)> {
        self.wait_for_idle();

        self.shell
            .with_layout_tree(|layout_tree| layout_tree.map_or_default(extract_rects_from_layout))
    }

    /// Print debug information about the current screen state.
    ///
    /// This outputs the layout tree and render scene for debugging.
    pub fn dump_screen(&mut self) {
        self.shell.log_debug_info();
    }

    /// Fails the test with every issue [`crate::audit_accessibility`] finds on
    /// what the shell shows once it is idle.
    pub fn assert_accessible(&mut self) {
        self.shell.set_semantics_enabled(true);
        self.wait_for_idle();
        let placed = crate::placed_semantics::placed_semantics_from_shell(&mut self.shell)
            .expect("the shell has no laid out semantics tree yet");
        crate::accessibility_audit::assert_accessible(&placed);
    }

    /// Get access to the underlying app shell for advanced scenarios.
    pub fn shell_mut(&mut self) -> &mut AppShell<R> {
        &mut self.shell
    }

    fn get_scene(&self) -> &R::Scene {
        self.shell.scene()
    }

    fn tagged_bounds(&mut self, tag: &str) -> Option<Rect> {
        let placed = crate::placed_semantics::placed_semantics_from_shell(&mut self.shell)?;
        placed
            .find_by_tag(tag)
            .map(crate::placed_semantics::PlacedSemanticsNode::target_bounds)
    }
}

#[derive(Clone, Debug)]
enum FinderQuery {
    Text(String),
    Tag(String),
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

impl<R> ElementFinder<'_, R>
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
            FinderQuery::Tag(tag) => self.robot.tagged_bounds(tag).is_some(),
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
            FinderQuery::Tag(tag) => self.robot.tagged_bounds(tag),
            FinderQuery::Position(x, y) => {
                let (x, y) = (*x, *y);
                self.robot
                    .get_all_rects()
                    .into_iter()
                    .map(|(rect, _)| rect)
                    .filter(|rect| rect.contains(x, y))
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
    /// This holds the pointer down for 50 frames of the robot's clock before releasing.
    pub fn long_press(&mut self) -> bool {
        if let Some(center) = self.center() {
            self.robot.move_pointer(center.x, center.y);
            self.robot.press();

            for _ in 0..LONG_PRESS_FRAMES {
                self.robot.advance_time(FRAME_NANOS);
            }

            self.robot.release();
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

fn extract_rects_from_layout(layout: &LayoutTree) -> Vec<(Rect, Option<String>)> {
    fn collect_rects(node: &cranpose_ui::LayoutBox, results: &mut Vec<(Rect, Option<String>)>) {
        let text = node
            .node_data
            .modifier_slices()
            .text_content()
            .map(|text| text.to_string());

        let rect = Rect {
            x: node.rect.x,
            y: node.rect.y,
            width: node.rect.width,
            height: node.rect.height,
        };

        results.push((rect, text));

        for child in &node.children {
            collect_rects(child, results);
        }
    }

    let mut results = Vec::new();
    collect_rects(layout.root(), &mut results);
    results
}

/// A headless renderer using the production scene and input geometry.
///
/// Controls receive pointer and accessibility actions without a GPU or window.
#[derive(Default)]
pub struct TestRenderer {
    scene: Scene,
    fonts: Option<TextFonts>,
}

impl TestRenderer {
    /// Creates a renderer that hands the app `fonts` to lay its text out
    /// with.
    pub fn with_fonts(fonts: TextFonts) -> Self {
        Self {
            scene: Scene::default(),
            fonts: Some(fonts),
        }
    }
}

impl Renderer for TestRenderer {
    type Scene = Scene;
    type Error = ();

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        if let Some(fonts) = &self.fonts {
            app_context.set_text_fonts(fonts.resolver());
        }
    }

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.rebuild_from_applier(applier, root);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene.update_from_applier(applier, root, updates);
        Ok(())
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

/// Create a headless robot test rule whose text lays out with `fonts`.
pub fn create_headless_robot_test_with_fonts<F>(
    width: u32,
    height: u32,
    fonts: TextFonts,
    content: F,
) -> RobotTestRule<TestRenderer>
where
    F: FnMut() + 'static,
{
    RobotTestRule::new(width, height, TestRenderer::with_fonts(fonts), content)
}

#[cfg(test)]
#[path = "tests/robot_tests.rs"]
mod tests;
