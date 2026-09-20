mod robot_launch;

use cranpose::{InspectorAction, InspectorMode, Robot};

fn capture(robot: &Robot, name: &str) -> Vec<u8> {
    let picture = robot.screenshot().expect("inspector picture");
    if let Some(directory) = std::env::var_os("CRANPOSE_INSPECTOR_ARTIFACTS") {
        std::fs::create_dir_all(&directory).expect("artifact directory");
        image::save_buffer(
            std::path::Path::new(&directory).join(format!("{name}.png")),
            &picture.pixels,
            picture.width,
            picture.height,
            image::ColorType::Rgba8,
        )
        .expect("save inspector screenshot");
    }
    picture.pixels
}

fn press(robot: &Robot, action: InspectorAction) {
    let state = robot.inspector_state().expect("read inspector");
    let bounds = state
        .controls
        .iter()
        .find(|control| control.action == action)
        .expect("visible inspector control")
        .bounds;
    robot
        .click(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        )
        .expect("press inspector control");
    robot.wait_for_idle().expect("inspector settles");
}

fn main() {
    env_logger::init();
    robot_launch::launch("Developer inspector robot", 900, 700)
        .with_developer_inspector(true)
        .with_test_driver(|robot| {
            robot.set_semantics_enabled(true).expect("enable semantics");
            robot.wait_for_idle().expect("initial layout");
            let before = robot.spoken_tree().expect("published app tree");
            let original = capture(&robot, "collapsed");
            assert!(
                original
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .take(450 * 900)
                    .any(|pixel| pixel[0] < 100 && pixel[1] < 100 && pixel[2] < 100),
                "normal view must include visible app content above the inspector badge"
            );
            assert!(!robot.inspector_state().expect("collapsed state").open);
            robot
                .send_key_with_modifiers("i", true, true, false, false)
                .expect("former inspector shortcut belongs to the app");
            robot.wait_for_idle().expect("shortcut settles");
            assert!(
                !robot.inspector_state().expect("shortcut state").open,
                "the inspector must open only from its floating control"
            );
            press(&robot, InspectorAction::Toggle);
            let state = robot.inspector_state().expect("open state");
            assert!(state.open);
            assert!(state
                .nodes
                .iter()
                .any(|node| node.label.contains("Account, Account")));
            assert!(!format!("{:?}", state.nodes).contains("robot-secret-value"));
            assert!(!format!("{:?}", state.nodes).contains("Decorative secret"));
            let mut pictures = vec![original.clone()];
            for (action, mode) in [
                (InspectorAction::Overlay, InspectorMode::Overlay),
                (InspectorAction::Accessibility, InspectorMode::Accessibility),
                (InspectorAction::Normal, InspectorMode::Normal),
            ] {
                press(&robot, action);
                assert_eq!(robot.inspector_state().expect("mode state").mode, mode);
                assert_eq!(robot.spoken_tree().expect("unchanged tree"), before);
                let picture = capture(&robot, &format!("{mode:?}"));
                assert!(
                    pictures.iter().all(|previous| *previous != picture),
                    "each mode draws a distinct picture"
                );
                pictures.push(picture);
            }
            press(&robot, InspectorAction::Pick);
            let state = robot.inspector_state().expect("picking state");
            let index = state
                .nodes
                .iter()
                .position(|node| node.label.starts_with("Increase"))
                .expect("inspectable button");
            let bounds = state.nodes[index].bounds;
            robot
                .click(
                    bounds.x + bounds.width / 2.0,
                    bounds.y + bounds.height / 2.0,
                )
                .expect("pick without activation");
            robot.wait_for_idle().expect("selection settles");
            let state = robot.inspector_state().expect("selected state");
            assert_eq!(state.selected, Some(index));
            capture(&robot, "selected");
            assert_eq!(
                robot.spoken_tree().expect("picking preserves state"),
                before
            );
            press(&robot, InspectorAction::Toggle);
            assert_eq!(robot.spoken_tree().expect("closing preserves tree"), before);
            assert!(
                capture(&robot, "restored") == original,
                "closing restores the app picture"
            );
            let launcher = robot.inspector_state().expect("launcher").controls[0].bounds;
            robot.drag(launcher.x + 20.0, launcher.y + 20.0, launcher.x - 100.0, launcher.y - 60.0)
                .expect("drag floating control");
            robot.wait_for_idle().expect("launcher moved");
            let moved = robot.inspector_state().expect("moved launcher");
            assert!(!moved.open, "dragging must not open the panel");
            assert_eq!(moved.controls[0].bounds.x, launcher.x - 120.0);
            assert_eq!(moved.controls[0].bounds.y, launcher.y - 80.0);
            assert_eq!(robot.spoken_tree().expect("drag preserves app"), before);
            press(&robot, InspectorAction::Toggle);
            assert!(robot.inspector_state().expect("floating control state").open);
            let state = robot.inspector_state().expect("panel controls");
            let title = state.controls.iter().find(|control| control.action == InspectorAction::Move)
                .expect("draggable title").bounds;
            robot.drag(title.x + 20.0, title.y + 15.0, title.x - 80.0, title.y + 45.0)
                .expect("move floating panel");
            robot.wait_for_idle().expect("panel moved");
            let moved = robot.inspector_state().expect("moved panel");
            let moved_title = moved.controls.iter().find(|control| control.action == InspectorAction::Move)
                .expect("moved title").bounds;
            assert_eq!(moved_title.x, title.x - 100.0);
            assert_eq!(moved_title.y, title.y + 30.0);
            assert_eq!(robot.spoken_tree().expect("panel drag preserves app"), before);
            capture(&robot, "floating");
            robot.send_key("Down").expect("select with keyboard");
            robot.wait_for_idle().expect("keyboard selection");
            assert_eq!(
                robot
                    .inspector_state()
                    .expect("keyboard selection state")
                    .selected,
                Some(0)
            );
            robot
                .click(
                    bounds.x + bounds.width / 2.0,
                    bounds.y + bounds.height / 2.0,
                )
                .expect("activate app while inspecting");
            robot.wait_for_idle().expect("live state update");
            assert!(robot
                .inspector_state()
                .expect("live projection")
                .nodes
                .iter()
                .any(|node| node.label.starts_with("Action count: 1")));
            assert!(robot
                .spoken_tree()
                .expect("updated app tree")
                .contains("Action count: 1"));
            println!("PASS: floating inspector, dragging, rendering, modes, selection, privacy and tree isolation");
            robot.exit().expect("exit inspector robot");
        })
        .run(desktop_app::test_screens::accessibility_robot::AccessibilityRobotScreen);
}
