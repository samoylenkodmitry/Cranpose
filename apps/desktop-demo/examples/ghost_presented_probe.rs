use std::time::Duration;

use cranpose::AppLauncher;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};

fn main() {
    let _ = env_logger::try_init();
    AppLauncher::new()
        .with_title("Ghost Probe")
        .with_size(1600, 1000)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(900));
            let _ = robot.wait_for_idle();

            robot.invoke_app_hook("set-tab", "liquid").expect("liquid");
            settle(&robot, 1200);
            robot.mouse_move(800.0, 500.0).expect("move");
            robot.mouse_scroll(0.0, -520.0).expect("scroll");
            settle(&robot, 900);
            if let Some((_, y, _, h)) = cranpose_testing::find_text_in_semantics(&robot, "Wi-Fi") {
                let toggle_y = y + h * 0.5;
                robot.mouse_move(1538.0, toggle_y).expect("hover thumb");
                robot.mouse_down().expect("press thumb");
                std::thread::sleep(Duration::from_millis(140));
                robot.mouse_up().expect("release thumb");
                std::thread::sleep(Duration::from_millis(60));
            }
            robot
                .invoke_app_hook("set-tab", "markdown")
                .expect("markdown");
            settle(&robot, 1200);
            println!("STAGE:markdown-after-liquid");
            std::thread::sleep(Duration::from_secs(6));

            robot.invoke_app_hook("set-tab", "liquid").expect("liquid");
            settle(&robot, 900);
            robot.mouse_move(800.0, 500.0).expect("move");
            robot.mouse_scroll(0.0, -880.0).expect("scroll");
            settle(&robot, 400);
            if let Some(discover) =
                cranpose_testing::find_button_exact_in_semantics(&robot, "Discover")
            {
                let bar_y = discover.1 + discover.3 * 0.5;
                let start_x = discover.0 + discover.2 * 0.5;
                robot.mouse_move(start_x, bar_y).expect("hover tab");
                robot.mouse_down().expect("press tab");
                for step in 1..=4 {
                    let _ = robot.mouse_move(start_x + step as f32 * 30.0, bar_y);
                    std::thread::sleep(Duration::from_millis(20));
                }
                robot.mouse_up().expect("release tab");
                std::thread::sleep(Duration::from_millis(90));
            }
            robot
                .invoke_app_hook("set-tab", "shaders")
                .expect("shaders");
            settle(&robot, 1200);
            println!("STAGE:shaders-after-liquid");
            std::thread::sleep(Duration::from_secs(6));

            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let tab = match argument.as_str() {
        "shaders" => app::DemoTab::Shaders,
        "liquid" => app::DemoTab::Liquid,
        "markdown" => app::DemoTab::MarkdownViewer,
        _ => return Err(format!("unknown demo tab '{argument}'")),
    };
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed".to_string())?;
    state.set(tab);
    Ok(None)
}

fn settle(robot: &cranpose::Robot, ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
    let _ = robot.wait_for_idle();
}
