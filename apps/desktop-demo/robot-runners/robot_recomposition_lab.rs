mod regression_robot_support;

use std::time::Duration;

use cranpose::AppLauncher;
use desktop_app::app;
use regression_robot_support::{
    click_button, semantics_dump, spawn_timeout, wait_for_text, wait_for_text_prefix,
};

fn expect_text(robot: &cranpose::Robot, text: &str) {
    wait_for_text(robot, text, 40, Duration::from_millis(100))
        .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(robot)));
}

fn text_with_prefix(robot: &cranpose::Robot, prefix: &str) -> String {
    let (_, _, _, _, text) = wait_for_text_prefix(robot, prefix, 40, Duration::from_millis(100))
        .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(robot)));
    text
}

fn trailing_number(text: &str, marker: &str) -> u32 {
    let tail = text
        .split(marker)
        .nth(1)
        .unwrap_or_else(|| panic!("text '{text}' does not contain '{marker}'"));
    tail.trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_else(|err| panic!("no number after '{marker}' in '{text}': {err}"))
}

fn section_composed(robot: &cranpose::Robot, section: &str) -> u32 {
    let text = text_with_prefix(robot, &format!("{section} composed:"));
    trailing_number(&text, "composed:")
}

fn click(robot: &cranpose::Robot, label: &str) {
    click_button(robot, label).unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(robot)));
    let _ = robot.wait_for_idle();
}

fn main() {
    env_logger::init();
    println!("=== Robot Recomposition Lab Test ===");

    AppLauncher::new()
        .with_title("Robot Recomposition Lab Test")
        .with_size(1100, 1500)
        .with_headless(true)
        .with_test_driver(|robot| {
            spawn_timeout(90, "robot_recomposition_lab");

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            expect_text(&robot, "Recomposition Lab");
            expect_text(&robot, "Phase A clicks: 0");
            expect_text(&robot, "gauge sees phase A");
            let phase_a_first = text_with_prefix(&robot, "Phase A card, instance #");
            let phase_a_first_instance = trailing_number(&phase_a_first, "instance #");
            let route_overview_first = text_with_prefix(&robot, "route card for overview");
            let overview_first_instance = trailing_number(&route_overview_first, "instance #");

            let rows_composed_initial = section_composed(&robot, "rows section");
            let route_composed_initial = section_composed(&robot, "route section");
            assert_eq!(
                section_composed(&robot, "footer"),
                1,
                "the footer must compose exactly once at startup\n{}",
                semantics_dump(&robot)
            );

            for _ in 0..3 {
                click(&robot, "Count in phase");
            }
            expect_text(&robot, "Phase A clicks: 3");

            click(&robot, "Flip phase");
            expect_text(&robot, "Phase B clicks: 0");
            expect_text(&robot, "gauge sees phase B");
            let phase_b = text_with_prefix(&robot, "Phase B card, instance #");
            let phase_b_instance = trailing_number(&phase_b, "instance #");
            assert_ne!(
                phase_b_instance, phase_a_first_instance,
                "the else branch must compose its own card instance\n{}",
                semantics_dump(&robot)
            );

            click(&robot, "Flip phase");
            expect_text(&robot, "Phase A clicks: 0");
            expect_text(&robot, "gauge sees phase A");
            let phase_a_second = text_with_prefix(&robot, "Phase A card, instance #");
            let phase_a_second_instance = trailing_number(&phase_a_second, "instance #");
            assert_ne!(
                phase_a_second_instance, phase_a_first_instance,
                "returning to a branch must compose fresh state, not resurrect the old card\n{}",
                semantics_dump(&robot)
            );

            assert_eq!(
                section_composed(&robot, "rows section"),
                rows_composed_initial,
                "phase flips must not recompose the keyed rows section\n{}",
                semantics_dump(&robot)
            );
            assert_eq!(
                section_composed(&robot, "route section"),
                route_composed_initial,
                "phase flips must not recompose the route section\n{}",
                semantics_dump(&robot)
            );

            click(&robot, "Route detail");
            expect_text(&robot, "detail extra line");
            let detail = text_with_prefix(&robot, "route card for detail");
            let detail_instance = trailing_number(&detail, "instance #");
            assert_ne!(
                detail_instance, overview_first_instance,
                "the detail arm must own its card instance\n{}",
                semantics_dump(&robot)
            );

            click(&robot, "Route settings");
            let settings = text_with_prefix(&robot, "route card for settings");
            let settings_instance = trailing_number(&settings, "instance #");
            assert_ne!(
                settings_instance, detail_instance,
                "the settings arm must own its card instance\n{}",
                semantics_dump(&robot)
            );

            click(&robot, "Route overview");
            let overview_again = text_with_prefix(&robot, "route card for overview");
            let overview_again_instance = trailing_number(&overview_again, "instance #");
            assert_ne!(
                overview_again_instance, overview_first_instance,
                "revisiting an arm must compose fresh state\n{}",
                semantics_dump(&robot)
            );

            let rows_composed_before_bumps = section_composed(&robot, "rows section");
            click(&robot, "Row 2 bump");
            click(&robot, "Row 2 bump");
            click(&robot, "Row 3 bump");
            expect_text(&robot, "Row 2 count: 2");
            expect_text(&robot, "Row 3 count: 1");
            assert_eq!(
                section_composed(&robot, "rows section"),
                rows_composed_before_bumps,
                "bumping a row must recompose the row alone, not its section\n{}",
                semantics_dump(&robot)
            );
            let row2 = text_with_prefix(&robot, "Row 2 count: 2");
            let row2_instance = trailing_number(&row2, "instance #");

            click(&robot, "Toggle row 1");
            expect_text(&robot, "Row 2 count: 2");
            expect_text(&robot, "Row 3 count: 1");
            let row2_after_hide = text_with_prefix(&robot, "Row 2 count: 2");
            assert_eq!(
                trailing_number(&row2_after_hide, "instance #"),
                row2_instance,
                "hiding row 1 must not rebuild row 2\n{}",
                semantics_dump(&robot)
            );

            click(&robot, "Toggle row 1");
            expect_text(&robot, "Row 1 count: 0");
            expect_text(&robot, "Row 2 count: 2");
            expect_text(&robot, "Row 3 count: 1");

            click(&robot, "Row 6 bump");
            click(&robot, "Row 6 bump");
            expect_text(&robot, "Row 6 count: 2");
            let row6 = text_with_prefix(&robot, "Row 6 count: 2");
            let row6_instance = trailing_number(&row6, "instance #");

            click(&robot, "Toggle row 5");
            expect_text(&robot, "Row 6 count: 2");
            let row6_after_hide = text_with_prefix(&robot, "Row 6 count: 2");
            assert_eq!(
                trailing_number(&row6_after_hide, "instance #"),
                row6_instance,
                "hiding row 5 must not rebuild bracketed row 6\n{}",
                semantics_dump(&robot)
            );

            click(&robot, "Toggle row 5");
            expect_text(&robot, "Row 5 count: 0");
            expect_text(&robot, "Row 6 count: 2");

            let gauge = text_with_prefix(&robot, "gauge width:");
            let gauge_width = trailing_number(&gauge, "width:");
            assert!(
                gauge_width > 0,
                "the subcompose gauge must measure a real width, got {gauge_width}\n{}",
                semantics_dump(&robot)
            );
            click(&robot, "Toggle gauge");
            expect_text(&robot, "gauge hidden");
            click(&robot, "Toggle gauge");
            expect_text(&robot, "gauge sees phase A");
            let gauge_back = text_with_prefix(&robot, "gauge width:");
            assert_eq!(
                trailing_number(&gauge_back, "width:"),
                gauge_width,
                "the re-shown gauge must measure the same width\n{}",
                semantics_dump(&robot)
            );

            assert_eq!(
                section_composed(&robot, "footer"),
                1,
                "the footer must never recompose\n{}",
                semantics_dump(&robot)
            );

            println!("✓ Recomposition Lab: branch identity, keyed continuity, subcompose and scope isolation all hold");
            robot.exit().ok();
        })
        .run(|| app::combined_app_with_initial_tab(Some(app::DemoTab::RecompositionLab)));
}
