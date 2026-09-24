use super::*;

#[test]
fn test_robot_creation() {
    let robot = create_headless_robot_test(800, 600, || {});

    assert_eq!(robot.viewport_size(), (800, 600));
}

#[test]
fn test_robot_click() {
    let mut robot = create_headless_robot_test(800, 600, || {});

    robot.click_at(100.0, 100.0);
}

#[test]
fn test_robot_drag() {
    let mut robot = create_headless_robot_test(800, 600, || {});

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
    let mut robot = create_headless_robot_test(800, 600, || {});

    for _ in 0..HEADLESS_IDLE_UPDATE_LIMIT {
        robot.wait_for_idle();
        assert!(!robot.shell_mut().needs_redraw());
    }
}

#[derive(Default)]
struct NeverWarmRenderer {
    inner: TestRenderer,
}

impl Renderer for NeverWarmRenderer {
    type Scene = Scene;
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
    let mut robot = RobotTestRule::new(800, 600, NeverWarmRenderer::default(), || {});

    robot.wait_for_idle();
}
